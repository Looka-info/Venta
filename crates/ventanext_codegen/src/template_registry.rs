// ============================================================================
// ⚡ VentaNext Framework — Template Registry, Manifest Verification,
// Cryptographic Integrity, and Safe Project Scaffolding Engine
// ============================================================================

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

// ----------------------------------------------------------------------------
// 1. Pure Rust FIPS 180-4 SHA-256 Cryptographic Hash Engine
// ----------------------------------------------------------------------------

pub fn compute_sha256(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut h_val = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h_val
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h_val = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(h_val);
    }

    let mut res = String::with_capacity(64);
    for val in h {
        res.push_str(&format!("{:08x}", val));
    }
    res
}

// ----------------------------------------------------------------------------
// 2. Data Models
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileInventoryEntry {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplatePageEntry {
    pub path: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateManifest {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub version: String,
    pub framework_compatibility: String,
    pub entry_point: String,
    pub preview_asset: String,
    pub download_source: String,
    pub features: Vec<String>,
    pub pages: Vec<TemplatePageEntry>,
    pub dependencies: BTreeMap<String, String>,
    pub file_inventory: Vec<FileInventoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateCardSummary {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub version: String,
    pub framework_compatibility: String,
    pub preview_asset: String,
    pub features: Vec<String>,
    pub page_count: usize,
    pub file_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryCatalog {
    pub version: String,
    pub schema_version: String,
    pub last_updated: String,
    pub categories: Vec<String>,
    pub templates: Vec<TemplateCardSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProjectReport {
    pub template_id: String,
    pub project_name: String,
    pub destination: PathBuf,
    pub files_created: usize,
    pub total_bytes: u64,
    pub entry_point: PathBuf,
    pub from_offline_cache: bool,
}

// ----------------------------------------------------------------------------
// 3. Errors
// ----------------------------------------------------------------------------

#[derive(Debug)]
pub enum TemplateError {
    Io(io::Error),
    Json(serde_json::Error),
    TemplateNotFound(String),
    ManifestMissingField(String),
    IncompatibleFramework {
        required: String,
        actual: String,
    },
    DestinationAlreadyExists(PathBuf),
    PathTraversalDetected(String),
    ChecksumMismatch {
        file: String,
        expected: String,
        actual: String,
    },
    MissingFileInArchive(String),
    InvalidProjectName(String),
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateError::Io(e) => write!(f, "I/O error: {}", e),
            TemplateError::Json(e) => write!(f, "JSON serialization error: {}", e),
            TemplateError::TemplateNotFound(id) => write!(f, "Template not found: '{}'", id),
            TemplateError::ManifestMissingField(field) => {
                write!(f, "Template manifest missing required field: '{}'", field)
            }
            TemplateError::IncompatibleFramework { required, actual } => {
                write!(
                    f,
                    "Incompatible framework version: required '{}', current version is '{}'",
                    required, actual
                )
            }
            TemplateError::DestinationAlreadyExists(p) => {
                write!(f, "Destination directory already exists and is not empty: {:?}. Existing projects are protected from silent overwrite.", p)
            }
            TemplateError::PathTraversalDetected(path) => {
                write!(
                    f,
                    "Path traversal attack detected and rejected for path: '{}'",
                    path
                )
            }
            TemplateError::ChecksumMismatch {
                file,
                expected,
                actual,
            } => {
                write!(f, "Cryptographic integrity failure for '{}': expected SHA-256 '{}', computed '{}'", file, expected, actual)
            }
            TemplateError::MissingFileInArchive(file) => {
                write!(f, "Expected file missing in template archive: '{}'", file)
            }
            TemplateError::InvalidProjectName(name) => write!(
                f,
                "Invalid project name '{}': must be non-empty and alphanumeric",
                name
            ),
        }
    }
}

impl std::error::Error for TemplateError {}

impl From<io::Error> for TemplateError {
    fn from(e: io::Error) -> Self {
        TemplateError::Io(e)
    }
}

impl From<serde_json::Error> for TemplateError {
    fn from(e: serde_json::Error) -> Self {
        TemplateError::Json(e)
    }
}

// ----------------------------------------------------------------------------
// 4. Registry & Manifest Operations
// ----------------------------------------------------------------------------

impl RegistryCatalog {
    pub fn load_from_file(path: &Path) -> Result<Self, TemplateError> {
        let content = fs::read_to_string(path)?;
        let cat: RegistryCatalog = serde_json::from_str(&content)?;
        Ok(cat)
    }

    pub fn get_template(&self, id: &str) -> Option<&TemplateCardSummary> {
        self.templates.iter().find(|t| t.id == id)
    }

    pub fn filter_by_category(&self, category: &str) -> Vec<&TemplateCardSummary> {
        if category.eq_ignore_ascii_case("all") || category.is_empty() {
            self.templates.iter().collect()
        } else {
            self.templates
                .iter()
                .filter(|t| t.category.eq_ignore_ascii_case(category))
                .collect()
        }
    }

    pub fn search(&self, query: &str) -> Vec<&TemplateCardSummary> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.templates.iter().collect();
        }
        self.templates
            .iter()
            .filter(|t| {
                t.id.to_lowercase().contains(&q)
                    || t.name.to_lowercase().contains(&q)
                    || t.description.to_lowercase().contains(&q)
                    || t.category.to_lowercase().contains(&q)
                    || t.features.iter().any(|f| f.to_lowercase().contains(&q))
            })
            .collect()
    }
}

impl TemplateManifest {
    pub fn load_from_file(path: &Path) -> Result<Self, TemplateError> {
        let content = fs::read_to_string(path)?;
        let manifest: TemplateManifest = serde_json::from_str(&content)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), TemplateError> {
        if self.id.trim().is_empty() {
            return Err(TemplateError::ManifestMissingField("id".into()));
        }
        if self.name.trim().is_empty() {
            return Err(TemplateError::ManifestMissingField("name".into()));
        }
        if self.category.trim().is_empty() {
            return Err(TemplateError::ManifestMissingField("category".into()));
        }
        if self.framework_compatibility.trim().is_empty() {
            return Err(TemplateError::ManifestMissingField(
                "framework_compatibility".into(),
            ));
        }
        if self.entry_point.trim().is_empty() {
            return Err(TemplateError::ManifestMissingField("entry_point".into()));
        }
        if self.file_inventory.is_empty() {
            return Err(TemplateError::ManifestMissingField("file_inventory".into()));
        }
        for entry in &self.file_inventory {
            validate_safe_relative_path(&entry.path)?;
            if entry.sha256.len() != 64 {
                return Err(TemplateError::ManifestMissingField(format!(
                    "file_inventory: invalid SHA-256 for '{}'",
                    entry.path
                )));
            }
        }
        Ok(())
    }

    pub fn check_compatibility(&self, current_version: &str) -> Result<bool, TemplateError> {
        check_version_compatibility(&self.framework_compatibility, current_version)
    }
}

// ----------------------------------------------------------------------------
// 5. Security & Path Traversal Validation
// ----------------------------------------------------------------------------

pub fn validate_safe_relative_path(rel_path: &str) -> Result<(), TemplateError> {
    if rel_path.is_empty() {
        return Err(TemplateError::PathTraversalDetected("empty path".into()));
    }
    let path = Path::new(rel_path);
    if path.is_absolute() {
        return Err(TemplateError::PathTraversalDetected(format!(
            "Absolute paths not permitted: '{}'",
            rel_path
        )));
    }

    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Err(TemplateError::PathTraversalDetected(format!(
                    "Parent directory traversal ('..') rejected: '{}'",
                    rel_path
                )));
            }
            Component::Prefix(_) => {
                return Err(TemplateError::PathTraversalDetected(format!(
                    "Path prefix not permitted: '{}'",
                    rel_path
                )));
            }
            Component::RootDir => {
                return Err(TemplateError::PathTraversalDetected(format!(
                    "Root directory marker not permitted: '{}'",
                    rel_path
                )));
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(())
}

pub fn check_version_compatibility(
    required_spec: &str,
    current: &str,
) -> Result<bool, TemplateError> {
    let req = required_spec.trim();
    if req == "*" || req.is_empty() {
        return Ok(true);
    }
    if let Some(min_ver) = req.strip_prefix(">=") {
        let min_clean = min_ver.trim().trim_start_matches('v');
        let cur_clean = current.trim().trim_start_matches('v');
        return Ok(compare_semver_min(cur_clean, min_clean));
    }
    Ok(true)
}

fn compare_semver_min(actual: &str, min_req: &str) -> bool {
    // Basic semver comparator for release candidates and numbers
    let parse_nums = |s: &str| -> (u64, u64, u64) {
        let clean = s.split('-').next().unwrap_or(s);
        let parts: Vec<u64> = clean.split('.').filter_map(|p| p.parse().ok()).collect();
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
        )
    };

    let actual_nums = parse_nums(actual);
    let min_nums = parse_nums(min_req);

    if actual_nums.0 != min_nums.0 {
        return actual_nums.0 >= min_nums.0;
    }
    if actual_nums.1 != min_nums.1 {
        return actual_nums.1 >= min_nums.1;
    }
    actual_nums.2 >= min_nums.2
}

/// Locates the root `templates` directory, searching user-provided paths, environment variables
/// (`VENTANEXT_TEMPLATES_DIR`, `VANTA_TEMPLATES_DIR`), workspace directory trees, executable sibling
/// directories, user data directories, and system paths.
pub fn find_templates_root(custom_dir: Option<&Path>) -> PathBuf {
    if let Some(d) = custom_dir {
        if d.exists() {
            return d.to_path_buf();
        }
    }
    if let Ok(env_path) =
        std::env::var("VENTANEXT_TEMPLATES_DIR").or_else(|_| std::env::var("VANTA_TEMPLATES_DIR"))
    {
        let p = PathBuf::from(env_path);
        if p.exists() && p.join("registry.json").exists() {
            return p;
        }
    }
    let mut candidates = vec![
        PathBuf::from("templates"),
        PathBuf::from("../templates"),
        PathBuf::from("../../templates"),
    ];
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            candidates.push(parent.join("templates"));
            candidates.push(parent.join("../templates"));
            candidates.push(parent.join("../share/ventanext/templates"));
            candidates.push(parent.join("../share/vanta/templates"));
            candidates.push(parent.join("../lib/vanta/templates"));
        }
    }
    // Walk up from current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let mut curr = Some(cwd.as_path());
        while let Some(dir) = curr {
            let candidate = dir.join("templates");
            if candidate.join("registry.json").exists() {
                candidates.push(candidate);
                break;
            }
            curr = dir.parent();
        }
    }
    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        let h = PathBuf::from(home);
        candidates.push(h.join(".local/share/ventanext/templates"));
        candidates.push(h.join(".local/share/vanta/templates"));
        candidates.push(h.join(".ventanext/templates"));
        candidates.push(h.join(".vanta/templates"));
    }
    candidates.push(PathBuf::from("/usr/local/share/ventanext/templates"));
    candidates.push(PathBuf::from("/usr/share/ventanext/templates"));

    // Development fallback using compile-time manifest dir if available
    if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(manifest_dir);
        if let Some(repo_root) = p.parent().and_then(|p| p.parent()) {
            candidates.push(repo_root.join("templates"));
        }
    }

    for c in &candidates {
        if c.exists() && c.join("registry.json").exists() {
            return c.clone();
        }
    }
    PathBuf::from("templates")
}

// ----------------------------------------------------------------------------
// 6. Safe Project Download, Verification & Atomic Scaffolding
// ----------------------------------------------------------------------------

pub fn create_project_from_template(
    template_id: &str,
    dest_dir: &Path,
    project_name: &str,
    templates_root: &Path,
    cache_dir: Option<&Path>,
    current_framework_version: &str,
) -> Result<CreateProjectReport, TemplateError> {
    let clean_name = project_name.trim();
    if clean_name.is_empty()
        || clean_name.contains('/')
        || clean_name.contains('\\')
        || clean_name.contains("..")
    {
        return Err(TemplateError::InvalidProjectName(project_name.to_string()));
    }

    // 1. Existing project protection: do not silently overwrite
    if dest_dir.exists() {
        let is_empty = match fs::read_dir(dest_dir) {
            Ok(mut it) => it.next().is_none(),
            Err(_) => false,
        };
        if !is_empty {
            return Err(TemplateError::DestinationAlreadyExists(
                dest_dir.to_path_buf(),
            ));
        }
    }

    // 2. Locate template manifest (check templates_root, fallback to cache_dir)
    let template_dir = templates_root.join(template_id);
    let mut manifest_path = template_dir.join("template.json");
    let mut from_offline_cache = false;
    if !manifest_path.exists() {
        if let Some(cd) = cache_dir {
            let cached_manifest = cd.join(template_id).join("template.json");
            if cached_manifest.exists() {
                manifest_path = cached_manifest;
                from_offline_cache = true;
            } else {
                return Err(TemplateError::TemplateNotFound(template_id.to_string()));
            }
        } else {
            return Err(TemplateError::TemplateNotFound(template_id.to_string()));
        }
    }

    let manifest = TemplateManifest::load_from_file(&manifest_path)?;

    // 3. Framework compatibility check
    let compatible = manifest.check_compatibility(current_framework_version)?;
    if !compatible {
        return Err(TemplateError::IncompatibleFramework {
            required: manifest.framework_compatibility.clone(),
            actual: current_framework_version.to_string(),
        });
    }

    // 4. Source location (local template directory or offline cache)
    let project_src_dir = template_dir.join("project");
    let actual_src_dir = if project_src_dir.exists() {
        project_src_dir
    } else if let Some(cd) = cache_dir {
        let cached_proj = cd.join(template_id).join("project");
        if cached_proj.exists() {
            from_offline_cache = true;
            cached_proj
        } else {
            return Err(TemplateError::TemplateNotFound(format!(
                "Neither local nor cached project directory found for template '{}'",
                template_id
            )));
        }
    } else {
        return Err(TemplateError::TemplateNotFound(format!(
            "Project directory not found for template '{}'",
            template_id
        )));
    };

    // 5. Pre-verify all checksums and path traversals before touching destination
    for entry in &manifest.file_inventory {
        validate_safe_relative_path(&entry.path)?;
        let src_file = actual_src_dir.join(&entry.path);
        if !src_file.exists() {
            return Err(TemplateError::MissingFileInArchive(entry.path.clone()));
        }
        let data = fs::read(&src_file)?;
        let computed_hash = compute_sha256(&data);
        if computed_hash != entry.sha256.to_lowercase() {
            return Err(TemplateError::ChecksumMismatch {
                file: entry.path.clone(),
                expected: entry.sha256.clone(),
                actual: computed_hash,
            });
        }
    }

    // 6. Safe extraction with atomic rollback on failure
    fs::create_dir_all(dest_dir)?;

    let extraction_result: Result<(usize, u64), TemplateError> = (|| {
        let mut total_files = 0;
        let mut total_bytes = 0;

        for entry in &manifest.file_inventory {
            let src_file = actual_src_dir.join(&entry.path);
            let target_file = dest_dir.join(&entry.path);

            if let Some(parent) = target_file.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut content = fs::read(&src_file)?;

            // Customize package metadata if package.json or ventanext.config.json
            if entry.path == "package.json" || entry.path == "ventanext.config.json" {
                if let Ok(mut json) = serde_json::from_slice::<serde_json::Value>(&content) {
                    if let Some(obj) = json.as_object_mut() {
                        obj.insert(
                            "name".into(),
                            serde_json::Value::String(clean_name.to_string()),
                        );
                    }
                    if let Ok(serialized) = serde_json::to_vec_pretty(&json) {
                        content = serialized;
                    }
                }
            }

            fs::write(&target_file, &content)?;
            total_files += 1;
            total_bytes += content.len() as u64;
        }

        Ok((total_files, total_bytes))
    })();

    match extraction_result {
        Ok((total_files, total_bytes)) => {
            // 7. Update offline cache if provided
            if let Some(cd) = cache_dir {
                let cache_template_dir = cd.join(template_id);
                let cache_manifest = cache_template_dir.join("template.json");
                let cache_proj = cache_template_dir.join("project");
                let _ = fs::create_dir_all(&cache_proj);
                let _ = fs::copy(&manifest_path, &cache_manifest);
                for entry in &manifest.file_inventory {
                    let src = actual_src_dir.join(&entry.path);
                    let dst = cache_proj.join(&entry.path);
                    if let Some(p) = dst.parent() {
                        let _ = fs::create_dir_all(p);
                    }
                    let _ = fs::copy(&src, &dst);
                }
            }

            let entry_point = dest_dir.join(&manifest.entry_point);
            Ok(CreateProjectReport {
                template_id: template_id.to_string(),
                project_name: clean_name.to_string(),
                destination: dest_dir.to_path_buf(),
                files_created: total_files,
                total_bytes,
                entry_point,
                from_offline_cache,
            })
        }
        Err(e) => {
            // Atomic rollback on failure: Clean up partial destination
            let _ = fs::remove_dir_all(dest_dir);
            Err(e)
        }
    }
}

// ----------------------------------------------------------------------------
// 6. Unit Tests for Registry, Security, Cryptography & Scaffolding
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_sha256_known_vectors() {
        assert_eq!(
            compute_sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            compute_sha256(b"hello world"),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        assert_eq!(
            compute_sha256(b"VentaNext Native Framework"),
            "6027b7297dcae9598844b9be2d04e2ab81370f9bf5f31c66836f0aad481560d7"
        );
    }

    #[test]
    fn test_validate_safe_relative_path() {
        assert!(validate_safe_relative_path("app/page.va").is_ok());
        assert!(validate_safe_relative_path("ventanext.config.json").is_ok());
        assert!(validate_safe_relative_path("public/assets/logo.svg").is_ok());

        // Path traversal rejection
        assert!(validate_safe_relative_path("../evil.txt").is_err());
        assert!(validate_safe_relative_path("foo/../../evil.txt").is_err());
        assert!(validate_safe_relative_path("/absolute/path").is_err());
        assert!(validate_safe_relative_path("").is_err());
    }

    #[test]
    fn test_check_version_compatibility() {
        assert!(check_version_compatibility(">=1.0.0-rc.2", "1.0.0-rc.2").unwrap());
        assert!(check_version_compatibility(">=1.0.0-rc.2", "1.0.0").unwrap());
        assert!(check_version_compatibility(">=1.0.0-rc.2", "1.2.3").unwrap());
        assert!(check_version_compatibility(">=1.0.0-rc.2", "2.0.0").unwrap());
        assert!(!check_version_compatibility(">=1.0.0-rc.2", "0.9.5").unwrap());
    }

    #[test]
    fn test_load_and_verify_all_official_templates() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let templates_dir = repo_root.join("templates");
        let registry_path = templates_dir.join("registry.json");
        let catalog = RegistryCatalog::load_from_file(&registry_path)
            .expect("Registry catalog should load cleanly from registry.json");

        assert_eq!(catalog.templates.len(), 6);
        let expected_ids = [
            "modern-saas",
            "developer-portfolio",
            "agency-business",
            "ecommerce-storefront",
            "admin-dashboard",
            "docs-website",
        ];

        for id in &expected_ids {
            let summary = catalog
                .get_template(id)
                .unwrap_or_else(|| panic!("Template {} must exist", id));
            assert_eq!(summary.id, *id);
            assert!(summary.file_count >= 10);
            assert!(
                check_version_compatibility(&summary.framework_compatibility, "1.0.0-rc.2")
                    .unwrap()
            );

            // Verify individual manifest and SHA-256 integrity
            let manifest_path = templates_dir.join(id).join("template.json");
            let manifest = TemplateManifest::load_from_file(&manifest_path)
                .unwrap_or_else(|_| panic!("Manifest for {} must load", id));
            assert_eq!(manifest.id, *id);
            assert_eq!(manifest.file_inventory.len(), summary.file_count);

            let project_dir = templates_dir.join(id).join("project");
            for file_entry in &manifest.file_inventory {
                let file_path = project_dir.join(&file_entry.path);
                assert!(
                    file_path.exists(),
                    "File {} in template {} must exist",
                    file_entry.path,
                    id
                );
                let content = fs::read(&file_path).unwrap();
                let computed_hash = compute_sha256(&content);
                assert_eq!(
                    computed_hash, file_entry.sha256,
                    "SHA-256 checksum mismatch for {} in {}",
                    file_entry.path, id
                );
            }
        }
    }

    #[test]
    fn test_create_project_and_metadata_customization() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let templates_dir = repo_root.join("templates");

        let tmp = tempdir().unwrap();
        let dest = tmp.path().join("my-saas-website");

        let report = create_project_from_template(
            "modern-saas",
            &dest,
            "my-saas-website",
            &templates_dir,
            None,
            "1.0.0-rc.2",
        )
        .expect("Scaffolding must succeed");

        assert_eq!(report.project_name, "my-saas-website");
        assert_eq!(report.template_id, "modern-saas");
        assert!(report.files_created >= 12);
        assert!(dest.join("app/page.va").exists());
        assert!(dest.join("app/layout.va").exists());
        assert!(dest.join("ventanext.config.json").exists());

        // Verify package.json customized with project name
        let pkg_content = fs::read_to_string(dest.join("package.json")).unwrap();
        assert!(pkg_content.contains("\"name\": \"my-saas-website\""));

        // Verify ventanext.config.json customized
        let config_content = fs::read_to_string(dest.join("ventanext.config.json")).unwrap();
        assert!(config_content.contains("\"name\": \"my-saas-website\""));
    }

    #[test]
    fn test_destination_already_exists_protection() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let templates_dir = repo_root.join("templates");

        let tmp = tempdir().unwrap();
        let dest = tmp.path().join("occupied_dir");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("precious_file.txt"), "do not overwrite me").unwrap();

        let res = create_project_from_template(
            "modern-saas",
            &dest,
            "occupied",
            &templates_dir,
            None,
            "1.0.0-rc.2",
        );
        assert!(res.is_err());
        let err_str = res.err().unwrap().to_string();
        assert!(err_str.contains("already exists and is not empty"));
        assert!(dest.join("precious_file.txt").exists());
    }

    #[test]
    fn test_atomic_rollback_on_tampered_manifest() {
        let tmp = tempdir().unwrap();
        let dest = tmp.path().join("failing_project");

        let manifest = TemplateManifest {
            id: "fake-template".into(),
            name: "Fake Template".into(),
            description: "Testing rollback".into(),
            category: "Testing".into(),
            version: "1.0.0".into(),
            framework_compatibility: ">=1.0.0".into(),
            entry_point: "app/page.va".into(),
            preview_asset: "preview.svg".into(),
            download_source: "local".into(),
            features: vec![],
            pages: vec![],
            dependencies: BTreeMap::new(),
            file_inventory: vec![FileInventoryEntry {
                path: "app/page.va".into(),
                sha256: "0000000000000000000000000000000000000000000000000000000000000000".into(),
                size_bytes: 10,
            }],
        };

        let fake_templates_root = tmp.path().join("templates");
        let fake_template_dir = fake_templates_root.join("fake-template");
        let fake_proj = fake_template_dir.join("project");
        fs::create_dir_all(fake_proj.join("app")).unwrap();
        fs::write(
            fake_proj.join("app/page.va"),
            "real content with different hash",
        )
        .unwrap();
        let fake_manifest_path = fake_template_dir.join("template.json");
        fs::write(
            &fake_manifest_path,
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();

        let res = create_project_from_template(
            "fake-template",
            &dest,
            "failing",
            &fake_templates_root,
            None,
            "1.0.0",
        );
        assert!(res.is_err());
        // Verify destination directory was completely cleaned up (atomic rollback)
        assert!(!dest.exists());
    }

    #[test]
    fn test_offline_caching() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let templates_dir = repo_root.join("templates");

        let tmp = tempdir().unwrap();
        let cache_dir = tmp.path().join("cache");
        let dest1 = tmp.path().join("cached_project1");
        let dest2 = tmp.path().join("cached_project2");

        let report1 = create_project_from_template(
            "docs-website",
            &dest1,
            "docs-demo",
            &templates_dir,
            Some(&cache_dir),
            "1.0.0-rc.2",
        )
        .expect("First download caches template");

        assert!(!report1.from_offline_cache);
        assert!(cache_dir.join("docs-website/template.json").exists());

        // Second download from cache (pointing to an empty templates directory to simulate offline)
        let empty_templates = tmp.path().join("empty_templates");
        fs::create_dir_all(&empty_templates).unwrap();

        let report2 = create_project_from_template(
            "docs-website",
            &dest2,
            "docs-demo-offline",
            &empty_templates,
            Some(&cache_dir),
            "1.0.0-rc.2",
        )
        .expect("Second download should succeed from offline cache");

        assert!(report2.from_offline_cache);
        assert!(dest2.join("app/page.va").exists());
    }

    #[test]
    fn test_find_templates_root_resolution() {
        let tmp = tempfile::tempdir().unwrap();
        let custom_templates = tmp.path().join("my_templates");
        fs::create_dir_all(&custom_templates).unwrap();
        fs::write(custom_templates.join("registry.json"), "{}").unwrap();

        // 1. Direct custom_dir takes highest precedence
        let resolved = find_templates_root(Some(&custom_templates));
        assert_eq!(resolved, custom_templates);

        // 2. Fallback when custom_dir does not exist continues search
        let nonexistent = tmp.path().join("does_not_exist");
        let fallback = find_templates_root(Some(&nonexistent));
        assert_ne!(fallback, nonexistent);
    }
}
