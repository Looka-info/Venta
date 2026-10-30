//! Codegen function fragment representation for function-level caching and object reassembly.

use crate::fingerprint::ContentFingerprint;
use serde::{Deserialize, Serialize};

/// Relocation inside a function fragment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelocationRecord {
    pub offset: usize,
    pub symbol: String,
    pub addend: i64,
    pub r_type: u32,
}

/// Source line table entry for a function fragment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineEntryRecord {
    pub file_idx: u32,
    pub line: u32,
    pub column: u32,
    pub code_offset: usize,
    pub is_stmt: bool,
}

/// Local variable debug metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariableDebugInfoRecord {
    pub name: String,
    pub type_name: String,
    pub file_idx: u32,
    pub line: u32,
    pub stack_offset: i32,
    pub is_param: bool,
}

/// Function-level debug metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionDebugInfoRecord {
    pub name: String,
    pub file_idx: u32,
    pub line: u32,
    pub code_offset: usize,
    pub size: usize,
    pub return_type: Option<String>,
    pub variables: Vec<VariableDebugInfoRecord>,
}

/// Compiled machine-code fragment for an individual function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionFragment {
    pub name: String,
    pub sig_fingerprint: ContentFingerprint,
    pub body_fingerprint: ContentFingerprint,
    pub code: Vec<u8>,
    pub relocations: Vec<RelocationRecord>,
    pub line_entries: Vec<LineEntryRecord>,
    pub debug_info: Option<FunctionDebugInfoRecord>,
    pub alignment: u32,
}

impl FunctionFragment {
    pub fn new(
        name: String,
        sig_fingerprint: ContentFingerprint,
        body_fingerprint: ContentFingerprint,
        code: Vec<u8>,
        relocations: Vec<RelocationRecord>,
        line_entries: Vec<LineEntryRecord>,
        debug_info: Option<FunctionDebugInfoRecord>,
        alignment: u32,
    ) -> Self {
        Self {
            name,
            sig_fingerprint,
            body_fingerprint,
            code,
            relocations,
            line_entries,
            debug_info,
            alignment,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(self).map_err(|e| format!("Failed to serialize FunctionFragment: {}", e))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes)
            .map_err(|e| format!("Failed to deserialize FunctionFragment: {}", e))
    }
}
