//! Persistent dependency build manifest for incremental compilation.

use crate::fingerprint::ContentFingerprint;
use crate::graph::{DependencyKind, NodeId};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Record of an input source file in the build manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestSourceFile {
    pub path: String,
    pub fingerprint: ContentFingerprint,
}

/// Record of an imported module in the build manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestModule {
    pub name: String,
    pub path_str: String,
    pub file_path: String,
    pub interface_fingerprint: ContentFingerprint,
}

/// Record of a compiled function in the build manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFunction {
    pub name: String,
    pub module: String,
    pub body_fingerprint: ContentFingerprint,
    pub sig_fingerprint: ContentFingerprint,
}

/// Record of a type interface or layout in the build manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestType {
    pub name: String,
    pub layout_fingerprint: ContentFingerprint,
}

/// Complete persisted build manifest for a project build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildManifest {
    pub schema_version: u32,
    pub compiler_id: String,
    pub target_triple: String,
    pub root_file: String,
    pub config_fingerprint: ContentFingerprint,
    pub source_files: Vec<ManifestSourceFile>,
    pub modules: Vec<ManifestModule>,
    pub functions: Vec<ManifestFunction>,
    pub types: Vec<ManifestType>,
    pub graph_edges: Vec<(NodeId, NodeId, DependencyKind)>,
    pub object_fingerprint: ContentFingerprint,
    pub link_fingerprint: ContentFingerprint,
}

impl BuildManifest {
    pub const MANIFEST_FILENAME: &'static str = "manifest.json";

    pub fn manifest_path(target_cache_dir: &Path) -> PathBuf {
        target_cache_dir.join(Self::MANIFEST_FILENAME)
    }

    pub fn load_from_dir(target_cache_dir: &Path) -> Option<Self> {
        let path = Self::manifest_path(target_cache_dir);
        if !path.exists() {
            return None;
        }
        let bytes = fs::read(&path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save_to_dir(&self, target_cache_dir: &Path) -> Result<(), String> {
        fs::create_dir_all(target_cache_dir)
            .map_err(|e| format!("Failed to create target cache dir: {}", e))?;
        let path = Self::manifest_path(target_cache_dir);
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| format!("Failed to serialize manifest: {}", e))?;
        fs::write(&path, bytes).map_err(|e| format!("Failed to write manifest: {}", e))
    }
}
