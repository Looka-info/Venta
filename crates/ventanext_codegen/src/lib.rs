// ============================================================================
// ⚡ VentaNext Framework — Build-Time Route Discovery & Code Generator
// Stage 1 Host-Side Generator: Scans app/ directory, parses route segments,
// validates naming, detects conflicts, and outputs deterministic RouteManifest
// and pure Venta static dispatch code.
// ============================================================================

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use vanta_diagnostics::{DiagnosticBag, Severity};
use vanta_lexer::{Lexer, TokenKind};
use vanta_parser::ast::{Item, PrimType, TypeExpr, TypeExprKind};
use vanta_parser::Parser;
use vanta_source::{FileId, SourceFile};

pub const MANIFEST_SCHEMA_VERSION: &str = "1.0.0";

/// Official 3D Gold Lightning vector icon for Venta (.va) files and favicons.
pub const VENTA_FILE_ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32" fill="none">
  <defs>
    <!-- Ambient Gold Glow Filter for Dark Themes -->
    <filter id="vanta-glow" x="-25%" y="-25%" width="150%" height="150%">
      <feDropShadow dx="0" dy="0" stdDeviation="1.2" flood-color="#FFD700" flood-opacity="0.55"/>
      <feDropShadow dx="0" dy="1.5" stdDeviation="2.5" flood-color="#FF9100" flood-opacity="0.35"/>
    </filter>

    <!-- Top-Left Lit Facet Gradient -->
    <linearGradient id="vanta-lit" x1="20%" y1="0%" x2="80%" y2="100%">
      <stop offset="0%" stop-color="#FFFDF0"/>
      <stop offset="25%" stop-color="#FFE838"/>
      <stop offset="60%" stop-color="#FFC000"/>
      <stop offset="100%" stop-color="#FF9500"/>
    </linearGradient>

    <!-- Bottom-Right Shaded Facet Gradient -->
    <linearGradient id="vanta-shade" x1="10%" y1="0%" x2="90%" y2="100%">
      <stop offset="0%" stop-color="#FFB300"/>
      <stop offset="35%" stop-color="#E67300"/>
      <stop offset="70%" stop-color="#B84A00"/>
      <stop offset="100%" stop-color="#662600"/>
    </linearGradient>

    <!-- Center Crease Specular Ridge -->
    <linearGradient id="vanta-ridge" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#FFFFFF" stop-opacity="0.95"/>
      <stop offset="40%" stop-color="#FFF1A8" stop-opacity="0.8"/>
      <stop offset="85%" stop-color="#FFB703" stop-opacity="0.3"/>
      <stop offset="100%" stop-color="#FB8500" stop-opacity="0.0"/>
    </linearGradient>

    <!-- Chamfer Accent Gradient -->
    <linearGradient id="vanta-chamfer" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#FFF6C2"/>
      <stop offset="100%" stop-color="#FFB703"/>
    </linearGradient>
  </defs>

  <!-- Ambient Glow Layer -->
  <g filter="url(#vanta-glow)">
    <path d="M 25.4 2.5 L 18.3 13.1 L 25.0 14.1 L 6.0 29.5 L 7.4 18.8 L 13.4 19.4 Z" fill="url(#vanta-lit)"/>
  </g>

  <!-- Left/Upper 3D Facet (Brilliant Gold Highlight) -->
  <path d="M 25.4 2.5 L 13.4 19.4 L 7.4 18.8 L 6.0 29.5 C 10.5 22.0 14.0 17.0 18.3 13.1 Z" fill="url(#vanta-lit)"/>

  <!-- Right/Lower 3D Facet (Deep Metallic Amber Shadow) -->
  <path d="M 25.4 2.5 L 18.3 13.1 L 25.0 14.1 L 6.0 29.5 C 14.0 17.0 10.5 22.0 18.3 13.1 Z" fill="url(#vanta-shade)"/>

  <!-- Chamfer Accent Shelf -->
  <path d="M 18.3 13.1 L 13.4 19.4" stroke="url(#vanta-chamfer)" stroke-width="0.75" stroke-linecap="round"/>

  <!-- Center Crease Specular Spine Highlight -->
  <path d="M 25.4 2.5 L 16.5 15.6 L 6.0 29.5" stroke="url(#vanta-ridge)" stroke-width="0.85" stroke-linecap="round"/>
</svg>
"##;

// ----------------------------------------------------------------------------
// Adapter Filename Portability Constants
// ----------------------------------------------------------------------------

/// Maximum supported filename component length for portable filesystems (ext4, NTFS, APFS, FAT32).
pub const MAX_PORTABLE_ADAPTER_FILENAME_LEN: usize = 255;

/// Fixed prefix and suffix wrapper character length in generated adapter filenames
/// (`route_` is 6 characters, `.va` is 3 characters, totaling 9 characters).
pub const ADAPTER_FILENAME_WRAPPER_LEN: usize = 9;

/// Maximum permitted UTF-8 byte length of an API route canonical pattern:
/// (255 - 9) / 2 = 123 bytes.
pub const MAX_PORTABLE_ROUTE_PATTERN_BYTES: usize =
    (MAX_PORTABLE_ADAPTER_FILENAME_LEN - ADAPTER_FILENAME_WRAPPER_LEN) / 2;

/// Calculates the generated adapter filename length for a given canonical route pattern byte length:
/// 9 + 2 * byte_len
pub fn adapter_filename_len_for_bytes(byte_len: usize) -> usize {
    ADAPTER_FILENAME_WRAPPER_LEN + (byte_len * 2)
}

// ----------------------------------------------------------------------------
// 1. Enums and Data Models
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RouteKind {
    Page,
    Api,
    Layout,
    Loading,
    Error,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RenderMode {
    Static = 1,
    Dynamic = 2,
    ISR = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SegmentKind {
    Static = 1,
    Dynamic = 2,
    CatchAll = 3,
    OptionalCatchAll = 4,
    Group = 5,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub raw: String,
    pub kind: SegmentKind,
    pub param_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PageHandlerKind {
    #[default]
    ParamsOnly,
    ParamsWithQuery,
    ContextOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPageHandler {
    pub param_names: Vec<String>,
    pub kind: PageHandlerKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteMetadata {
    pub id: usize,
    pub canonical_pattern: String,
    pub normalized_url: String,
    pub route_kind: RouteKind,
    pub render_mode: RenderMode,
    pub revalidate_seconds: Option<u64>,
    pub source_file: String,
    pub segments: Vec<Segment>,
    pub layout_chain: Vec<String>,
    pub methods: Vec<String>,
    pub specificity: i64,
    #[serde(default)]
    pub has_generic_handler: bool,
    #[serde(default)]
    pub explicit_methods: Vec<String>,
    #[serde(default)]
    pub adapter_module_name: String,
    #[serde(default)]
    pub source_content: String,
    #[serde(default)]
    pub page_handler_kind: PageHandlerKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialFilesMetadata {
    pub root_layout: Option<String>,
    pub loading: Option<String>,
    pub error: Option<String>,
    pub not_found: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteManifestJson {
    pub schema_version: String,
    pub app_id: String,
    pub route_count: usize,
    pub routes: Vec<RouteMetadata>,
    pub special_files: SpecialFilesMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedFile {
    pub url_path: String,
    pub file_path: String,
    pub size_bytes: u64,
    pub content_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportManifest {
    pub schema_version: String,
    pub exported_at: u64,
    pub route_count: usize,
    pub files: Vec<ExportedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteConflictError {
    pub message: String,
    pub first_source: Option<PathBuf>,
    pub second_source: Option<PathBuf>,
}

impl std::fmt::Display for RouteConflictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(ref s1) = self.first_source {
            write!(f, " (first source: {:?})", s1)?;
        }
        if let Some(ref s2) = self.second_source {
            write!(f, " (conflicting source: {:?})", s2)?;
        }
        Ok(())
    }
}

impl std::error::Error for RouteConflictError {}

// ----------------------------------------------------------------------------
// 2. Segment Parsing and Validation
// ----------------------------------------------------------------------------

pub fn parse_segment(raw: &str) -> Result<Segment, RouteConflictError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(RouteConflictError {
            message: "Empty route segment detected".to_string(),
            first_source: None,
            second_source: None,
        });
    }

    // Route Group: (group)
    if s.starts_with('(') && s.ends_with(')') {
        let group_name = &s[1..s.len() - 1];
        if group_name.is_empty() || !is_valid_identifier(group_name) {
            return Err(RouteConflictError {
                message: format!(
                    "Invalid route group name '({})': must be a valid identifier",
                    group_name
                ),
                first_source: None,
                second_source: None,
            });
        }
        return Ok(Segment {
            raw: s.to_string(),
            kind: SegmentKind::Group,
            param_name: Some(group_name.to_string()),
        });
    }

    // Optional Catch-All: [[...param]]
    if s.starts_with("[[...") && s.ends_with("]]") {
        let param_name = &s[5..s.len() - 2];
        if param_name.is_empty() || !is_valid_identifier(param_name) {
            return Err(RouteConflictError {
                message: format!("Invalid optional catch-all parameter name in '{}'", s),
                first_source: None,
                second_source: None,
            });
        }
        return Ok(Segment {
            raw: s.to_string(),
            kind: SegmentKind::OptionalCatchAll,
            param_name: Some(param_name.to_string()),
        });
    }

    // Catch-All: [...param]
    if s.starts_with("[...") && s.ends_with(']') {
        let param_name = &s[4..s.len() - 1];
        if param_name.is_empty() || !is_valid_identifier(param_name) {
            return Err(RouteConflictError {
                message: format!("Invalid catch-all parameter name in '{}'", s),
                first_source: None,
                second_source: None,
            });
        }
        return Ok(Segment {
            raw: s.to_string(),
            kind: SegmentKind::CatchAll,
            param_name: Some(param_name.to_string()),
        });
    }

    // Dynamic: [param]
    if s.starts_with('[') && s.ends_with(']') {
        let param_name = &s[1..s.len() - 1];
        if param_name.is_empty() || !is_valid_identifier(param_name) {
            return Err(RouteConflictError {
                message: format!("Invalid dynamic parameter name in '{}'", s),
                first_source: None,
                second_source: None,
            });
        }
        return Ok(Segment {
            raw: s.to_string(),
            kind: SegmentKind::Dynamic,
            param_name: Some(param_name.to_string()),
        });
    }

    // Reject unescaped/malformed brackets
    if s.contains('[') || s.contains(']') || s.contains('(') || s.contains(')') {
        return Err(RouteConflictError {
            message: format!("Malformed route segment syntax in '{}'", s),
            first_source: None,
            second_source: None,
        });
    }

    // Reject path traversal dot-segments and invalid path characters
    if s == "."
        || s == ".."
        || s.contains('/')
        || s.contains('\\')
        || s.contains('\0')
        || s.eq_ignore_ascii_case("%2e")
        || s.eq_ignore_ascii_case("%2e%2e")
        || s.eq_ignore_ascii_case(".%2e")
        || s.eq_ignore_ascii_case("%2e.")
    {
        return Err(RouteConflictError {
            message: format!(
                "Path traversal dot-segment or invalid path character in '{}'",
                s
            ),
            first_source: None,
            second_source: None,
        });
    }

    // Static segment
    Ok(Segment {
        raw: s.to_string(),
        kind: SegmentKind::Static,
        param_name: None,
    })
}

fn is_valid_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    for c in chars {
        if !c.is_ascii_alphanumeric() && c != '_' {
            return false;
        }
    }
    true
}

// ----------------------------------------------------------------------------
// 3. App Directory Scanner
// ----------------------------------------------------------------------------

pub fn segment_precedence_rank(kind: SegmentKind) -> u8 {
    match kind {
        SegmentKind::Static => 4,
        SegmentKind::Dynamic => 3,
        SegmentKind::CatchAll => 2,
        SegmentKind::OptionalCatchAll => 1,
        SegmentKind::Group => 0,
    }
}

pub fn compare_route_precedence(a: &RouteMetadata, b: &RouteMetadata) -> std::cmp::Ordering {
    let clean_a: Vec<&Segment> = a
        .segments
        .iter()
        .filter(|s| s.kind != SegmentKind::Group)
        .collect();
    let clean_b: Vec<&Segment> = b
        .segments
        .iter()
        .filter(|s| s.kind != SegmentKind::Group)
        .collect();

    let min_len = clean_a.len().min(clean_b.len());
    for i in 0..min_len {
        let rank_a = segment_precedence_rank(clean_a[i].kind);
        let rank_b = segment_precedence_rank(clean_b[i].kind);
        if rank_a != rank_b {
            return rank_a.cmp(&rank_b);
        }
    }

    if clean_a.len() < clean_b.len() {
        if clean_b[min_len].kind == SegmentKind::OptionalCatchAll {
            return std::cmp::Ordering::Greater;
        }
        return std::cmp::Ordering::Less;
    }
    if clean_a.len() > clean_b.len() {
        if clean_a[min_len].kind == SegmentKind::OptionalCatchAll {
            return std::cmp::Ordering::Less;
        }
        return std::cmp::Ordering::Greater;
    }

    std::cmp::Ordering::Equal
}

#[derive(Debug, Clone)]
pub struct DiscoveredApp {
    pub routes: Vec<RouteMetadata>,
    pub special_files: SpecialFilesMetadata,
}

pub fn scan_app_directory(app_dir: &Path) -> Result<DiscoveredApp, RouteConflictError> {
    if !app_dir.is_dir() {
        return Err(RouteConflictError {
            message: format!("App directory does not exist: {:?}", app_dir),
            first_source: Some(app_dir.to_path_buf()),
            second_source: None,
        });
    }

    // Collect all files recursively in deterministic order
    let mut files = Vec::new();
    collect_files_recursive(app_dir, &mut files)?;
    files.sort();

    let mut raw_routes = Vec::new();
    let mut special = SpecialFilesMetadata {
        root_layout: None,
        loading: None,
        error: None,
        not_found: None,
    };

    // Find all layout files for layout chain resolution
    let mut layout_map: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    for path in &files {
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name == "layout.va" {
            let parent = path.parent().unwrap_or(app_dir).to_path_buf();
            layout_map.insert(parent, path.clone());
        }
    }

    // Root layout
    if let Some(rl) = layout_map.get(app_dir) {
        special.root_layout = Some(rl.to_string_lossy().to_string());
    }

    for path in &files {
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let rel_path = path.strip_prefix(app_dir).map_err(|e| RouteConflictError {
            message: format!("Failed to strip prefix from {:?}: {}", path, e),
            first_source: Some(path.clone()),
            second_source: None,
        })?;

        match file_name {
            "page.va" => {
                let route =
                    parse_route_file(app_dir, path, rel_path, RouteKind::Page, &layout_map)?;
                raw_routes.push(route);
            }
            "route.va" => {
                let route = parse_route_file(app_dir, path, rel_path, RouteKind::Api, &layout_map)?;
                raw_routes.push(route);
            }
            "loading.va" => {
                special.loading = Some(path.to_string_lossy().to_string());
            }
            "error.va" => {
                special.error = Some(path.to_string_lossy().to_string());
            }
            "not-found.va" => {
                special.not_found = Some(path.to_string_lossy().to_string());
            }
            _ => {
                // Ignore non-route auxiliary files
            }
        }
    }

    // Validate for collisions and conflicts
    validate_conflicts(&raw_routes)?;

    // Sort routes deterministically by segment-level precedence descending, then canonical_pattern ascending
    raw_routes.sort_by(|a, b| {
        compare_route_precedence(b, a).then_with(|| a.canonical_pattern.cmp(&b.canonical_pattern))
    });

    // Assign final sequential deterministic IDs
    for (idx, r) in raw_routes.iter_mut().enumerate() {
        r.id = idx;
    }

    Ok(DiscoveredApp {
        routes: raw_routes,
        special_files: special,
    })
}

fn collect_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), RouteConflictError> {
    let entries = fs::read_dir(dir).map_err(|e| RouteConflictError {
        message: format!("Failed to read directory {:?}: {}", dir, e),
        first_source: Some(dir.to_path_buf()),
        second_source: None,
    })?;

    let mut sorted_entries = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| RouteConflictError {
            message: format!("Failed to inspect directory entry in {:?}: {}", dir, e),
            first_source: Some(dir.to_path_buf()),
            second_source: None,
        })?;
        sorted_entries.push(entry.path());
    }
    sorted_entries.sort();

    for path in sorted_entries {
        if path.is_dir() {
            collect_files_recursive(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn parse_route_file(
    app_dir: &Path,
    full_path: &Path,
    rel_path: &Path,
    kind: RouteKind,
    layout_map: &BTreeMap<PathBuf, PathBuf>,
) -> Result<RouteMetadata, RouteConflictError> {
    let parent = rel_path.parent().unwrap_or(Path::new(""));
    let mut segments = Vec::new();

    for comp in parent.iter() {
        let comp_str = comp.to_str().unwrap_or("");
        if !comp_str.is_empty() {
            let seg = parse_segment(comp_str).map_err(|mut e| {
                e.first_source = Some(full_path.to_path_buf());
                e
            })?;
            segments.push(seg);
        }
    }

    // Validate catch-all placement: CatchAll and OptionalCatchAll MUST only be at the terminal segment
    for (idx, seg) in segments.iter().enumerate() {
        if (seg.kind == SegmentKind::CatchAll || seg.kind == SegmentKind::OptionalCatchAll)
            && idx != segments.len() - 1
        {
            return Err(RouteConflictError {
                message: format!(
                    "Catch-all segment '{}' must be the last segment in route {:?}",
                    seg.raw, full_path
                ),
                first_source: Some(full_path.to_path_buf()),
                second_source: None,
            });
        }
    }

    // Check for duplicate parameter names in the same route path
    let mut param_names = BTreeSet::new();
    for seg in &segments {
        if let Some(ref p) = seg.param_name {
            if seg.kind != SegmentKind::Group {
                if !param_names.insert(p.clone()) {
                    return Err(RouteConflictError {
                        message: format!(
                            "Duplicate parameter name '[{}]' in route {:?}",
                            p, full_path
                        ),
                        first_source: Some(full_path.to_path_buf()),
                        second_source: None,
                    });
                }
            }
        }
    }

    // Build canonical pattern and normalized URL (excluding route groups)
    let mut canonical_parts = Vec::new();
    for seg in &segments {
        if seg.kind != SegmentKind::Group {
            canonical_parts.push(seg.raw.as_str());
        }
    }

    let canonical_pattern = if canonical_parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", canonical_parts.join("/"))
    };

    let normalized_url = canonical_pattern.clone();

    // Portable adapter filename length validation:
    // Every API route produces an adapter file named `route_{hex}.va` where hex is 2 * pattern_bytes.
    // Portable filesystems limit a filename component to 255 bytes, meaning canonical_pattern
    // cannot exceed MAX_PORTABLE_ROUTE_PATTERN_BYTES (123 bytes).
    if kind == RouteKind::Api {
        let pattern_bytes = canonical_pattern.as_bytes().len();
        if pattern_bytes > MAX_PORTABLE_ROUTE_PATTERN_BYTES {
            let encoded_mod = sanitize_module_name(&canonical_pattern);
            let encoded_filename = format!("{}.va", encoded_mod);
            let actual_filename_len = encoded_filename.len();
            return Err(RouteConflictError {
                message: format!(
                    "API route pattern '{}' in \"{}\" exceeds portable filesystem filename limit: canonical pattern is {} UTF-8 bytes which encodes to adapter filename '{}' ({} bytes). Maximum permitted pattern length is {} UTF-8 bytes (max adapter filename length is {} bytes).",
                    canonical_pattern,
                    full_path.to_string_lossy(),
                    pattern_bytes,
                    encoded_filename,
                    actual_filename_len,
                    MAX_PORTABLE_ROUTE_PATTERN_BYTES,
                    MAX_PORTABLE_ADAPTER_FILENAME_LEN,
                ),
                first_source: Some(full_path.to_path_buf()),
                second_source: None,
            });
        }
    }

    // Compute specificity score:
    // Static = 1000, Dynamic = 100, CatchAll = 10, OptionalCatchAll = 1
    let mut specificity: i64 = 0;
    if canonical_parts.is_empty() {
        specificity = 10000;
    } else {
        for seg in &segments {
            match seg.kind {
                SegmentKind::Static => specificity += 1000,
                SegmentKind::Dynamic => specificity += 100,
                SegmentKind::CatchAll => specificity += 10,
                SegmentKind::OptionalCatchAll => specificity += 1,
                SegmentKind::Group => {}
            }
        }
        specificity += canonical_parts.len() as i64;
    }

    // Resolve layout chain from outermost (app/layout.va) to innermost
    let mut layout_chain = Vec::new();
    let mut current_ancestor = full_path.parent();
    let mut ancestor_chain = Vec::new();
    while let Some(anc) = current_ancestor {
        if anc.starts_with(app_dir) {
            ancestor_chain.push(anc.to_path_buf());
            if anc == app_dir {
                break;
            }
            current_ancestor = anc.parent();
        } else {
            break;
        }
    }
    ancestor_chain.reverse();

    for anc in ancestor_chain {
        if let Some(l_path) = layout_map.get(&anc) {
            layout_chain.push(l_path.to_string_lossy().to_string());
        }
    }

    let dynamic_params: Vec<String> = segments
        .iter()
        .filter(|s| {
            s.kind == SegmentKind::Dynamic
                || s.kind == SegmentKind::CatchAll
                || s.kind == SegmentKind::OptionalCatchAll
        })
        .filter_map(|s| s.param_name.clone())
        .collect();

    let (
        methods,
        explicit_methods,
        has_generic_handler,
        source_content,
        adapter_module_name,
        page_handler_kind,
    ) = if kind == RouteKind::Page {
        let pattern_bytes = canonical_pattern.as_bytes().len();
        let encoded_mod = sanitize_page_module_name(&canonical_pattern);
        let encoded_filename = format!("{}.va", encoded_mod);
        let actual_filename_len = encoded_filename.len();
        if pattern_bytes > MAX_PORTABLE_ROUTE_PATTERN_BYTES
            || actual_filename_len > MAX_PORTABLE_ADAPTER_FILENAME_LEN
        {
            return Err(RouteConflictError {
                    message: format!(
                        "Page route pattern '{}' in \"{}\" exceeds portable filesystem filename limit: canonical pattern is {} UTF-8 bytes which encodes to adapter filename '{}' ({} bytes). Maximum permitted pattern length is {} UTF-8 bytes (max adapter filename length is {} bytes).",
                        canonical_pattern,
                        full_path.to_string_lossy(),
                        pattern_bytes,
                        encoded_filename,
                        actual_filename_len,
                        MAX_PORTABLE_ROUTE_PATTERN_BYTES,
                        MAX_PORTABLE_ADAPTER_FILENAME_LEN,
                    ),
                    first_source: Some(full_path.to_path_buf()),
                    second_source: None,
                });
        }

        let content = fs::read_to_string(full_path).map_err(|e| RouteConflictError {
            message: format!("Failed to read page file {:?}: {}", full_path, e),
            first_source: Some(full_path.to_path_buf()),
            second_source: None,
        })?;

        let validated = validate_page_route_handlers(full_path, &content, &dynamic_params)?;

        (
            vec!["GET".to_string(), "HEAD".to_string()],
            Vec::new(),
            false,
            content,
            encoded_mod,
            validated.kind,
        )
    } else {
        let content = fs::read_to_string(full_path).unwrap_or_default();
        let validated = validate_api_route_handlers(full_path, &content)?;
        let encoded_mod = sanitize_module_name(&canonical_pattern);
        (
            validated.methods,
            validated.explicit_methods,
            validated.has_generic_handler,
            content,
            encoded_mod,
            PageHandlerKind::ParamsOnly,
        )
    };

    let has_dynamic_segments = segments.iter().any(|s| {
        s.kind == SegmentKind::Dynamic
            || s.kind == SegmentKind::CatchAll
            || s.kind == SegmentKind::OptionalCatchAll
    });

    let render_mode = if kind == RouteKind::Api {
        RenderMode::Dynamic
    } else if has_dynamic_segments {
        RenderMode::Dynamic
    } else {
        RenderMode::Static
    };

    Ok(RouteMetadata {
        id: 0, // Assigned after sorting
        canonical_pattern,
        normalized_url,
        route_kind: kind,
        render_mode,
        revalidate_seconds: None,
        source_file: full_path.to_string_lossy().to_string(),
        segments,
        layout_chain,
        methods,
        specificity,
        has_generic_handler,
        explicit_methods,
        adapter_module_name,
        source_content,
        page_handler_kind,
    })
}

// ----------------------------------------------------------------------------
// 3.1. API Method Discovery Parser
// ----------------------------------------------------------------------------

/// Standard candidate HTTP methods recognized for VentaNext API route handlers.
pub const CANDIDATE_HTTP_METHODS: [&str; 7] =
    ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];

/// Discovers top-level exported HTTP handler declarations in an API route file.
///
/// Recognized declarations:
/// - `pub fn GET`
/// - `pub fn POST`
/// - `pub fn PUT`
/// - `pub fn DELETE`
/// - `pub fn PATCH`
/// - `pub fn HEAD`
/// - `pub fn OPTIONS`
///
/// Rules:
/// - Only recognizes top-level declarations (`brace_depth == 0`).
/// - Rejects private declarations (`fn METHOD`).
/// - Rejects method declarations inside line or block comments (stripped by `vanta_lexer`).
/// - Rejects method declarations inside string literals (`"pub fn PUT()"`).
/// - Rejects similar identifiers (`pub fn GETTING()`).
/// - Rejects nested declarations (e.g., inside functions, structs, or impl blocks).
/// - Tolerates ordinary formatting variations, whitespace, and multiline signatures.
/// - If `GET` is declared and `HEAD` is not explicitly declared, `HEAD` is automatically added.
/// - If explicit `HEAD` is declared, it is recognized.
/// - If `handle_api` is declared without explicit HTTP methods, recognizes all 7 standard methods
///   for backward compatibility with generic API handler files (Configuration B).
/// - If no valid handler is found or mixed conventions are used, returns an empty vector.
pub fn discover_api_methods(content: &str) -> Vec<String> {
    let source_file = SourceFile::new(FileId(0), "route.va".to_string(), content.to_string());
    let mut diags = DiagnosticBag::new();
    let mut lexer = Lexer::new(&source_file);
    let tokens = lexer.tokenize(&mut diags);

    if diags.has_errors() {
        return Vec::new();
    }

    let mut found = Vec::new();
    let mut has_handle_api = false;
    let mut brace_depth: usize = 0;

    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i].kind {
            TokenKind::LBrace => {
                brace_depth += 1;
                i += 1;
            }
            TokenKind::RBrace => {
                brace_depth = brace_depth.saturating_sub(1);
                i += 1;
            }
            TokenKind::Pub if brace_depth == 0 => {
                let mut cursor = i + 1;
                if cursor < tokens.len() && tokens[cursor].kind == TokenKind::Unsafe {
                    cursor += 1;
                }
                if cursor < tokens.len() && tokens[cursor].kind == TokenKind::Fn {
                    cursor += 1;
                    if cursor < tokens.len() {
                        if let TokenKind::Ident(ref name) = tokens[cursor].kind {
                            let next_cursor = cursor + 1;
                            let is_fn_sig = next_cursor < tokens.len()
                                && matches!(
                                    tokens[next_cursor].kind,
                                    TokenKind::LParen | TokenKind::Lt
                                );

                            if is_fn_sig {
                                if CANDIDATE_HTTP_METHODS.contains(&name.as_str()) {
                                    if !found.contains(name) {
                                        found.push(name.clone());
                                    }
                                } else if name == "handle_api" {
                                    has_handle_api = true;
                                }
                            }
                        }
                    }
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    // Rejection on mixed or empty handlers
    if found.is_empty() {
        if has_handle_api {
            // Configuration B: Generic handler convention
            return CANDIDATE_HTTP_METHODS
                .iter()
                .map(|&s| s.to_string())
                .collect();
        } else {
            // No valid handler found -> empty
            return Vec::new();
        }
    }

    if has_handle_api {
        // Mixed configuration is invalid -> empty
        return Vec::new();
    }

    // Configuration A: Explicit HTTP handlers
    // GET implies HEAD when HEAD is not explicitly declared
    let has_get = found.iter().any(|m| m == "GET");
    let has_head = found.iter().any(|m| m == "HEAD");
    if has_get && !has_head {
        found.push("HEAD".to_string());
    }

    // Return methods in canonical HTTP method order
    let mut ordered = Vec::new();
    for &candidate in &CANDIDATE_HTTP_METHODS {
        if found.iter().any(|m| m == candidate) {
            ordered.push(candidate.to_string());
        }
    }

    ordered
}

// ----------------------------------------------------------------------------
// 3.2. Static API Route Handler Validation & Adapter Generation
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedApiHandlers {
    pub methods: Vec<String>,
    pub explicit_methods: Vec<String>,
    pub has_generic_handler: bool,
}

/// Encodes a canonical route pattern into a collision-resistant, deterministic Venta module identifier.
///
/// Injective UTF-8 hexadecimal encoding ensures that every distinct canonical pattern
/// (e.g. `/api/users` vs `/api-users` vs `/api_[id]`) maps to a distinct identifier.
/// Identifiers always begin with the valid alphabetic prefix `route_` followed by lowercase hexadecimal characters.
pub fn sanitize_module_name(canonical_pattern: &str) -> String {
    let hex = canonical_pattern
        .as_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    format!("route_{}", hex)
}

/// Validates API route handlers against the Phase 1.3A signature contract.
///
/// Contract:
/// - Configuration A (Explicit HTTP handlers): Top-level exported candidate HTTP methods
///   (`GET`, `POST`, `PUT`, `DELETE`, `PATCH`, `HEAD`, `OPTIONS`) with 0 parameters and `String` return type.
///   GET automatically implies HEAD unless explicit HEAD is declared.
/// - Configuration B (Generic handler): Top-level exported `pub fn handle_api() -> String` with 0 parameters
///   and `String` return type. Retains 7-method compatibility dispatch.
/// - Fail-closed: Lexer or parser errors immediately halt with diagnostics and reject generation.
/// - Mixed configuration: Reject files declaring both `handle_api()` and explicit method handlers.
/// - Empty / unrecognized: Reject files with no valid handlers. Private and unrelated helpers alone are rejected.
pub fn validate_api_route_handlers(
    file_path: &Path,
    content: &str,
) -> Result<ValidatedApiHandlers, RouteConflictError> {
    let source_file = SourceFile::new(
        FileId(0),
        file_path.to_string_lossy().to_string(),
        content.to_string(),
    );
    let mut diags = DiagnosticBag::new();
    let mut lexer = Lexer::new(&source_file);
    let tokens = lexer.tokenize(&mut diags);

    if diags.has_errors() {
        let err_msgs: Vec<String> = diags
            .diagnostics()
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| {
                if let Some(span) = d.primary_span {
                    let (line, col) = source_file.line_col(span.start);
                    format!("{}:{}: {}", line, col, d.message)
                } else {
                    d.message.clone()
                }
            })
            .collect();
        return Err(RouteConflictError {
            message: format!(
                "Lexer error in API route file {:?}: {}",
                file_path,
                err_msgs.join("; ")
            ),
            first_source: Some(file_path.to_path_buf()),
            second_source: None,
        });
    }

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program(&mut diags);

    if diags.has_errors() {
        let err_msgs: Vec<String> = diags
            .diagnostics()
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| {
                if let Some(span) = d.primary_span {
                    let (line, col) = source_file.line_col(span.start);
                    format!("{}:{}: {}", line, col, d.message)
                } else {
                    d.message.clone()
                }
            })
            .collect();
        return Err(RouteConflictError {
            message: format!(
                "Parse error in API route file {:?}: {}",
                file_path,
                err_msgs.join("; ")
            ),
            first_source: Some(file_path.to_path_buf()),
            second_source: None,
        });
    }

    let mut explicit_methods = Vec::new();
    let mut has_generic_handler = false;

    for item in &program.items {
        if let Item::Function(f) = item {
            if !f.is_pub {
                continue;
            }
            let fn_name = &f.name.name;
            let is_candidate_method = CANDIDATE_HTTP_METHODS.contains(&fn_name.as_str());
            let is_handle_api = fn_name == "handle_api";

            if is_candidate_method || is_handle_api {
                // 1. Check parameter count: must be zero
                if !f.params.is_empty() {
                    return Err(RouteConflictError {
                        message: format!(
                            "Unsupported handler signature in {:?}: '{}' has {} parameter(s). Only zero-argument handlers returning 'String' (e.g. 'pub fn {}() -> String') are currently supported in Phase 1.3A.",
                            file_path, fn_name, f.params.len(), fn_name
                        ),
                        first_source: Some(file_path.to_path_buf()),
                        second_source: None,
                    });
                }

                // 2. Check return type: must be String
                let returns_string = match &f.ret_type {
                    Some(type_expr) => match &type_expr.kind {
                        TypeExprKind::Named(name) => {
                            name == "String" || name == "std::string::String"
                        }
                        _ => false,
                    },
                    None => false,
                };

                if !returns_string {
                    return Err(RouteConflictError {
                        message: format!(
                            "Unsupported return type in {:?}: handler '{}' must return 'String', found unsupported return type. Only handlers returning 'String' (e.g. 'pub fn {}() -> String') are currently supported in Phase 1.3A.",
                            file_path, fn_name, fn_name
                        ),
                        first_source: Some(file_path.to_path_buf()),
                        second_source: None,
                    });
                }

                if is_handle_api {
                    has_generic_handler = true;
                } else if !explicit_methods.contains(fn_name) {
                    explicit_methods.push(fn_name.clone());
                }
            }
        }
    }

    // Check mixed configuration: cannot combine handle_api and explicit HTTP method handlers
    if has_generic_handler && !explicit_methods.is_empty() {
        return Err(RouteConflictError {
            message: format!(
                "Mixed API handler conventions in {:?}: route contains both generic 'pub fn handle_api()' and explicit HTTP handler(s) ({}). An API route must use either explicit method handlers or the generic handler convention, not both.",
                file_path,
                explicit_methods.join(", ")
            ),
            first_source: Some(file_path.to_path_buf()),
            second_source: None,
        });
    }

    // Fail-closed validation: route MUST contain at least one valid handler (Configuration A or B)
    if !has_generic_handler && explicit_methods.is_empty() {
        return Err(RouteConflictError {
            message: format!(
                "No valid API handler found in {:?}. An API route must export either explicit HTTP method handlers (e.g. 'pub fn GET() -> String') or a generic handler ('pub fn handle_api() -> String'). Private functions and unrelated helpers are not HTTP handlers.",
                file_path
            ),
            first_source: Some(file_path.to_path_buf()),
            second_source: None,
        });
    }

    let methods = if has_generic_handler {
        CANDIDATE_HTTP_METHODS
            .iter()
            .map(|&s| s.to_string())
            .collect()
    } else {
        let mut m = explicit_methods.clone();
        if m.contains(&"GET".to_string()) && !m.contains(&"HEAD".to_string()) {
            m.push("HEAD".to_string());
        }
        let mut ordered = Vec::new();
        for &cand in &CANDIDATE_HTTP_METHODS {
            if m.contains(&cand.to_string()) {
                ordered.push(cand.to_string());
            }
        }
        ordered
    };

    Ok(ValidatedApiHandlers {
        methods,
        explicit_methods,
        has_generic_handler,
    })
}

/// Generates the static adapter Venta module content for an API route.
///
/// Wraps the user's route source and appends lowercase static invocation bridge functions
/// (`invoke_get`, `invoke_post`, etc., or `invoke_handle_api`) so that generated dispatch code
/// can invoke route handlers without enum variant name collision.
pub fn generate_api_adapter_code(route: &RouteMetadata, content: &str) -> String {
    let mut out = String::new();
    out.push_str(
        "// ============================================================================\n",
    );
    out.push_str(&format!(
        "// ⚡ VentaNext Framework — Static Route Adapter for {}\n",
        route.canonical_pattern
    ));
    out.push_str("// Auto-Generated Module — DO NOT EDIT MANUALLY\n");
    out.push_str(
        "// ============================================================================\n\n",
    );

    if !content.contains("import std::string::String") {
        out.push_str("import std::string::String;\n\n");
    }
    out.push_str(content.trim());
    out.push_str("\n\n");

    out.push_str(
        "// ----------------------------------------------------------------------------\n",
    );
    out.push_str(
        "// Static Invocation Bridges (lowercase to prevent uppercase enum variant collision)\n",
    );
    out.push_str(
        "// ----------------------------------------------------------------------------\n\n",
    );

    if route.has_generic_handler {
        out.push_str("pub fn invoke_handle_api() -> String {\n");
        out.push_str("    handle_api()\n");
        out.push_str("}\n\n");
    } else {
        let has_explicit_head = route.explicit_methods.contains(&"HEAD".to_string());
        for m in &route.methods {
            let lower = m.to_lowercase();
            if m == "HEAD" && !has_explicit_head {
                out.push_str(&format!("pub fn invoke_{}() -> String {{\n", lower));
                out.push_str("    GET()\n");
                out.push_str("}\n\n");
            } else if m == "HEAD" && has_explicit_head {
                out.push_str(&format!("pub fn invoke_{}() -> String {{\n", lower));
                out.push_str("    HEAD()\n");
                out.push_str("}\n\n");
            } else {
                out.push_str(&format!("pub fn invoke_{}() -> String {{\n", lower));
                out.push_str(&format!("    {}()\n", m));
                out.push_str("}\n\n");
            }
        }
    }

    out
}

// ----------------------------------------------------------------------------
// 3.3. Static Page Route Handler Validation & Adapter Generation (Phase 1.4A)
// ----------------------------------------------------------------------------

/// Encodes a canonical page route pattern into a collision-resistant, deterministic Venta module identifier.
///
/// Injective UTF-8 hexadecimal encoding ensures that every distinct canonical pattern
/// maps to a distinct identifier.
/// Identifiers always begin with the valid alphabetic prefix `page_` followed by lowercase hexadecimal characters.
pub fn sanitize_page_module_name(canonical_pattern: &str) -> String {
    let hex = canonical_pattern
        .as_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    format!("page_{}", hex)
}

/// Validates page route handlers against the Phase 1.4A signature contract.
///
/// Contract:
/// - Static pages: `pub fn render_page() -> String` (zero parameters, explicit String return type).
/// - Dynamic pages: `pub fn render_page([param]: &str, ...) -> String` with parameters corresponding
///   to the ordinary `[param]` route segments in route-segment order, each typed as `&str`, returning `String`.
/// - Fail-closed: Lexer or parser errors immediately halt with diagnostics and reject generation.
/// - Only exported functions (`pub fn`) are valid handlers. Private `fn render_page` is rejected.
/// - Unrelated exported helpers alone are rejected if `render_page` is missing.
/// - Duplicate definitions of `render_page` in the same file are rejected.
pub fn validate_page_route_handlers(
    file_path: &Path,
    content: &str,
    dynamic_params: &[String],
) -> Result<ValidatedPageHandler, RouteConflictError> {
    let source_file = SourceFile::new(
        FileId(0),
        file_path.to_string_lossy().to_string(),
        content.to_string(),
    );
    let mut diags = DiagnosticBag::new();
    let mut lexer = Lexer::new(&source_file);
    let tokens = lexer.tokenize(&mut diags);

    if diags.has_errors() {
        let err_msgs: Vec<String> = diags
            .diagnostics()
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| {
                if let Some(span) = d.primary_span {
                    let (line, col) = source_file.line_col(span.start);
                    format!("{}:{}: {}", line, col, d.message)
                } else {
                    d.message.clone()
                }
            })
            .collect();
        return Err(RouteConflictError {
            message: format!(
                "Lexer error in page route file {:?}: {}",
                file_path,
                err_msgs.join("; ")
            ),
            first_source: Some(file_path.to_path_buf()),
            second_source: None,
        });
    }

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program(&mut diags);

    if diags.has_errors() {
        let err_msgs: Vec<String> = diags
            .diagnostics()
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| {
                if let Some(span) = d.primary_span {
                    let (line, col) = source_file.line_col(span.start);
                    format!("{}:{}: {}", line, col, d.message)
                } else {
                    d.message.clone()
                }
            })
            .collect();
        return Err(RouteConflictError {
            message: format!(
                "Parse error in page route file {:?}: {}",
                file_path,
                err_msgs.join("; ")
            ),
            first_source: Some(file_path.to_path_buf()),
            second_source: None,
        });
    }

    let mut render_page_count = 0;
    let mut has_private_render_page = false;
    let mut valid_render_page = None;

    for item in &program.items {
        if let Item::Function(f) = item {
            if f.name.name == "render_page" {
                render_page_count += 1;
                if !f.is_pub {
                    has_private_render_page = true;
                } else if valid_render_page.is_none() {
                    valid_render_page = Some(f);
                }
            }
        }
    }

    if render_page_count > 1 {
        return Err(RouteConflictError {
            message: format!(
                "Duplicate 'render_page' definition found in {:?}. Multiple definitions of 'render_page' in the same page file are not permitted.",
                file_path
            ),
            first_source: Some(file_path.to_path_buf()),
            second_source: None,
        });
    }

    let f = match valid_render_page {
        Some(f) => f,
        None => {
            if has_private_render_page {
                return Err(RouteConflictError {
                    message: format!(
                        "Private handler found in {:?}: 'fn render_page' is not exported. Page handlers must be exported as 'pub fn render_page'.",
                        file_path
                    ),
                    first_source: Some(file_path.to_path_buf()),
                    second_source: None,
                });
            } else {
                return Err(RouteConflictError {
                    message: format!(
                        "No valid page handler found in {:?}. A page route file must export 'pub fn render_page'.",
                        file_path
                    ),
                    first_source: Some(file_path.to_path_buf()),
                    second_source: None,
                });
            }
        }
    };

    // Return type validation: must explicitly return String
    if f.ret_type.is_none() {
        return Err(RouteConflictError {
            message: format!(
                "Missing return type in {:?}: handler 'render_page' must explicitly return 'String'.",
                file_path
            ),
            first_source: Some(file_path.to_path_buf()),
            second_source: None,
        });
    }

    let returns_string = match &f.ret_type {
        Some(type_expr) => match &type_expr.kind {
            TypeExprKind::Named(name) => name == "String" || name == "std::string::String",
            _ => false,
        },
        None => false,
    };

    if !returns_string {
        return Err(RouteConflictError {
            message: format!(
                "Unsupported return type in {:?}: handler 'render_page' must return 'String', found unsupported return type.",
                file_path
            ),
            first_source: Some(file_path.to_path_buf()),
            second_source: None,
        });
    }

    fn is_type_ref_named(ty: &TypeExpr, expected_names: &[&str]) -> bool {
        match &ty.kind {
            TypeExprKind::Reference { is_mut, inner, .. } => {
                if *is_mut {
                    false
                } else {
                    match &inner.kind {
                        TypeExprKind::Named(name) => expected_names
                            .iter()
                            .any(|exp| name == *exp || name.ends_with(&format!("::{}", exp))),
                        _ => false,
                    }
                }
            }
            _ => false,
        }
    }

    fn is_type_ref_str(ty: &TypeExpr) -> bool {
        match &ty.kind {
            TypeExprKind::Reference { is_mut, inner, .. } => {
                if *is_mut {
                    false
                } else {
                    match &inner.kind {
                        TypeExprKind::Primitive(PrimType::Str) => true,
                        TypeExprKind::Named(s) => s == "str",
                        _ => false,
                    }
                }
            }
            _ => false,
        }
    }

    // ContextOnly: single parameter typed as &RenderContext
    if f.params.len() == 1 && is_type_ref_named(&f.params[0].ty, &["RenderContext"]) {
        return Ok(ValidatedPageHandler {
            param_names: vec![f.params[0].name.name.clone()],
            kind: PageHandlerKind::ContextOnly,
        });
    }

    // ParamsOnly: exact parameter count matching dynamic segments
    if f.params.len() == dynamic_params.len() {
        let mut param_names = Vec::new();
        for (idx, p) in f.params.iter().enumerate() {
            let expected_name = &dynamic_params[idx];
            if p.name.name != *expected_name {
                return Err(RouteConflictError {
                    message: format!(
                        "Parameter name mismatch at index {} in {:?}: expected parameter name '{}' matching route segment, but found '{}'.",
                        idx,
                        file_path,
                        expected_name,
                        p.name.name
                    ),
                    first_source: Some(file_path.to_path_buf()),
                    second_source: None,
                });
            }

            if !is_type_ref_str(&p.ty) {
                return Err(RouteConflictError {
                    message: format!(
                        "Unsupported parameter type for parameter '{}' in {:?}: dynamic route parameters must be typed as '&str'.",
                        p.name.name,
                        file_path
                    ),
                    first_source: Some(file_path.to_path_buf()),
                    second_source: None,
                });
            }

            param_names.push(p.name.name.clone());
        }

        return Ok(ValidatedPageHandler {
            param_names,
            kind: PageHandlerKind::ParamsOnly,
        });
    }

    // ParamsWithQuery: dynamic segments + trailing &QueryMap
    if f.params.len() == dynamic_params.len() + 1 {
        let query_param = &f.params[dynamic_params.len()];
        if !is_type_ref_named(&query_param.ty, &["QueryMap"]) {
            if dynamic_params.is_empty() {
                return Err(RouteConflictError {
                    message: format!(
                        "Parameter count mismatch in {:?}: route has 0 dynamic parameter(s) ([]), but 'pub fn render_page' has 1 parameter(s).",
                        file_path
                    ),
                    first_source: Some(file_path.to_path_buf()),
                    second_source: None,
                });
            } else {
                return Err(RouteConflictError {
                    message: format!(
                        "Unsupported parameter type for parameter '{}' in {:?}: trailing query parameter must be typed as '&QueryMap'.",
                        query_param.name.name,
                        file_path
                    ),
                    first_source: Some(file_path.to_path_buf()),
                    second_source: None,
                });
            }
        }

        let mut param_names = Vec::new();
        for (idx, p) in f.params[..dynamic_params.len()].iter().enumerate() {
            let expected_name = &dynamic_params[idx];
            if p.name.name != *expected_name {
                return Err(RouteConflictError {
                    message: format!(
                        "Parameter name mismatch at index {} in {:?}: expected parameter name '{}' matching route segment, but found '{}'.",
                        idx,
                        file_path,
                        expected_name,
                        p.name.name
                    ),
                    first_source: Some(file_path.to_path_buf()),
                    second_source: None,
                });
            }

            if !is_type_ref_str(&p.ty) {
                return Err(RouteConflictError {
                    message: format!(
                        "Unsupported parameter type for parameter '{}' in {:?}: dynamic route parameters must be typed as '&str'.",
                        p.name.name,
                        file_path
                    ),
                    first_source: Some(file_path.to_path_buf()),
                    second_source: None,
                });
            }

            param_names.push(p.name.name.clone());
        }

        param_names.push(query_param.name.name.clone());

        return Ok(ValidatedPageHandler {
            param_names,
            kind: PageHandlerKind::ParamsWithQuery,
        });
    }

    // Parameter count mismatch fallback
    Err(RouteConflictError {
        message: format!(
            "Parameter count mismatch in {:?}: route has {} dynamic parameter(s) ({:?}), but 'pub fn render_page' has {} parameter(s).",
            file_path,
            dynamic_params.len(),
            dynamic_params,
            f.params.len()
        ),
        first_source: Some(file_path.to_path_buf()),
        second_source: None,
    })
}

/// Generates the static adapter Venta module content for a page route.
///
/// Wraps the user's route source and appends a lowercase static invocation bridge function
/// (`invoke_render_page`) so that generated dispatch code can invoke the page handler.
pub fn generate_page_adapter_code(route: &RouteMetadata, content: &str) -> String {
    let mut out = String::new();
    out.push_str(
        "// ============================================================================\n",
    );
    out.push_str(&format!(
        "// ⚡ VentaNext Framework — Static Page Adapter for {}\n",
        route.canonical_pattern
    ));
    out.push_str("// Auto-Generated Module — DO NOT EDIT MANUALLY\n");
    out.push_str(
        "// ============================================================================\n\n",
    );

    out.push_str(content);
    if !content.ends_with('\n') {
        out.push('\n');
    }

    out.push_str("\n");
    out.push_str(
        "// ----------------------------------------------------------------------------\n",
    );
    out.push_str("// Static Invocation Bridge\n");
    out.push_str(
        "// ----------------------------------------------------------------------------\n\n",
    );

    let dynamic_params: Vec<String> = route
        .segments
        .iter()
        .filter(|s| {
            s.kind == SegmentKind::Dynamic
                || s.kind == SegmentKind::CatchAll
                || s.kind == SegmentKind::OptionalCatchAll
        })
        .filter_map(|s| s.param_name.clone())
        .collect();

    match route.page_handler_kind {
        PageHandlerKind::ContextOnly => {
            if !content.contains("import ssr::RenderContext") && !content.contains("import ssr::{")
            {
                out.push_str("import ssr::RenderContext;\n\n");
            }
            out.push_str("pub fn invoke_render_page(ctx: &RenderContext) -> String {\n");
            out.push_str("    render_page(ctx)\n");
            out.push_str("}\n");
        }
        PageHandlerKind::ParamsWithQuery => {
            if !content.contains("import http::QueryMap") && !content.contains("import http::{") {
                out.push_str("import http::QueryMap;\n\n");
            }
            if dynamic_params.is_empty() {
                out.push_str("pub fn invoke_render_page(query: &QueryMap) -> String {\n");
                out.push_str("    render_page(query)\n");
                out.push_str("}\n");
            } else {
                let params_sig = dynamic_params
                    .iter()
                    .map(|p| format!("{}: &str", p))
                    .collect::<Vec<_>>()
                    .join(", ");
                let params_call = dynamic_params.join(", ");

                out.push_str(&format!(
                    "pub fn invoke_render_page({}, query: &QueryMap) -> String {{\n",
                    params_sig
                ));
                out.push_str(&format!("    render_page({}, query)\n", params_call));
                out.push_str("}\n");
            }
        }
        PageHandlerKind::ParamsOnly => {
            if dynamic_params.is_empty() {
                out.push_str("pub fn invoke_render_page() -> String {\n");
                out.push_str("    render_page()\n");
                out.push_str("}\n");
            } else {
                let params_sig = dynamic_params
                    .iter()
                    .map(|p| format!("{}: &str", p))
                    .collect::<Vec<_>>()
                    .join(", ");
                let params_call = dynamic_params.join(", ");

                out.push_str(&format!(
                    "pub fn invoke_render_page({}) -> String {{\n",
                    params_sig
                ));
                out.push_str(&format!("    render_page({})\n", params_call));
                out.push_str("}\n");
            }
        }
    }

    out
}

// ----------------------------------------------------------------------------
// 4. Collision & Conflict Detection
// ----------------------------------------------------------------------------

pub fn routes_have_identical_shape(r1: &RouteMetadata, r2: &RouteMetadata) -> bool {
    let clean1: Vec<&Segment> = r1
        .segments
        .iter()
        .filter(|s| s.kind != SegmentKind::Group)
        .collect();
    let clean2: Vec<&Segment> = r2
        .segments
        .iter()
        .filter(|s| s.kind != SegmentKind::Group)
        .collect();

    if clean1.len() != clean2.len() {
        return false;
    }

    for (s1, s2) in clean1.iter().zip(clean2.iter()) {
        if s1.kind != s2.kind {
            return false;
        }
        if s1.kind == SegmentKind::Static && s1.raw != s2.raw {
            return false;
        }
    }

    true
}

pub fn routes_have_conflicting_methods(r1: &RouteMetadata, r2: &RouteMetadata) -> bool {
    if r1.route_kind == RouteKind::Page && r2.route_kind == RouteKind::Page {
        return true;
    }
    if r1.route_kind != r2.route_kind {
        // Page and API route cannot occupy identical route structure
        return true;
    }
    // Both are API routes: conflict if any HTTP method overlaps
    r1.methods.iter().any(|m| r2.methods.contains(m))
}

pub fn validate_conflicts(routes: &[RouteMetadata]) -> Result<(), RouteConflictError> {
    for i in 0..routes.len() {
        for j in (i + 1)..routes.len() {
            let r1 = &routes[i];
            let r2 = &routes[j];

            if routes_have_identical_shape(r1, r2) && routes_have_conflicting_methods(r1, r2) {
                let message = if r1.canonical_pattern == r2.canonical_pattern {
                    format!(
                        "Route collision detected: '{}' defined in both files",
                        r1.canonical_pattern
                    )
                } else {
                    format!(
                        "Conflicting route patterns detected between '{}' and '{}'",
                        r1.canonical_pattern, r2.canonical_pattern
                    )
                };

                return Err(RouteConflictError {
                    message,
                    first_source: Some(PathBuf::from(&r1.source_file)),
                    second_source: Some(PathBuf::from(&r2.source_file)),
                });
            }
        }
    }

    // Validate adapter filename length for API and Page routes
    for r in routes {
        if r.route_kind == RouteKind::Api
            || (r.route_kind == RouteKind::Page && !r.adapter_module_name.is_empty())
        {
            let pattern_bytes = r.canonical_pattern.as_bytes().len();
            let mod_name = if !r.adapter_module_name.is_empty() {
                r.adapter_module_name.clone()
            } else if r.route_kind == RouteKind::Api {
                sanitize_module_name(&r.canonical_pattern)
            } else {
                sanitize_page_module_name(&r.canonical_pattern)
            };
            let adapter_filename = format!("{}.va", mod_name);
            let actual_filename_len = adapter_filename.len();
            if pattern_bytes > MAX_PORTABLE_ROUTE_PATTERN_BYTES
                || actual_filename_len > MAX_PORTABLE_ADAPTER_FILENAME_LEN
            {
                let source_display = if r.source_file.is_empty() {
                    "<unknown source>".to_string()
                } else {
                    r.source_file.clone()
                };
                let kind_str = if r.route_kind == RouteKind::Api {
                    "API"
                } else {
                    "Page"
                };
                return Err(RouteConflictError {
                    message: format!(
                        "{} route pattern '{}' in \"{}\" exceeds portable filesystem filename limit: canonical pattern is {} UTF-8 bytes which encodes to adapter filename '{}' ({} bytes). Maximum permitted pattern length is {} UTF-8 bytes (max adapter filename length is {} bytes).",
                        kind_str,
                        r.canonical_pattern,
                        source_display,
                        pattern_bytes,
                        adapter_filename,
                        actual_filename_len,
                        MAX_PORTABLE_ROUTE_PATTERN_BYTES,
                        MAX_PORTABLE_ADAPTER_FILENAME_LEN,
                    ),
                    first_source: if r.source_file.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(&r.source_file))
                    },
                    second_source: None,
                });
            }
        }
    }

    // Validate uniqueness of adapter module identifiers
    let mut seen_adapters: BTreeMap<String, &RouteMetadata> = BTreeMap::new();
    for r in routes {
        if r.route_kind == RouteKind::Api
            || (r.route_kind == RouteKind::Page && !r.adapter_module_name.is_empty())
        {
            let mod_name = if !r.adapter_module_name.is_empty() {
                r.adapter_module_name.clone()
            } else if r.route_kind == RouteKind::Api {
                sanitize_module_name(&r.canonical_pattern)
            } else {
                sanitize_page_module_name(&r.canonical_pattern)
            };
            if let Some(prev) = seen_adapters.get(&mod_name) {
                let kind_str = if prev.route_kind == RouteKind::Api
                    && r.route_kind == RouteKind::Api
                {
                    "API"
                } else if prev.route_kind == RouteKind::Page && r.route_kind == RouteKind::Page {
                    "Page"
                } else {
                    "Route"
                };
                return Err(RouteConflictError {
                    message: format!(
                        "{} adapter module name collision: module '{}' is generated for both '{}' ({:?}) and '{}' ({:?})",
                        kind_str,
                        mod_name,
                        prev.canonical_pattern,
                        prev.source_file,
                        r.canonical_pattern,
                        r.source_file
                    ),
                    first_source: Some(PathBuf::from(&prev.source_file)),
                    second_source: Some(PathBuf::from(&r.source_file)),
                });
            }
            seen_adapters.insert(mod_name, r);
        }
    }

    Ok(())
}

// ----------------------------------------------------------------------------
// 5. Code Generation (Pure Venta Output)
// ----------------------------------------------------------------------------

pub fn generate_manifest_json(app: &DiscoveredApp, app_id: &str) -> String {
    let manifest = RouteManifestJson {
        schema_version: MANIFEST_SCHEMA_VERSION.to_string(),
        app_id: app_id.to_string(),
        route_count: app.routes.len(),
        routes: app.routes.clone(),
        special_files: app.special_files.clone(),
    };
    serde_json::to_string_pretty(&manifest).unwrap_or_default()
}

pub fn generate_venta_code(app: &DiscoveredApp) -> String {
    let mut out = String::new();
    out.push_str(
        "// ============================================================================\n",
    );
    out.push_str("// ⚡ VentaNext Framework — Generated Route Manifest & Static Dispatcher\n");
    out.push_str("// Stage 2 Auto-Generated Module — DO NOT EDIT MANUALLY\n");
    out.push_str(
        "// ============================================================================\n\n",
    );
    out.push_str("import router::{RouteManifest, make_route_manifest};\n");
    out.push_str(
        "import http::{Request, Response, response_html, response_json, response_not_found, response_bad_request, response_not_implemented};\n",
    );
    out.push_str("import ssr::{RenderContext};\n");
    out.push_str("import std::string::String;\n");
    out.push_str("import std::option::Option;\n\n");

    let mut page_adapter_routes: Vec<&RouteMetadata> = app
        .routes
        .iter()
        .filter(|r| r.route_kind == RouteKind::Page && !r.adapter_module_name.is_empty())
        .collect();
    page_adapter_routes.sort_by_key(|r| r.id);

    let mut api_routes: Vec<&RouteMetadata> = app
        .routes
        .iter()
        .filter(|r| r.route_kind == RouteKind::Api)
        .collect();
    api_routes.sort_by_key(|r| r.id);

    for r in &page_adapter_routes {
        out.push_str(&format!("mod {};\n", r.adapter_module_name));
    }
    for r in &api_routes {
        let mod_name = if !r.adapter_module_name.is_empty() {
            r.adapter_module_name.clone()
        } else {
            sanitize_module_name(&r.canonical_pattern)
        };
        out.push_str(&format!("mod {};\n", mod_name));
    }
    if !page_adapter_routes.is_empty() || !api_routes.is_empty() {
        out.push_str("\n");
    }

    out.push_str(
        "// ----------------------------------------------------------------------------\n",
    );
    out.push_str("// 1. Build Generated Manifest\n");
    out.push_str(
        "// ----------------------------------------------------------------------------\n\n",
    );
    out.push_str("pub fn build_generated_manifest() -> RouteManifest {\n");
    out.push_str("    let mut m = make_route_manifest();\n");

    for r in &app.routes {
        if r.route_kind == RouteKind::Page {
            let mode_code = match r.render_mode {
                RenderMode::Static => 1,
                RenderMode::Dynamic => 2,
                RenderMode::ISR => 3,
            };
            out.push_str(&format!(
                "    m.add_page_route_with_mode(\"{}\", {}, {}, {});\n",
                r.canonical_pattern,
                r.layout_chain.len(),
                r.id,
                mode_code
            ));
        } else if r.route_kind == RouteKind::Api {
            for m in &r.methods {
                out.push_str(&format!(
                    "    m.add_api_route(\"{}\", \"{}\", {});\n",
                    m, r.canonical_pattern, r.id
                ));
            }
        }
    }

    out.push_str("    m\n");
    out.push_str("}\n\n");

    out.push_str(
        "// ----------------------------------------------------------------------------\n",
    );
    out.push_str("// 2. Static Route Dispatcher\n");
    out.push_str(
        "// ----------------------------------------------------------------------------\n\n",
    );
    out.push_str("pub fn dispatch_route(route_id: i64, ctx: &RenderContext) -> Response {\n");
    for r in &app.routes {
        if r.route_kind == RouteKind::Page {
            out.push_str(&format!("    if route_id == {} {{\n", r.id));

            let dynamic_params: Vec<String> = r
                .segments
                .iter()
                .filter(|s| {
                    s.kind == SegmentKind::Dynamic
                        || s.kind == SegmentKind::CatchAll
                        || s.kind == SegmentKind::OptionalCatchAll
                })
                .filter_map(|s| s.param_name.clone())
                .collect();

            if r.adapter_module_name.is_empty() {
                out.push_str("        return response_not_implemented(\"<h1>501 — Page handler not supported</h1>\");\n");
            } else {
                match r.page_handler_kind {
                    PageHandlerKind::ContextOnly => {
                        out.push_str(&format!(
                            "        let content = {}::invoke_render_page(ctx);\n        return response_html(content.as_str());\n",
                            r.adapter_module_name
                        ));
                    }
                    PageHandlerKind::ParamsWithQuery => {
                        if dynamic_params.is_empty() {
                            out.push_str(&format!(
                                "        let content = {}::invoke_render_page(&ctx.query);\n        return response_html(content.as_str());\n",
                                r.adapter_module_name
                            ));
                        } else {
                            let mut indent = "        ".to_string();
                            let mut closing = Vec::new();
                            for p in &dynamic_params {
                                out.push_str(&format!(
                                    "{}let p_opt_{} = ctx.params.get(\"{}\");\n",
                                    indent, p, p
                                ));
                                out.push_str(&format!("{}match p_opt_{} {{\n", indent, p));
                                out.push_str(&format!(
                                    "{}    Option::Some(val_{}) => {{\n",
                                    indent, p
                                ));
                                closing.push(format!(
                                    "{indent}    }}\n{indent}    Option::None => {{\n{indent}        return response_bad_request(\"<h1>400 — Missing required route parameter</h1>\");\n{indent}    }}\n{indent}}}\n",
                                    indent = indent
                                ));
                                indent.push_str("        ");
                            }

                            let mut args = dynamic_params
                                .iter()
                                .map(|p| format!("val_{}.as_str()", p))
                                .collect::<Vec<_>>()
                                .join(", ");
                            args.push_str(", &ctx.query");

                            out.push_str(&format!(
                                "{}let content = {}::invoke_render_page({});\n",
                                indent, r.adapter_module_name, args
                            ));
                            out.push_str(&format!(
                                "{}return response_html(content.as_str());\n",
                                indent
                            ));

                            for close in closing.into_iter().rev() {
                                out.push_str(&close);
                            }
                        }
                    }
                    PageHandlerKind::ParamsOnly => {
                        if dynamic_params.is_empty() {
                            out.push_str(&format!(
                                "        let content = {}::invoke_render_page();\n        return response_html(content.as_str());\n",
                                r.adapter_module_name
                            ));
                        } else {
                            let mut indent = "        ".to_string();
                            let mut closing = Vec::new();
                            for p in &dynamic_params {
                                out.push_str(&format!(
                                    "{}let p_opt_{} = ctx.params.get(\"{}\");\n",
                                    indent, p, p
                                ));
                                out.push_str(&format!("{}match p_opt_{} {{\n", indent, p));
                                out.push_str(&format!(
                                    "{}    Option::Some(val_{}) => {{\n",
                                    indent, p
                                ));
                                closing.push(format!(
                                    "{indent}    }}\n{indent}    Option::None => {{\n{indent}        return response_bad_request(\"<h1>400 — Missing required route parameter</h1>\");\n{indent}    }}\n{indent}}}\n",
                                    indent = indent
                                ));
                                indent.push_str("        ");
                            }

                            let args = dynamic_params
                                .iter()
                                .map(|p| format!("val_{}.as_str()", p))
                                .collect::<Vec<_>>()
                                .join(", ");
                            out.push_str(&format!(
                                "{}let content = {}::invoke_render_page({});\n",
                                indent, r.adapter_module_name, args
                            ));
                            out.push_str(&format!(
                                "{}return response_html(content.as_str());\n",
                                indent
                            ));

                            for close in closing.into_iter().rev() {
                                out.push_str(&close);
                            }
                        }
                    }
                }
            }
            out.push_str("    }\n");
        }
    }
    out.push_str("    response_not_found(\"<h1>404 — Page Not Found</h1>\")\n");
    out.push_str("}\n\n");

    out.push_str("pub fn dispatch_api(handler_id: i64, req: &mut Request) -> Response {\n");
    for r in &api_routes {
        let mod_name = if !r.adapter_module_name.is_empty() {
            r.adapter_module_name.clone()
        } else {
            sanitize_module_name(&r.canonical_pattern)
        };

        out.push_str(&format!("    if handler_id == {} {{\n", r.id));
        if r.has_generic_handler {
            out.push_str(&format!(
                "        let content = {}::invoke_handle_api();\n",
                mod_name
            ));
            out.push_str("        return response_json(content.as_str());\n");
        } else {
            if r.methods.contains(&"HEAD".to_string()) {
                out.push_str("        if req.method.equals(\"HEAD\") {\n");
                out.push_str(&format!(
                    "            let content = {}::invoke_head();\n",
                    mod_name
                ));
                out.push_str("            return response_json(content.as_str());\n");
                out.push_str("        }\n");
            }
            for m in &r.methods {
                if m == "HEAD" {
                    continue;
                }
                let lower = m.to_lowercase();
                out.push_str(&format!("        if req.method.equals(\"{}\") {{\n", m));
                out.push_str(&format!(
                    "            let content = {}::invoke_{}();\n",
                    mod_name, lower
                ));
                out.push_str("            return response_json(content.as_str());\n");
                out.push_str("        }\n");
            }
        }
        out.push_str("    }\n");
    }
    out.push_str("    response_not_found(\"{\\\"error\\\":\\\"API endpoint not found\\\"}\")\n");
    out.push_str("}\n");

    out
}

/// Writes both the main generated routes module and all child API and Page route adapter modules.
///
/// Directory layout:
/// - `{out_va_path}` (e.g. `ventanext/generated_routes.va`)
/// - `{out_va_path.stem()}/` (e.g. `ventanext/generated_routes/`)
///   - `{adapter_mod_1}.va` (e.g. `route_2f6170692f6865616c7468.va`)
///   - `{adapter_mod_2}.va` (e.g. `page_2f61626f7574.va`)
pub fn write_generated_routes(
    discovered: &DiscoveredApp,
    out_va_path: &Path,
) -> std::io::Result<()> {
    // 1. Enforce filename length limits before ANY filesystem operations
    for r in &discovered.routes {
        if r.route_kind == RouteKind::Api
            || (r.route_kind == RouteKind::Page && !r.adapter_module_name.is_empty())
        {
            let pattern_bytes = r.canonical_pattern.as_bytes().len();
            let mod_name = if !r.adapter_module_name.is_empty() {
                r.adapter_module_name.clone()
            } else if r.route_kind == RouteKind::Api {
                sanitize_module_name(&r.canonical_pattern)
            } else {
                sanitize_page_module_name(&r.canonical_pattern)
            };
            let adapter_filename = format!("{}.va", mod_name);
            let actual_filename_len = adapter_filename.len();
            if pattern_bytes > MAX_PORTABLE_ROUTE_PATTERN_BYTES
                || actual_filename_len > MAX_PORTABLE_ADAPTER_FILENAME_LEN
            {
                let source_display = if r.source_file.is_empty() {
                    "<unknown source>".to_string()
                } else {
                    r.source_file.clone()
                };
                let kind_str = if r.route_kind == RouteKind::Api {
                    "API"
                } else {
                    "Page"
                };
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "{} route pattern '{}' in \"{}\" exceeds portable filesystem filename limit: canonical pattern is {} UTF-8 bytes which encodes to adapter filename '{}' ({} bytes). Maximum permitted pattern length is {} UTF-8 bytes (max adapter filename length is {} bytes).",
                        kind_str,
                        r.canonical_pattern,
                        source_display,
                        pattern_bytes,
                        adapter_filename,
                        actual_filename_len,
                        MAX_PORTABLE_ROUTE_PATTERN_BYTES,
                        MAX_PORTABLE_ADAPTER_FILENAME_LEN,
                    ),
                ));
            }
        }
    }

    let parent_dir = out_va_path.parent().unwrap_or(Path::new("."));
    let stem = out_va_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("generated_routes");
    let adapter_dir = parent_dir.join(stem);

    fs::create_dir_all(&adapter_dir)?;

    // Enforce uniqueness of adapter module identifiers and filenames before writing
    let mut seen_mods: BTreeMap<String, &RouteMetadata> = BTreeMap::new();
    let mut seen_files: BTreeMap<String, &RouteMetadata> = BTreeMap::new();

    for r in &discovered.routes {
        if r.route_kind == RouteKind::Api
            || (r.route_kind == RouteKind::Page && !r.adapter_module_name.is_empty())
        {
            let mod_name = if !r.adapter_module_name.is_empty() {
                r.adapter_module_name.clone()
            } else if r.route_kind == RouteKind::Api {
                sanitize_module_name(&r.canonical_pattern)
            } else {
                sanitize_page_module_name(&r.canonical_pattern)
            };

            if let Some(prev) = seen_mods.get(&mod_name) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "Duplicate adapter module '{}' for routes '{}' ({}) and '{}' ({})",
                        mod_name,
                        prev.canonical_pattern,
                        prev.source_file,
                        r.canonical_pattern,
                        r.source_file
                    ),
                ));
            }
            seen_mods.insert(mod_name.clone(), r);

            let adapter_filename = format!("{}.va", mod_name);
            if let Some(prev) = seen_files.get(&adapter_filename) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "Duplicate adapter filename '{}' for routes '{}' ({}) and '{}' ({})",
                        adapter_filename,
                        prev.canonical_pattern,
                        prev.source_file,
                        r.canonical_pattern,
                        r.source_file
                    ),
                ));
            }
            seen_files.insert(adapter_filename, r);
        }
    }

    let mut expected_files = BTreeSet::new();

    for r in &discovered.routes {
        if r.route_kind == RouteKind::Api {
            let mod_name = if !r.adapter_module_name.is_empty() {
                r.adapter_module_name.clone()
            } else {
                sanitize_module_name(&r.canonical_pattern)
            };
            let adapter_filename = format!("{}.va", mod_name);
            let adapter_path = adapter_dir.join(&adapter_filename);
            expected_files.insert(adapter_filename);

            let content = if !r.source_content.is_empty() {
                r.source_content.clone()
            } else {
                fs::read_to_string(&r.source_file).unwrap_or_default()
            };

            let adapter_code = generate_api_adapter_code(r, &content);
            fs::write(&adapter_path, adapter_code)?;
        } else if r.route_kind == RouteKind::Page && !r.adapter_module_name.is_empty() {
            let mod_name = r.adapter_module_name.clone();
            let adapter_filename = format!("{}.va", mod_name);
            let adapter_path = adapter_dir.join(&adapter_filename);
            expected_files.insert(adapter_filename);

            let content = if !r.source_content.is_empty() {
                r.source_content.clone()
            } else {
                fs::read_to_string(&r.source_file).unwrap_or_default()
            };

            let adapter_code = generate_page_adapter_code(r, &content);
            fs::write(&adapter_path, adapter_code)?;
        }
    }

    if let Ok(entries) = fs::read_dir(&adapter_dir) {
        for entry in entries.flatten() {
            if let Ok(file_name) = entry.file_name().into_string() {
                if file_name.ends_with(".va") && !expected_files.contains(&file_name) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    let main_code = generate_venta_code(discovered);
    fs::write(out_va_path, main_code)?;

    Ok(())
}

// ----------------------------------------------------------------------------
// 6. Static Site Generation (SSG) & Static Export Pipeline
// ----------------------------------------------------------------------------

pub fn compute_content_digest(content: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in content.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

pub fn export_static_site(
    app_dir: &Path,
    out_dir: &Path,
    discovered: &DiscoveredApp,
) -> Result<ExportManifest, Box<dyn std::error::Error>> {
    fs::create_dir_all(out_dir)?;

    let mut exported_files = Vec::new();

    // 1. Copy public/ directory if it exists
    let public_dir = if let Some(parent) = app_dir.parent() {
        parent.join("public")
    } else {
        PathBuf::from("public")
    };
    if public_dir.is_dir() {
        copy_dir_recursive(&public_dir, out_dir)?;
    }

    // 2. Export each Static Page route to out_dir
    for route in &discovered.routes {
        if route.route_kind == RouteKind::Page && route.render_mode == RenderMode::Static {
            let target_file = if route.canonical_pattern == "/" {
                out_dir.join("index.html")
            } else {
                let rel = route.canonical_pattern.trim_start_matches('/');
                let rel_path = Path::new(rel);
                if rel_path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err(format!(
                        "Security error: static export path {:?} escapes output directory via parent directory traversal",
                        rel
                    )
                    .into());
                }
                let page_dir = out_dir.join(rel);
                fs::create_dir_all(&page_dir)?;
                page_dir.join("index.html")
            };

            let title = if route.canonical_pattern == "/" {
                "Home".to_string()
            } else {
                let seg = route.canonical_pattern.trim_start_matches('/');
                format!("{}{}", seg[..1].to_uppercase(), &seg[1..])
            };

            let html_content = format!(
                r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{} | ⚡ Venta</title>
    <link rel="icon" type="image/svg+xml" href="/vanta-file-icon.svg">
</head>
<body>
    <header>
        <nav><a href="/">⚡ VentaNext Static Export</a></nav>
    </header>
    <main>
        <h1>{}</h1>
        <p>Prerendered via VentaNext SSG (Static Site Generation).</p>
        <p>Canonical Route: <code>{}</code></p>
    </main>
    <footer>
        <p>Powered by ⚡ VentaNext SSG • Native Static Export</p>
    </footer>
</body>
</html>
"#,
                title, title, route.canonical_pattern
            );

            fs::write(&target_file, &html_content)?;
            let meta = fs::metadata(&target_file)?;
            let digest = compute_content_digest(&html_content);
            let rel_target = target_file.strip_prefix(out_dir).unwrap_or(&target_file);

            exported_files.push(ExportedFile {
                url_path: route.canonical_pattern.clone(),
                file_path: rel_target.to_string_lossy().to_string(),
                size_bytes: meta.len(),
                content_digest: digest,
            });
        }
    }

    // 3. Export 404.html
    let not_found_file = out_dir.join("404.html");
    let not_found_html = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>404 Not Found | ⚡ Venta</title>
</head>
<body>
    <h1>404 — Page Not Found</h1>
    <p>This static route does not exist.</p>
    <a href="/">Return Home</a>
</body>
</html>
"#;
    fs::write(&not_found_file, not_found_html)?;
    let nf_meta = fs::metadata(&not_found_file)?;
    exported_files.push(ExportedFile {
        url_path: "/404".to_string(),
        file_path: "404.html".to_string(),
        size_bytes: nf_meta.len(),
        content_digest: compute_content_digest(not_found_html),
    });

    // 4. Export official Venta file icon SVGs
    for icon_name in &["vanta-file-icon.svg", "venta-file-icon.svg"] {
        let icon_file = out_dir.join(icon_name);
        fs::write(&icon_file, VENTA_FILE_ICON_SVG)?;
        let icon_meta = fs::metadata(&icon_file)?;
        exported_files.push(ExportedFile {
            url_path: format!("/{}", icon_name),
            file_path: icon_name.to_string(),
            size_bytes: icon_meta.len(),
            content_digest: compute_content_digest(VENTA_FILE_ICON_SVG),
        });
    }

    let manifest = ExportManifest {
        schema_version: MANIFEST_SCHEMA_VERSION.to_string(),
        exported_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        route_count: exported_files.len(),
        files: exported_files,
    };

    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    fs::write(out_dir.join("export_manifest.json"), manifest_json)?;

    Ok(manifest)
}
