//! System linker executable resolution, version querying, and downstream linker detection.
//!
//! Provides the `LinkerResolver` trait to allow injecting custom/mock command runners and
//! executable resolvers for deterministic cross-platform unit testing.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Normalizes canonical paths by stripping the Windows `\\?\` verbatim prefix if present.
pub fn normalize_canonical_path(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path
    }
}

/// Abstract interface for discovering, inspecting, and reading effective linker executables.
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
    ) -> Result<Option<PathBuf>, String>;
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

    fn query_downstream_linker(
        &self,
        driver_path: &Path,
        driver_name: &str,
        linker_flags: &[String],
    ) -> Result<Option<PathBuf>, String> {
        let name_lower = driver_name.to_lowercase();
        if !name_lower.contains("gcc")
            && !name_lower.contains("clang")
            && !name_lower.contains("g++")
        {
            return Ok(None);
        }

        let mut cmd = Command::new(driver_path);
        for flag in linker_flags {
            if flag.starts_with("-fuse-ld") {
                cmd.arg(flag);
            }
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
}

/// Injected mock linker resolver for deterministic, host-independent unit and integration testing.
#[derive(Debug, Clone, Default)]
pub struct MockLinkerResolver {
    /// Mapping from command name / path to (canonical_path, bytes, version)
    pub executables: HashMap<String, (PathBuf, Vec<u8>, Option<String>)>,
    /// Mapping from (driver_name, fuse_ld_flag) to downstream linker path or error
    pub downstream_map: HashMap<(String, Option<String>), Result<Option<PathBuf>, String>>,
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

    fn query_downstream_linker(
        &self,
        driver_path: &Path,
        driver_name: &str,
        linker_flags: &[String],
    ) -> Result<Option<PathBuf>, String> {
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
        if let Some(file_name) = driver_path.file_name().and_then(|s| s.to_str()) {
            if let Some(res) = self.downstream_map.get(&(file_name.to_string(), fuse_ld)) {
                return res.clone();
            }
        }

        Ok(None)
    }
}
