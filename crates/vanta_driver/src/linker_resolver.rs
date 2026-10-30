//! System linker executable resolution, version querying, and downstream linker detection.
//!
//! Provides the `LinkerResolver` trait to allow injecting custom/mock command runners and
//! executable resolvers for deterministic cross-platform unit testing.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

/// Environment variables known to affect compiler driver / linker backend selection.
pub const LINKER_SELECTION_ENV_VARS: &[&str] = &[
    "COMPILER_PATH",
    "GCC_EXEC_PREFIX",
    "LIBRARY_PATH",
    "LD_LIBRARY_PATH",
    "PATH",
];

/// Collects and deterministically sorts relevant environment variables affecting linker selection.
pub fn collect_linker_environment_variables() -> Vec<(String, String)> {
    let mut vars = Vec::new();
    for &key in LINKER_SELECTION_ENV_VARS {
        if let Ok(val) = std::env::var(key) {
            vars.push((key.to_string(), val));
        }
    }
    vars.sort_by(|a, b| a.0.cmp(&b.0));
    vars
}

/// Recorded mock link execution for verifying exact driver arguments, environment, and backend constraints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockExecution {
    pub driver_path: PathBuf,
    pub args: Vec<String>,
    pub env_vars: Vec<(String, String)>,
    pub expected_backend: Option<PathBuf>,
}

/// Normalizes canonical paths by stripping the Windows `\\?\` verbatim prefix if present.
pub fn normalize_canonical_path(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path
    }
}

/// Abstract interface for discovering, inspecting, validating, and executing linker invocations.
pub trait LinkerResolver {
    /// Resolve an executable name or path to its canonical, normalized PathBuf.
    fn resolve_in_path(&self, name_or_path: &str) -> Result<PathBuf, String>;

    /// Read executable contents into bytes.
    fn read_executable_bytes(&self, path: &Path) -> Result<Vec<u8>, String>;

    /// Query version banner as diagnostic metadata.
    fn query_version(&self, exe_path: &Path) -> Option<String>;

    /// Query the downstream linker executable for a compiler driver (e.g., `-print-prog-name=ld`).
    fn query_downstream_linker(
        &self,
        driver_path: &Path,
        driver_name: &str,
        linker_flags: &[String],
    ) -> Result<Option<PathBuf>, String> {
        self.resolve_downstream_backend(driver_path, driver_name, linker_flags, &[])
    }

    /// Authoritatively resolve the downstream linker backend, accounting for all selection flags
    /// (`-fuse-ld`, `-B`, `--sysroot`, `-target`) and environment variables.
    fn resolve_downstream_backend(
        &self,
        driver_path: &Path,
        driver_name: &str,
        linker_flags: &[String],
        env_vars: &[(String, String)],
    ) -> Result<Option<PathBuf>, String>;

    /// Verify via dry-run probe that the driver will invoke the expected backend.
    /// Fails closed if the driver selects a different backend or cannot authoritatively resolve one.
    fn verify_driver_invocation(
        &self,
        driver_path: &Path,
        driver_name: &str,
        linker_flags: &[String],
        expected_backend: &Path,
        env_vars: &[(String, String)],
    ) -> Result<(), String>;

    /// Execute the linking step using the driver and arguments, ensuring execution matches the fingerprinted backend.
    fn execute_link(
        &self,
        driver_path: &Path,
        args: &[String],
        env_vars: &[(String, String)],
        expected_backend: Option<&Path>,
    ) -> Result<(), String>;
}

/// Default system linker resolver using actual system PATH, filesystem reads, and subprocess execution.
#[derive(Debug, Clone, Default)]
pub struct DefaultLinkerResolver;

impl LinkerResolver for DefaultLinkerResolver {
    fn resolve_in_path(&self, name_or_path: &str) -> Result<PathBuf, String> {
        let p = Path::new(name_or_path);
        if name_or_path.contains('/') || name_or_path.contains('\\') {
            if !p.exists() {
                return Err(format!(
                    "Linker executable '{}' does not exist",
                    name_or_path
                ));
            }
            let canonical = fs::canonicalize(p).map_err(|e| {
                format!(
                    "Failed to canonicalize linker executable '{}': {}",
                    name_or_path, e
                )
            })?;
            return Ok(normalize_canonical_path(canonical));
        }

        if let Some(path_var) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join(name_or_path);
                if candidate.is_file() {
                    if let Ok(canonical) = fs::canonicalize(&candidate) {
                        return Ok(normalize_canonical_path(canonical));
                    }
                }
                #[cfg(windows)]
                {
                    for ext in &[".exe", ".cmd", ".bat"] {
                        let cand_ext = dir.join(format!("{}{}", name_or_path, ext));
                        if cand_ext.is_file() {
                            if let Ok(canonical) = fs::canonicalize(&cand_ext) {
                                return Ok(normalize_canonical_path(canonical));
                            }
                        }
                    }
                }
            }
        }

        Err(format!(
            "Linker executable '{}' not found in system PATH",
            name_or_path
        ))
    }

    fn read_executable_bytes(&self, path: &Path) -> Result<Vec<u8>, String> {
        fs::read(path).map_err(|e| {
            format!(
                "Failed to read linker executable '{}': {}",
                path.display(),
                e
            )
        })
    }

    fn query_version(&self, exe_path: &Path) -> Option<String> {
        let is_link_exe = exe_path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("link.exe"))
            .unwrap_or(false);

        let output = if is_link_exe {
            Command::new(exe_path).arg("/?").output().ok()
        } else {
            Command::new(exe_path).arg("--version").output().ok()
        };

        output.and_then(|out| {
            if out.status.success() || is_link_exe {
                let text = if !out.stdout.is_empty() {
                    String::from_utf8_lossy(&out.stdout).to_string()
                } else {
                    String::from_utf8_lossy(&out.stderr).to_string()
                };
                text.lines().next().map(|l| l.trim().to_string())
            } else {
                None
            }
        })
    }

    fn resolve_downstream_backend(
        &self,
        driver_path: &Path,
        driver_name: &str,
        linker_flags: &[String],
        env_vars: &[(String, String)],
    ) -> Result<Option<PathBuf>, String> {
        let name_lower = driver_name.to_lowercase();
        if !name_lower.contains("gcc")
            && !name_lower.contains("clang")
            && !name_lower.contains("g++")
        {
            return Ok(None);
        }

        let mut cmd = Command::new(driver_path);
        let mut pass_next = false;
        for flag in linker_flags {
            if pass_next {
                cmd.arg(flag);
                pass_next = false;
                continue;
            }
            if flag.starts_with("-fuse-ld") {
                cmd.arg(flag);
            } else if flag.starts_with("-B") {
                cmd.arg(flag);
                if flag == "-B" {
                    pass_next = true;
                }
            } else if flag.starts_with("--sysroot") {
                cmd.arg(flag);
                if flag == "--sysroot" {
                    pass_next = true;
                }
            } else if flag.starts_with("-target") || flag.starts_with("--target") {
                cmd.arg(flag);
                if flag == "-target" || flag == "--target" {
                    pass_next = true;
                }
            }
        }

        for (k, v) in env_vars {
            cmd.env(k, v);
        }

        cmd.arg("-print-prog-name=ld");

        let output = cmd.output().map_err(|e| {
            format!(
                "Failed to invoke driver '{}' to query downstream linker: {}",
                driver_path.display(),
                e
            )
        })?;

        if !output.status.success() {
            return Err(format!(
                "Driver '{} -print-prog-name=ld' failed with exit status {}",
                driver_path.display(),
                output.status
            ));
        }

        let prog = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if prog.is_empty() {
            return Ok(None);
        }

        let prog_path = Path::new(&prog);
        if prog.contains('/') || prog.contains('\\') {
            if !prog_path.exists() {
                return Err(format!(
                    "Downstream linker reported by '{}' does not exist: '{}'",
                    driver_path.display(),
                    prog
                ));
            }
            let canonical = fs::canonicalize(prog_path).map_err(|e| {
                format!("Failed to canonicalize downstream linker '{}': {}", prog, e)
            })?;
            Ok(Some(normalize_canonical_path(canonical)))
        } else {
            let canonical = self.resolve_in_path(&prog)?;
            Ok(Some(canonical))
        }
    }

    fn verify_driver_invocation(
        &self,
        driver_path: &Path,
        driver_name: &str,
        linker_flags: &[String],
        expected_backend: &Path,
        env_vars: &[(String, String)],
    ) -> Result<(), String> {
        let actual_backend =
            self.resolve_downstream_backend(driver_path, driver_name, linker_flags, env_vars)?;
        match actual_backend {
            Some(actual) => {
                let actual_norm = normalize_canonical_path(actual);
                let expected_norm = normalize_canonical_path(expected_backend.to_path_buf());
                if actual_norm != expected_norm {
                    return Err(format!(
                        "Driver verification failed: driver '{}' selected backend '{}', but plan expected '{}'",
                        driver_path.display(),
                        actual_norm.display(),
                        expected_norm.display()
                    ));
                }
                Ok(())
            }
            None => Err(format!(
                "Driver verification failed: driver '{}' could not resolve an authoritative backend, but plan expected '{}'",
                driver_path.display(),
                expected_backend.display()
            )),
        }
    }

    fn execute_link(
        &self,
        driver_path: &Path,
        args: &[String],
        env_vars: &[(String, String)],
        expected_backend: Option<&Path>,
    ) -> Result<(), String> {
        if let Some(expected) = expected_backend {
            let driver_name = driver_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            self.verify_driver_invocation(driver_path, driver_name, args, expected, env_vars)?;
        }

        let mut cmd = Command::new(driver_path);
        for arg in args {
            cmd.arg(arg);
        }
        for (k, v) in env_vars {
            cmd.env(k, v);
        }

        let status = cmd
            .status()
            .map_err(|e| format!("Could not invoke linker '{}': {}", driver_path.display(), e))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("Linker exited with status {}", status))
        }
    }
}

/// Injected mock linker resolver for deterministic, host-independent unit and integration testing.
#[derive(Debug, Clone, Default)]
pub struct MockLinkerResolver {
    /// Mapping from command name / path to (canonical_path, bytes, version)
    pub executables: HashMap<String, (PathBuf, Vec<u8>, Option<String>)>,
    /// Mapping from (driver_name, fuse_ld_flag) to downstream linker path or error
    pub downstream_map: HashMap<(String, Option<String>), Result<Option<PathBuf>, String>>,
    /// Mapping for custom flag combinations: (driver_name, flag_prefix) -> Result<Option<PathBuf>, String>
    pub custom_flags_map: HashMap<(String, String), Result<Option<PathBuf>, String>>,
    /// Dedicated dry-run override: expected_backend string -> Result<(), String>
    pub dry_run_override: Option<Result<(), String>>,
    /// History of executed links: records (driver_path, args, env_vars, expected_backend)
    pub executed_links: Arc<Mutex<Vec<MockExecution>>>,
}

impl MockLinkerResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_executable(
        mut self,
        name: &str,
        path: impl Into<PathBuf>,
        bytes: impl Into<Vec<u8>>,
        version: Option<&str>,
    ) -> Self {
        let p = path.into();
        let b = bytes.into();
        let v = version.map(|s| s.to_string());
        self.executables
            .insert(name.to_string(), (p.clone(), b.clone(), v.clone()));
        self.executables
            .insert(p.to_string_lossy().to_string(), (p, b, v));
        self
    }

    pub fn with_downstream(
        mut self,
        driver_name: &str,
        fuse_ld: Option<&str>,
        downstream: Result<Option<PathBuf>, String>,
    ) -> Self {
        self.downstream_map.insert(
            (driver_name.to_string(), fuse_ld.map(|s| s.to_string())),
            downstream,
        );
        self
    }

    pub fn with_flag_downstream(
        mut self,
        driver_name: &str,
        flag_prefix: &str,
        downstream: Result<Option<PathBuf>, String>,
    ) -> Self {
        self.custom_flags_map.insert(
            (driver_name.to_string(), flag_prefix.to_string()),
            downstream,
        );
        self
    }

    pub fn with_dry_run_override(mut self, result: Result<(), String>) -> Self {
        self.dry_run_override = Some(result);
        self
    }

    pub fn get_executed_links(&self) -> Vec<MockExecution> {
        self.executed_links.lock().unwrap().clone()
    }
}

impl LinkerResolver for MockLinkerResolver {
    fn resolve_in_path(&self, name_or_path: &str) -> Result<PathBuf, String> {
        if let Some((p, _, _)) = self.executables.get(name_or_path) {
            Ok(p.clone())
        } else {
            Err(format!(
                "MockLinkerResolver: executable '{}' not found",
                name_or_path
            ))
        }
    }

    fn read_executable_bytes(&self, path: &Path) -> Result<Vec<u8>, String> {
        let path_str = path.to_string_lossy().to_string();
        for (_, (p, bytes, _)) in &self.executables {
            if p == path || p.to_string_lossy() == path_str {
                if !bytes.is_empty() {
                    return Ok(bytes.clone());
                }
            }
        }
        if let Some((_, bytes, _)) = self.executables.get(&path_str) {
            if !bytes.is_empty() {
                return Ok(bytes.clone());
            }
        }
        if path.is_file() {
            return fs::read(path).map_err(|e| e.to_string());
        }
        Err(format!(
            "MockLinkerResolver: failed to read bytes for '{}'",
            path.display()
        ))
    }

    fn query_version(&self, exe_path: &Path) -> Option<String> {
        let path_str = exe_path.to_string_lossy().to_string();
        for (_, (p, _, ver)) in &self.executables {
            if p == exe_path || p.to_string_lossy() == path_str {
                if ver.is_some() {
                    return ver.clone();
                }
            }
        }
        self.executables
            .get(&path_str)
            .and_then(|(_, _, v)| v.clone())
    }

    fn resolve_downstream_backend(
        &self,
        driver_path: &Path,
        driver_name: &str,
        linker_flags: &[String],
        _env_vars: &[(String, String)],
    ) -> Result<Option<PathBuf>, String> {
        let file_name = driver_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        // 1. Check custom_flags_map (e.g. -B flags, --sysroot, etc.)
        for ((drv, flag_pat), res) in &self.custom_flags_map {
            if drv == driver_name || drv == file_name {
                for flag in linker_flags {
                    if flag.starts_with(flag_pat) {
                        return res.clone();
                    }
                }
            }
        }

        // 2. Check downstream_map for -fuse-ld
        let fuse_ld = linker_flags
            .iter()
            .find(|f| f.starts_with("-fuse-ld"))
            .cloned();

        if let Some(res) = self
            .downstream_map
            .get(&(driver_name.to_string(), fuse_ld.clone()))
        {
            return res.clone();
        }
        if !file_name.is_empty() {
            if let Some(res) = self.downstream_map.get(&(file_name.to_string(), fuse_ld)) {
                return res.clone();
            }
        }

        Ok(None)
    }

    fn verify_driver_invocation(
        &self,
        driver_path: &Path,
        driver_name: &str,
        linker_flags: &[String],
        expected_backend: &Path,
        env_vars: &[(String, String)],
    ) -> Result<(), String> {
        if let Some(ref ov) = self.dry_run_override {
            return ov.clone();
        }

        let actual =
            self.resolve_downstream_backend(driver_path, driver_name, linker_flags, env_vars)?;
        match actual {
            Some(act) => {
                let act_norm = normalize_canonical_path(act);
                let exp_norm = normalize_canonical_path(expected_backend.to_path_buf());
                if act_norm != exp_norm {
                    return Err(format!(
                        "Driver verification failed: driver '{}' selected backend '{}', but plan expected '{}'",
                        driver_path.display(),
                        act_norm.display(),
                        exp_norm.display()
                    ));
                }
                Ok(())
            }
            None => Err(format!(
                "Driver verification failed: driver '{}' could not resolve an authoritative backend, but plan expected '{}'",
                driver_path.display(),
                expected_backend.display()
            )),
        }
    }

    fn execute_link(
        &self,
        driver_path: &Path,
        args: &[String],
        env_vars: &[(String, String)],
        expected_backend: Option<&Path>,
    ) -> Result<(), String> {
        if let Some(expected) = expected_backend {
            let driver_name = driver_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            self.verify_driver_invocation(driver_path, driver_name, args, expected, env_vars)?;
        }

        let mut rec = self.executed_links.lock().unwrap();
        rec.push(MockExecution {
            driver_path: driver_path.to_path_buf(),
            args: args.to_vec(),
            env_vars: env_vars.to_vec(),
            expected_backend: expected_backend.map(|p| p.to_path_buf()),
        });
        Ok(())
    }
}
