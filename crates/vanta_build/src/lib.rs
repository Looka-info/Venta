//! Venta build graph, content-addressed artifact cache, and incremental compilation engine.

pub mod cache;
pub mod engine;
pub mod fingerprint;
pub mod graph;

pub use cache::{
    ArtifactType, CacheCleanReport, CacheEntryMetadata, CacheManager, CacheStats,
    CacheVerificationReport,
};
pub use engine::{BuildStats, IncrementalBuildEngine, IncrementalOptions};
pub use fingerprint::{CompilerIdentity, ContentFingerprint, Domain, FingerprintBuilder};
pub use graph::{BuildGraph, BuildNode, DependencyKind, InvalidationReason, NodeId, NodeKind};
