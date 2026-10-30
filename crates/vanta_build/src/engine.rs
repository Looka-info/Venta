//! Incremental build execution engine and statistics tracker.
//!
//! Orchestrates the build graph, content-addressed cache, change detection,
//! invalidation propagation, and user-facing rebuild explanations.

use crate::cache::{ArtifactType, CacheManager};
use crate::fingerprint::{ContentFingerprint, Domain, FingerprintBuilder};
use crate::graph::BuildGraph;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Configuration options for incremental compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncrementalOptions {
    pub enabled: bool,
    pub cache_dir: PathBuf,
    pub target_triple: String,
    pub opt_level: String,
    pub target_features: String,
    pub debug_info: bool,
    pub explain_rebuild: bool,
    pub build_stats: bool,
}

impl Default for IncrementalOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            cache_dir: PathBuf::from(CacheManager::DEFAULT_CACHE_REL_PATH),
            target_triple: "x86_64-unknown-linux-gnu".into(),
            opt_level: "O1".into(),
            target_features: "baseline".into(),
            debug_info: false,
            explain_rebuild: false,
            build_stats: false,
        }
    }
}

/// Detailed build metrics for reporting and verification.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildStats {
    pub source_files_total: usize,
    pub source_files_unchanged: usize,
    pub source_files_changed: usize,
    pub functions_total: usize,
    pub functions_cached: usize,
    pub functions_recompiled: usize,
    pub objects_reused: usize,
    pub objects_regenerated: usize,
    pub linking_performed: bool,
    pub linking_reason: Option<String>,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub cache_corruptions: usize,
}

/// The incremental build engine driving graph resolution and caching.
pub struct IncrementalBuildEngine {
    options: IncrementalOptions,
    cache: CacheManager,
    graph: BuildGraph,
    stats: BuildStats,
    rebuild_explanations: Vec<(String, String)>,
}

impl IncrementalBuildEngine {
    pub fn new(options: IncrementalOptions) -> Result<Self, String> {
        let cache = CacheManager::new(options.cache_dir.clone(), &options.target_triple)
            .map_err(|e| format!("Failed to initialize incremental cache: {}", e))?;
        Ok(Self {
            options,
            cache,
            graph: BuildGraph::new(),
            stats: BuildStats::default(),
            rebuild_explanations: Vec::new(),
        })
    }

    pub fn options(&self) -> &IncrementalOptions {
        &self.options
    }

    pub fn cache(&self) -> &CacheManager {
        &self.cache
    }

    pub fn cache_mut(&mut self) -> &mut CacheManager {
        &mut self.cache
    }

    pub fn graph(&self) -> &BuildGraph {
        &self.graph
    }

    pub fn graph_mut(&mut self) -> &mut BuildGraph {
        &mut self.graph
    }

    pub fn stats(&self) -> &BuildStats {
        &self.stats
    }

    pub fn stats_mut(&mut self) -> &mut BuildStats {
        &mut self.stats
    }

    pub fn explanations(&self) -> &[(String, String)] {
        &self.rebuild_explanations
    }

    pub fn add_rebuild_reason(&mut self, unit: impl Into<String>, reason: impl Into<String>) {
        self.rebuild_explanations.push((unit.into(), reason.into()));
    }

    /// Compute cryptographic fingerprint for a source file.
    pub fn compute_source_fingerprint(&self, path: &Path, content: &str) -> ContentFingerprint {
        let mut builder = FingerprintBuilder::new(Domain::Source);
        builder.update_str(&path.to_string_lossy());
        builder.update_str(content);
        builder.finish()
    }

    /// Compute fingerprint for an object file given source fingerprint and build configuration.
    pub fn compute_object_fingerprint(&self, source_fp: &ContentFingerprint) -> ContentFingerprint {
        let mut builder = FingerprintBuilder::new(Domain::Object);
        builder.update_fingerprint(source_fp);
        builder.update_str(&self.options.target_triple);
        builder.update_str(&self.options.opt_level);
        builder.update_str(&self.options.target_features);
        builder.update_bool(self.options.debug_info);
        builder.finish()
    }

    /// Compute final link fingerprint for linking an executable.
    pub fn compute_link_fingerprint(
        &self,
        object_fps: &[ContentFingerprint],
        archive_paths: &[PathBuf],
        extra_inputs: &[PathBuf],
    ) -> ContentFingerprint {
        let mut builder = FingerprintBuilder::new(Domain::Link);
        builder.update_str(&self.options.target_triple);
        builder.update_bool(self.options.debug_info);
        builder.update_u32(object_fps.len() as u32);
        for fp in object_fps {
            builder.update_fingerprint(fp);
        }
        for arch in archive_paths {
            builder.update_str(&arch.to_string_lossy());
            if let Ok(meta) = fs::metadata(arch) {
                builder.update_u64(meta.len());
            }
        }
        for extra in extra_inputs {
            builder.update_str(&extra.to_string_lossy());
            if let Ok(meta) = fs::metadata(extra) {
                builder.update_u64(meta.len());
            }
        }
        builder.finish()
    }

    /// Check if a cached object exists and is valid for a compilation unit.
    pub fn query_cached_object(
        &mut self,
        source_path: &Path,
        source_fp: &ContentFingerprint,
    ) -> Option<Vec<u8>> {
        if !self.options.enabled {
            return None;
        }

        let obj_key = self.compute_object_fingerprint(source_fp);
        match self.cache.read_artifact(&obj_key, ArtifactType::Object) {
            Ok(Some(bytes)) => {
                self.stats.cache_hits += 1;
                self.stats.objects_reused += 1;
                self.stats.source_files_unchanged += 1;
                Some(bytes)
            }
            Ok(None) => {
                self.stats.cache_corruptions = self.cache.stats().corruptions_recovered;
                self.stats.cache_misses += 1;
                self.stats.source_files_changed += 1;
                self.add_rebuild_reason(
                    source_path.to_string_lossy(),
                    if self.stats.cache_corruptions > 0 {
                        "cache corruption recovered and rebuilt"
                    } else {
                        "cache miss or content changed"
                    },
                );
                None
            }
            Err(e) => {
                self.stats.cache_corruptions += 1;
                self.stats.cache_misses += 1;
                self.stats.source_files_changed += 1;
                self.add_rebuild_reason(
                    source_path.to_string_lossy(),
                    format!("cache corruption recovered: {}", e),
                );
                None
            }
        }
    }

    /// Store a newly compiled object file into the cache.
    pub fn store_compiled_object(
        &mut self,
        source_fp: &ContentFingerprint,
        obj_bytes: &[u8],
    ) -> Result<PathBuf, String> {
        let obj_key = self.compute_object_fingerprint(source_fp);
        self.stats.objects_regenerated += 1;
        self.cache
            .store_artifact(&obj_key, ArtifactType::Object, obj_bytes, &[*source_fp])
    }

    /// Check if the executable can be reused without relinking.
    pub fn check_link_reusable(&mut self, link_key: &ContentFingerprint, exe_path: &Path) -> bool {
        if !self.options.enabled {
            self.stats.linking_performed = true;
            self.stats.linking_reason = Some("incremental compilation disabled".into());
            return false;
        }

        if !exe_path.exists() {
            self.stats.linking_performed = true;
            self.stats.linking_reason = Some("output executable does not exist".into());
            return false;
        }

        if self.cache.get_cached_link(link_key, exe_path) {
            self.stats.linking_performed = false;
            self.stats.linking_reason = None;
            self.stats.cache_hits += 1;
            true
        } else {
            self.stats.linking_performed = true;
            self.stats.linking_reason = Some("link inputs or settings changed".into());
            self.stats.cache_misses += 1;
            false
        }
    }

    /// Record completed link operation.
    pub fn record_linked_executable(
        &mut self,
        link_key: &ContentFingerprint,
        exe_path: &Path,
        input_fps: &[ContentFingerprint],
    ) -> Result<(), String> {
        self.cache.record_link(link_key, exe_path, input_fps)
    }

    /// Format build stats string for CLI output.
    pub fn format_stats(&self) -> String {
        format!(
            r#"Venta incremental build:
  Source files:
    Total:                 {}
    Unchanged:             {}
    Changed:                {}
  Objects:
    Reused:                 {}
    Regenerated:            {}
  Linking:
    Status:                 {}
    Reason:                 {}
  Cache:
    Hits:                  {}
    Misses:                 {}
    Corrupted recovered:    {}"#,
            self.stats.source_files_total,
            self.stats.source_files_unchanged,
            self.stats.source_files_changed,
            self.stats.objects_reused,
            self.stats.objects_regenerated,
            if self.stats.linking_performed {
                "Performed"
            } else {
                "Skipped (no-op)"
            },
            self.stats
                .linking_reason
                .as_deref()
                .unwrap_or("No link inputs changed"),
            self.stats.cache_hits,
            self.stats.cache_misses,
            self.stats.cache_corruptions,
        )
    }

    /// Format explanation log for CLI output.
    pub fn format_explanations(&self) -> String {
        let mut s = String::new();
        if self.rebuild_explanations.is_empty() {
            s.push_str("All artifacts up to date; zero rebuilds.\n");
        } else {
            s.push_str("Incremental rebuild explanations:\n");
            for (unit, reason) in &self.rebuild_explanations {
                s.push_str(&format!("  - {}: {}\n", unit, reason));
            }
        }
        s
    }
}
