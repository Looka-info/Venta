//! Persistent, content-addressed, crash-safe artifact cache for Venta.
//!
//! Stores artifacts under domain-separated subdirectories with cryptographic checksums,
//! atomic rename writes, schema versioning, corruption detection, and garbage collection.

use crate::fingerprint::{CompilerIdentity, ContentFingerprint, Domain};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Type of build artifact stored in the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactType {
    Object,
    ModuleInterface,
    Mir,
    FunctionFragment,
    Archive,
    LinkExecutable,
}

impl ArtifactType {
    pub fn subdir_name(&self) -> &'static str {
        match self {
            ArtifactType::Object => "objects",
            ArtifactType::ModuleInterface => "interfaces",
            ArtifactType::Mir => "mir",
            ArtifactType::FunctionFragment => "functions",
            ArtifactType::Archive => "archives",
            ArtifactType::LinkExecutable => "links",
        }
    }

    pub fn file_extension(&self) -> &'static str {
        match self {
            ArtifactType::Object => "obj",
            ArtifactType::ModuleInterface => "vif",
            ArtifactType::Mir => "vmir",
            ArtifactType::FunctionFragment => "vfn",
            ArtifactType::Archive => "a",
            ArtifactType::LinkExecutable => "link",
        }
    }
}

/// Metadata recorded for every cache entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEntryMetadata {
    pub artifact_type: ArtifactType,
    pub schema_version: u32,
    pub cache_key: ContentFingerprint,
    pub compiler_id: String,
    pub target_triple: String,
    pub input_fingerprints: Vec<ContentFingerprint>,
    pub artifact_size: u64,
    pub artifact_checksum: ContentFingerprint,
    pub created_timestamp: u64,
}

/// Verification report of the on-disk cache.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheVerificationReport {
    pub total_entries: usize,
    pub valid_entries: usize,
    pub corrupted_entries: usize,
    pub schema_mismatches: usize,
    pub compiler_mismatches: usize,
    pub missing_data_files: usize,
}

/// Cleanup report after cleaning the cache.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheCleanReport {
    pub files_removed: usize,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
}

/// Runtime stats and metrics for the cache.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_bytes: u64,
    pub object_count: usize,
    pub function_count: usize,
    pub mir_count: usize,
    pub interface_count: usize,
    pub link_count: usize,
    pub archive_count: usize,
    pub hits: usize,
    pub misses: usize,
    pub corruptions_recovered: usize,
}

/// Content-addressed persistent cache manager.
pub struct CacheManager {
    root_dir: PathBuf,
    target_triple: String,
    compiler_identity: CompilerIdentity,
    hit_counter: AtomicUsize,
    miss_counter: AtomicUsize,
    recovery_counter: AtomicUsize,
}

impl CacheManager {
    pub const DEFAULT_CACHE_REL_PATH: &'static str = ".venta/cache/v1";

    pub fn new(root_dir: PathBuf, target_triple: &str) -> io::Result<Self> {
        let compiler_identity = CompilerIdentity::current(target_triple);
        let target_dir = root_dir.join(target_triple);

        // Ensure subdirectories exist
        fs::create_dir_all(target_dir.join("objects"))?;
        fs::create_dir_all(target_dir.join("interfaces"))?;
        fs::create_dir_all(target_dir.join("mir"))?;
        fs::create_dir_all(target_dir.join("functions"))?;
        fs::create_dir_all(target_dir.join("archives"))?;
        fs::create_dir_all(target_dir.join("links"))?;
        fs::create_dir_all(target_dir.join("metadata"))?;
        fs::create_dir_all(target_dir.join("locks"))?;

        Ok(Self {
            root_dir,
            target_triple: target_triple.to_string(),
            compiler_identity,
            hit_counter: AtomicUsize::new(0),
            miss_counter: AtomicUsize::new(0),
            recovery_counter: AtomicUsize::new(0),
        })
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn target_triple(&self) -> &str {
        &self.target_triple
    }

    pub fn target_dir(&self) -> PathBuf {
        self.root_dir.join(&self.target_triple)
    }

    fn metadata_path(&self, key: &ContentFingerprint) -> PathBuf {
        self.target_dir()
            .join("metadata")
            .join(format!("{}.json", key.to_hex()))
    }

    fn artifact_path(&self, key: &ContentFingerprint, artifact_type: ArtifactType) -> PathBuf {
        self.target_dir()
            .join(artifact_type.subdir_name())
            .join(format!(
                "{}.{}",
                key.to_hex(),
                artifact_type.file_extension()
            ))
    }

    /// Read an artifact from cache, verifying schema, compiler identity, and checksum.
    /// Returns Ok(Some(bytes)) on valid hit, Ok(None) on miss or safely handled corruption.
    pub fn read_artifact(
        &self,
        key: &ContentFingerprint,
        expected_type: ArtifactType,
    ) -> Result<Option<Vec<u8>>, String> {
        let meta_path = self.metadata_path(key);
        let art_path = self.artifact_path(key, expected_type);

        if !meta_path.exists() || !art_path.exists() {
            self.miss_counter.fetch_add(1, Ordering::Relaxed);
            return Ok(None);
        }

        // Read and parse metadata
        let meta_bytes = match fs::read(&meta_path) {
            Ok(b) => b,
            Err(_) => {
                self.miss_counter.fetch_add(1, Ordering::Relaxed);
                return Ok(None);
            }
        };

        let metadata: CacheEntryMetadata = match serde_json::from_slice(&meta_bytes) {
            Ok(m) => m,
            Err(_) => {
                self.recovery_counter.fetch_add(1, Ordering::Relaxed);
                self.miss_counter.fetch_add(1, Ordering::Relaxed);
                let _ = fs::remove_file(&meta_path);
                let _ = fs::remove_file(&art_path);
                return Ok(None);
            }
        };

        // Validate schema version
        if metadata.schema_version != CompilerIdentity::CURRENT_CACHE_SCHEMA_VERSION {
            self.miss_counter.fetch_add(1, Ordering::Relaxed);
            return Ok(None);
        }

        // Validate target
        if metadata.target_triple != self.target_triple {
            self.miss_counter.fetch_add(1, Ordering::Relaxed);
            return Ok(None);
        }

        // Validate compiler identity
        let current_compiler_id = self.compiler_identity.fingerprint().to_hex();
        if metadata.compiler_id != current_compiler_id {
            self.miss_counter.fetch_add(1, Ordering::Relaxed);
            return Ok(None);
        }

        // Read artifact bytes
        let data = match fs::read(&art_path) {
            Ok(d) => d,
            Err(_) => {
                self.recovery_counter.fetch_add(1, Ordering::Relaxed);
                self.miss_counter.fetch_add(1, Ordering::Relaxed);
                return Ok(None);
            }
        };

        // Validate size
        if data.len() as u64 != metadata.artifact_size {
            self.recovery_counter.fetch_add(1, Ordering::Relaxed);
            self.miss_counter.fetch_add(1, Ordering::Relaxed);
            let _ = fs::remove_file(&meta_path);
            let _ = fs::remove_file(&art_path);
            return Ok(None);
        }

        // Validate cryptographic checksum
        let actual_checksum = ContentFingerprint::for_bytes(Domain::Object, &data);
        if actual_checksum != metadata.artifact_checksum {
            self.recovery_counter.fetch_add(1, Ordering::Relaxed);
            self.miss_counter.fetch_add(1, Ordering::Relaxed);
            let _ = fs::remove_file(&meta_path);
            let _ = fs::remove_file(&art_path);
            return Ok(None);
        }

        self.hit_counter.fetch_add(1, Ordering::Relaxed);
        Ok(Some(data))
    }

    /// Atomically store an artifact and its metadata in cache.
    pub fn store_artifact(
        &self,
        key: &ContentFingerprint,
        artifact_type: ArtifactType,
        data: &[u8],
        input_fingerprints: &[ContentFingerprint],
    ) -> Result<PathBuf, String> {
        let locks_dir = self.target_dir().join("locks");
        fs::create_dir_all(&locks_dir).map_err(|e| format!("Create locks dir: {}", e))?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();

        let tmp_data_path =
            locks_dir.join(format!("tmp_{}_{}_{}.dat", key.to_hex(), pid, timestamp));
        let tmp_meta_path =
            locks_dir.join(format!("tmp_{}_{}_{}.meta", key.to_hex(), pid, timestamp));

        // 1. Write data file safely to temp file
        {
            let mut file = File::create(&tmp_data_path)
                .map_err(|e| format!("Create tmp data file {}: {}", tmp_data_path.display(), e))?;
            file.write_all(data)
                .map_err(|e| format!("Write tmp data: {}", e))?;
            file.flush().map_err(|e| format!("Flush tmp data: {}", e))?;
        }

        let checksum = ContentFingerprint::for_bytes(Domain::Object, data);
        let metadata = CacheEntryMetadata {
            artifact_type,
            schema_version: CompilerIdentity::CURRENT_CACHE_SCHEMA_VERSION,
            cache_key: *key,
            compiler_id: self.compiler_identity.fingerprint().to_hex(),
            target_triple: self.target_triple.clone(),
            input_fingerprints: input_fingerprints.to_vec(),
            artifact_size: data.len() as u64,
            artifact_checksum: checksum,
            created_timestamp: timestamp as u64,
        };

        // 2. Write metadata file to temp file
        {
            let meta_bytes = serde_json::to_vec_pretty(&metadata)
                .map_err(|e| format!("Serialize metadata: {}", e))?;
            let mut file =
                File::create(&tmp_meta_path).map_err(|e| format!("Create tmp meta file: {}", e))?;
            file.write_all(&meta_bytes)
                .map_err(|e| format!("Write tmp meta: {}", e))?;
            file.flush().map_err(|e| format!("Flush tmp meta: {}", e))?;
        }

        // 3. Atomically rename data file into destination
        let final_art_path = self.artifact_path(key, artifact_type);
        fs::rename(&tmp_data_path, &final_art_path).map_err(|e| {
            let _ = fs::remove_file(&tmp_data_path);
            let _ = fs::remove_file(&tmp_meta_path);
            format!("Rename artifact to {}: {}", final_art_path.display(), e)
        })?;

        // 4. Atomically rename metadata file into destination
        let final_meta_path = self.metadata_path(key);
        fs::rename(&tmp_meta_path, &final_meta_path).map_err(|e| {
            let _ = fs::remove_file(&tmp_meta_path);
            format!("Rename metadata to {}: {}", final_meta_path.display(), e)
        })?;

        Ok(final_art_path)
    }

    /// Retrieve the on-disk path of a cached object file if it exists and is valid.
    pub fn get_cached_object_path(
        &self,
        key: &ContentFingerprint,
    ) -> Result<Option<PathBuf>, String> {
        let art_path = self.artifact_path(key, ArtifactType::Object);
        if art_path.exists() {
            if let Some(_) = self.read_artifact(key, ArtifactType::Object)? {
                return Ok(Some(art_path));
            }
        }
        Ok(None)
    }

    /// Read link record (link fingerprint -> destination executable path and validity).
    pub fn get_cached_link(&self, link_key: &ContentFingerprint, expected_exe_path: &Path) -> bool {
        if !expected_exe_path.exists() {
            self.miss_counter.fetch_add(1, Ordering::Relaxed);
            return false;
        }

        match self.read_artifact(link_key, ArtifactType::LinkExecutable) {
            Ok(Some(bytes)) => {
                let recorded_path = String::from_utf8_lossy(&bytes);
                if Path::new(recorded_path.trim()) == expected_exe_path {
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Record successful link operation for future incremental link skips.
    pub fn record_link(
        &self,
        link_key: &ContentFingerprint,
        exe_path: &Path,
        input_fingerprints: &[ContentFingerprint],
    ) -> Result<(), String> {
        let path_str = exe_path.to_string_lossy().to_string();
        self.store_artifact(
            link_key,
            ArtifactType::LinkExecutable,
            path_str.as_bytes(),
            input_fingerprints,
        )?;
        Ok(())
    }

    /// Verify integrity of all artifacts in cache.
    pub fn verify_cache(&self) -> CacheVerificationReport {
        let mut report = CacheVerificationReport::default();
        let meta_dir = self.target_dir().join("metadata");

        if let Ok(entries) = fs::read_dir(meta_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                report.total_entries += 1;

                let meta_bytes = match fs::read(&path) {
                    Ok(b) => b,
                    Err(_) => {
                        report.corrupted_entries += 1;
                        continue;
                    }
                };

                let meta: CacheEntryMetadata = match serde_json::from_slice(&meta_bytes) {
                    Ok(m) => m,
                    Err(_) => {
                        report.corrupted_entries += 1;
                        continue;
                    }
                };

                if meta.schema_version != CompilerIdentity::CURRENT_CACHE_SCHEMA_VERSION {
                    report.schema_mismatches += 1;
                    continue;
                }

                let current_compiler_id = self.compiler_identity.fingerprint().to_hex();
                if meta.compiler_id != current_compiler_id {
                    report.compiler_mismatches += 1;
                    continue;
                }

                let art_path = self.artifact_path(&meta.cache_key, meta.artifact_type);
                if !art_path.exists() {
                    report.missing_data_files += 1;
                    continue;
                }

                let data = match fs::read(&art_path) {
                    Ok(d) => d,
                    Err(_) => {
                        report.corrupted_entries += 1;
                        continue;
                    }
                };

                if data.len() as u64 != meta.artifact_size {
                    report.corrupted_entries += 1;
                    continue;
                }

                let checksum = ContentFingerprint::for_bytes(Domain::Object, &data);
                if checksum != meta.artifact_checksum {
                    report.corrupted_entries += 1;
                    continue;
                }

                report.valid_entries += 1;
            }
        }

        report
    }

    /// Garbage-collect or clear cache. If `target_filter` is specified, only that target is cleaned.
    pub fn clean_cache(&self, target_filter: Option<&str>) -> CacheCleanReport {
        let mut report = CacheCleanReport::default();

        let targets_to_clean: Vec<PathBuf> = if let Some(target) = target_filter {
            vec![self.root_dir.join(target)]
        } else if let Ok(entries) = fs::read_dir(&self.root_dir) {
            entries.flatten().map(|e| e.path()).collect()
        } else {
            Vec::new()
        };

        for dir in targets_to_clean {
            if dir.is_dir() {
                if let Ok(entries) = fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let sub_path = entry.path();
                        if sub_path.is_dir() {
                            if let Ok(files) = fs::read_dir(&sub_path) {
                                for file in files.flatten() {
                                    let fp = file.path();
                                    if let Ok(meta) = fs::metadata(&fp) {
                                        report.bytes_freed += meta.len();
                                        report.files_removed += 1;
                                    }
                                    let _ = fs::remove_file(fp);
                                }
                            }
                        }
                    }
                }
            }
        }

        report
    }

    /// Gather summary metrics and on-disk size.
    pub fn stats(&self) -> CacheStats {
        let mut stats = CacheStats {
            hits: self.hit_counter.load(Ordering::Relaxed),
            misses: self.miss_counter.load(Ordering::Relaxed),
            corruptions_recovered: self.recovery_counter.load(Ordering::Relaxed),
            ..Default::default()
        };

        let target_dir = self.target_dir();
        for art_type in &[
            ArtifactType::Object,
            ArtifactType::FunctionFragment,
            ArtifactType::Mir,
            ArtifactType::ModuleInterface,
            ArtifactType::LinkExecutable,
            ArtifactType::Archive,
        ] {
            let subdir = target_dir.join(art_type.subdir_name());
            if let Ok(entries) = fs::read_dir(subdir) {
                for entry in entries.flatten() {
                    if let Ok(meta) = entry.metadata() {
                        if meta.is_file() {
                            stats.total_bytes += meta.len();
                            match art_type {
                                ArtifactType::Object => stats.object_count += 1,
                                ArtifactType::FunctionFragment => stats.function_count += 1,
                                ArtifactType::Mir => stats.mir_count += 1,
                                ArtifactType::ModuleInterface => stats.interface_count += 1,
                                ArtifactType::LinkExecutable => stats.link_count += 1,
                                ArtifactType::Archive => stats.archive_count += 1,
                            }
                        }
                    }
                }
            }
        }

        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cache_store_and_read() {
        let dir = tempdir().unwrap();
        let cache = CacheManager::new(dir.path().to_path_buf(), "x86_64-linux").unwrap();

        let key = ContentFingerprint::for_bytes(Domain::Object, b"sample machine code");
        let data = b"machine code bytes 1234";

        let path = cache
            .store_artifact(&key, ArtifactType::Object, data, &[])
            .expect("store");
        assert!(path.exists());

        let read_back = cache
            .read_artifact(&key, ArtifactType::Object)
            .expect("read")
            .expect("hit");
        assert_eq!(read_back, data);
    }

    #[test]
    fn test_corrupted_cache_recovery() {
        let dir = tempdir().unwrap();
        let cache = CacheManager::new(dir.path().to_path_buf(), "x86_64-linux").unwrap();

        let key = ContentFingerprint::for_bytes(Domain::Object, b"sample");
        let data = b"intact data";
        let path = cache
            .store_artifact(&key, ArtifactType::Object, data, &[])
            .unwrap();

        // Corrupt the data file on disk
        fs::write(&path, b"corrupted bytes").unwrap();

        // Reading must detect checksum failure, remove corrupted file, and return None (miss)
        let res = cache.read_artifact(&key, ArtifactType::Object).unwrap();
        assert_eq!(res, None);
        assert!(!path.exists(), "corrupted entry should be cleared");
    }

    #[test]
    fn test_cache_verify_and_clean() {
        let dir = tempdir().unwrap();
        let cache = CacheManager::new(dir.path().to_path_buf(), "x86_64-linux").unwrap();

        let key = ContentFingerprint::for_bytes(Domain::Object, b"test_entry");
        cache
            .store_artifact(&key, ArtifactType::Object, b"valid content", &[])
            .unwrap();

        let verify_report = cache.verify_cache();
        assert_eq!(verify_report.valid_entries, 1);
        assert_eq!(verify_report.corrupted_entries, 0);

        let clean_report = cache.clean_cache(None);
        assert!(clean_report.files_removed >= 1);
        assert!(clean_report.bytes_freed > 0);
    }
}
