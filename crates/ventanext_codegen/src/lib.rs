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

pub const MANIFEST_SCHEMA_VERSION: &str = "1.0.0";

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

    let methods = if kind == RouteKind::Page {
        vec!["GET".to_string(), "HEAD".to_string()]
    } else {
        let content = fs::read_to_string(full_path).unwrap_or_default();
        let candidate_methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
        let mut found = Vec::new();
        for m in &candidate_methods {
            let pat1 = format!("pub fn {}()", m);
            let pat2 = format!("pub fn {}(", m);
            let pat3 = format!("fn {}()", m);
            let pat4 = format!("fn {}(", m);
            if content.contains(&pat1)
                || content.contains(&pat2)
                || content.contains(&pat3)
                || content.contains(&pat4)
            {
                found.push(m.to_string());
            }
        }
        if !found.is_empty() {
            if found.contains(&"GET".to_string()) && !found.contains(&"HEAD".to_string()) {
                found.push("HEAD".to_string());
            }
            found
        } else {
            vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "PATCH".to_string(),
                "HEAD".to_string(),
                "OPTIONS".to_string(),
            ]
        }
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
    })
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
        "import http::{Request, Response, response_html, response_json, response_not_found};\n",
    );
    out.push_str("import ssr::{RenderContext};\n");
    out.push_str("import std::string::String;\n");
    out.push_str("import std::option::Option;\n\n");

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
            let mut params = Vec::new();
            for seg in &r.segments {
                if let Some(ref p) = seg.param_name {
                    if seg.kind != SegmentKind::Group {
                        params.push(p);
                    }
                }
            }
            if params.is_empty() {
                out.push_str(&format!(
                    "        return response_html(\"<h1>Page {}</h1><p>Pattern: {}</p>\");\n",
                    r.id, r.canonical_pattern
                ));
            } else {
                out.push_str(&format!(
                    "        let mut body = String::from_str(\"<h1>Route: {}</h1>\");\n",
                    r.canonical_pattern
                ));
                for p in &params {
                    out.push_str(&format!(
                        "        let p_opt_{} = ctx.params.get(\"{}\");\n        body.push_str(\"<p>{} = \");\n        match p_opt_{} {{\n            Option::Some(val) => {{ body.push_str(val.as_str()); }}\n            Option::None => {{}}\n        }}\n        body.push_str(\"</p>\");\n",
                        p, p, p, p
                    ));
                }
                out.push_str("        return response_html(body.as_str());\n");
            }
            out.push_str("    }\n");
        }
    }
    out.push_str("    response_not_found(\"<h1>404 — Page Not Found</h1>\")\n");
    out.push_str("}\n\n");

    out.push_str("pub fn dispatch_api(handler_id: i64, req: &mut Request) -> Response {\n");
    for r in &app.routes {
        if r.route_kind == RouteKind::Api {
            out.push_str(&format!("    if handler_id == {} {{\n", r.id));
            out.push_str(&format!(
                "        let mut json = String::from_str(\"{{\\\"status\\\":\\\"ok\\\",\\\"endpoint\\\":\\\"{}\\\",\\\"method\\\":\\\"\");\n",
                r.canonical_pattern
            ));
            out.push_str("        json.push_str(req.method.as_str());\n");
            out.push_str("        json.push_str(\"\\\"}\");\n");
            out.push_str("        return response_json(json.as_str());\n");
            out.push_str("    }\n");
        }
    }
    out.push_str("    response_not_found(\"{\\\"error\\\":\\\"API endpoint not found\\\"}\")\n");
    out.push_str("}\n");

    out
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
