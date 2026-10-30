//! Venta build graph, content-addressed artifact cache, and incremental compilation engine.

pub mod cache;
pub mod engine;
pub mod fingerprint;
pub mod fragment;
pub mod graph;
pub mod manifest;
pub mod source_digest;

pub use cache::{
    ArtifactType, CacheCleanReport, CacheEntryMetadata, CacheManager, CacheStats,
    CacheVerificationReport, LinkRecord,
};
pub use engine::{BuildStats, IncrementalBuildEngine, IncrementalOptions};
pub use fingerprint::{
    BuildConfiguration, CompilerIdentity, ContentFingerprint, Domain, FingerprintBuilder,
    LinkConfiguration, LinkInputKind, ResolvedLibrary, ResolvedLinkInput, TargetFeaturesConfig,
};
pub use fragment::{
    FunctionDebugInfoRecord, FunctionFragment, LineEntryRecord, RelocationRecord,
    VariableDebugInfoRecord,
};
pub use graph::{BuildGraph, BuildNode, DependencyKind, InvalidationReason, NodeId, NodeKind};
pub use manifest::{
    BuildManifest, ManifestFunction, ManifestModule, ManifestSourceFile, ManifestType,
};
pub use source_digest::{
    compute_source_tree_digest, compute_source_tree_digest_with_reader, is_valid_digest,
    scan_compiler_source_files, SourceTreeDigest,
};
