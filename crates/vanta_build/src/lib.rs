//! Venta build graph, content-addressed artifact cache, and incremental compilation engine.

pub mod cache;
pub mod engine;
pub mod fingerprint;
pub mod fragment;
pub mod graph;
pub mod manifest;

pub use cache::{
    ArtifactType, CacheCleanReport, CacheEntryMetadata, CacheManager, CacheStats,
    CacheVerificationReport, LinkRecord,
};
pub use engine::{BuildStats, IncrementalBuildEngine, IncrementalOptions};
pub use fingerprint::{
    BuildConfiguration, CompilerIdentity, ContentFingerprint, Domain, FingerprintBuilder,
    TargetFeaturesConfig,
};
pub use fragment::{
    FunctionDebugInfoRecord, FunctionFragment, LineEntryRecord, RelocationRecord,
    VariableDebugInfoRecord,
};
pub use graph::{BuildGraph, BuildNode, DependencyKind, InvalidationReason, NodeId, NodeKind};
pub use manifest::{
    BuildManifest, ManifestFunction, ManifestModule, ManifestSourceFile, ManifestType,
};
