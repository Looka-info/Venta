// ============================================================================
// ⚡ VentaNext Framework — Automated Route Discovery & Generator Tests
// Tests covering all 13 discovery, conflict, and manifest generation scenarios
// ============================================================================

use std::fs;
use std::path::Path;
use ventanext_codegen::{
    export_static_site, generate_manifest_json, generate_venta_code, parse_segment,
    scan_app_directory, RenderMode, RouteKind, SegmentKind,
};

fn create_temp_app_tree(base: &Path) {
    fs::create_dir_all(base.join("about")).unwrap();
    fs::create_dir_all(base.join("dashboard")).unwrap();
    fs::create_dir_all(base.join("blog/[slug]")).unwrap();
    fs::create_dir_all(base.join("docs/[...path]")).unwrap();
    fs::create_dir_all(base.join("shop/[[...filter]]")).unwrap();
    fs::create_dir_all(base.join("(marketing)/pricing")).unwrap();
    fs::create_dir_all(base.join("api/health")).unwrap();
    fs::create_dir_all(base.join("api/users")).unwrap();

    fs::write(base.join("layout.va"), "pub fn render() {}").unwrap();
    fs::write(base.join("page.va"), "pub fn render() {}").unwrap();
    fs::write(base.join("loading.va"), "pub fn render() {}").unwrap();
    fs::write(base.join("error.va"), "pub fn render() {}").unwrap();
    fs::write(base.join("not-found.va"), "pub fn render() {}").unwrap();

    fs::write(base.join("about/page.va"), "pub fn render() {}").unwrap();
    fs::write(base.join("dashboard/layout.va"), "pub fn render() {}").unwrap();
    fs::write(base.join("dashboard/page.va"), "pub fn render() {}").unwrap();
    fs::write(base.join("blog/[slug]/page.va"), "pub fn render() {}").unwrap();
    fs::write(base.join("docs/[...path]/page.va"), "pub fn render() {}").unwrap();
    fs::write(
        base.join("shop/[[...filter]]/page.va"),
        "pub fn render() {}",
    )
    .unwrap();
    fs::write(
        base.join("(marketing)/pricing/page.va"),
        "pub fn render() {}",
    )
    .unwrap();

    fs::write(base.join("api/health/route.va"), "pub fn handle() {}").unwrap();
    fs::write(base.join("api/users/route.va"), "pub fn handle() {}").unwrap();
}

#[test]
fn test_01_root_page_discovery() {
    let tmp = tempfile::tempdir().unwrap();
    create_temp_app_tree(tmp.path());

    let app = scan_app_directory(tmp.path()).expect("Scan must succeed");
    let root = app.routes.iter().find(|r| r.canonical_pattern == "/");
    assert!(root.is_some(), "Root page must be discovered");
    let root = root.unwrap();
    assert_eq!(root.route_kind, RouteKind::Page);
    assert_eq!(
        root.layout_chain.len(),
        1,
        "Root page must have root layout in chain"
    );
}

#[test]
fn test_02_nested_page_discovery() {
    let tmp = tempfile::tempdir().unwrap();
    create_temp_app_tree(tmp.path());

    let app = scan_app_directory(tmp.path()).expect("Scan must succeed");
    let about = app.routes.iter().find(|r| r.canonical_pattern == "/about");
    assert!(about.is_some(), "Nested /about page must be discovered");
}

#[test]
fn test_03_static_route_discovery() {
    let tmp = tempfile::tempdir().unwrap();
    create_temp_app_tree(tmp.path());

    let app = scan_app_directory(tmp.path()).expect("Scan must succeed");
    let dash = app
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/dashboard");
    assert!(dash.is_some());
    let dash = dash.unwrap();
    assert_eq!(
        dash.layout_chain.len(),
        2,
        "Dashboard must have root and dashboard layout in chain"
    );
}

#[test]
fn test_04_dynamic_route_discovery() {
    let tmp = tempfile::tempdir().unwrap();
    create_temp_app_tree(tmp.path());

    let app = scan_app_directory(tmp.path()).expect("Scan must succeed");
    let blog = app
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/blog/[slug]");
    assert!(blog.is_some());
    let blog = blog.unwrap();
    assert_eq!(blog.segments.len(), 2);
    assert_eq!(blog.segments[1].kind, SegmentKind::Dynamic);
    assert_eq!(blog.segments[1].param_name.as_deref(), Some("slug"));
}

#[test]
fn test_05_catch_all_discovery() {
    let tmp = tempfile::tempdir().unwrap();
    create_temp_app_tree(tmp.path());

    let app = scan_app_directory(tmp.path()).expect("Scan must succeed");
    let docs = app
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/docs/[...path]");
    assert!(docs.is_some());
    let docs = docs.unwrap();
    assert_eq!(docs.segments.last().unwrap().kind, SegmentKind::CatchAll);
    assert_eq!(
        docs.segments.last().unwrap().param_name.as_deref(),
        Some("path")
    );
}

#[test]
fn test_06_optional_catch_all_discovery() {
    let tmp = tempfile::tempdir().unwrap();
    create_temp_app_tree(tmp.path());

    let app = scan_app_directory(tmp.path()).expect("Scan must succeed");
    let shop = app
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/shop/[[...filter]]");
    assert!(shop.is_some());
    let shop = shop.unwrap();
    assert_eq!(
        shop.segments.last().unwrap().kind,
        SegmentKind::OptionalCatchAll
    );
    assert_eq!(
        shop.segments.last().unwrap().param_name.as_deref(),
        Some("filter")
    );
}

#[test]
fn test_07_route_group_normalization() {
    let tmp = tempfile::tempdir().unwrap();
    create_temp_app_tree(tmp.path());

    let app = scan_app_directory(tmp.path()).expect("Scan must succeed");
    // (marketing)/pricing must be normalized to /pricing in the URL
    let pricing = app
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/pricing");
    assert!(
        pricing.is_some(),
        "(marketing) route group must be stripped from public URL pattern"
    );
    let pricing = pricing.unwrap();
    assert!(
        pricing.source_file.contains("(marketing)"),
        "Source path must retain route group"
    );
}

#[test]
fn test_08_api_route_discovery() {
    let tmp = tempfile::tempdir().unwrap();
    create_temp_app_tree(tmp.path());

    let app = scan_app_directory(tmp.path()).expect("Scan must succeed");
    let health = app
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/api/health");
    assert!(health.is_some());
    assert_eq!(health.unwrap().route_kind, RouteKind::Api);
}

#[test]
fn test_09_special_files_recognition() {
    let tmp = tempfile::tempdir().unwrap();
    create_temp_app_tree(tmp.path());

    let app = scan_app_directory(tmp.path()).expect("Scan must succeed");
    assert!(app.special_files.root_layout.is_some());
    assert!(app.special_files.loading.is_some());
    assert!(app.special_files.error.is_some());
    assert!(app.special_files.not_found.is_some());
}

#[test]
fn test_10_duplicate_route_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("pricing")).unwrap();
    fs::create_dir_all(tmp.path().join("(marketing)/pricing")).unwrap();
    fs::write(tmp.path().join("pricing/page.va"), "").unwrap();
    fs::write(tmp.path().join("(marketing)/pricing/page.va"), "").unwrap();

    let res = scan_app_directory(tmp.path());
    assert!(res.is_err(), "Must reject duplicate route collision");
    let err = res.unwrap_err();
    assert!(err.message.contains("Route collision detected"));
}

#[test]
fn test_11_invalid_segment_syntax() {
    assert!(parse_segment("").is_err());
    assert!(parse_segment("[]").is_err());
    assert!(parse_segment("[123bad]").is_err());
    assert!(parse_segment("[...123]").is_err());
    assert!(parse_segment("[[...123]]").is_err());
    assert!(parse_segment("(bad name)").is_err());
    assert!(parse_segment("unclosed[").is_err());
}

#[test]
fn test_12_route_group_collision() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("(team_a)/billing")).unwrap();
    fs::create_dir_all(tmp.path().join("(team_b)/billing")).unwrap();
    fs::write(tmp.path().join("(team_a)/billing/page.va"), "").unwrap();
    fs::write(tmp.path().join("(team_b)/billing/page.va"), "").unwrap();

    let res = scan_app_directory(tmp.path());
    assert!(
        res.is_err(),
        "Must reject two route groups mapping to identical canonical URL"
    );
}

#[test]
fn test_13_deterministic_manifest_output() {
    let tmp = tempfile::tempdir().unwrap();
    create_temp_app_tree(tmp.path());

    let app1 = scan_app_directory(tmp.path()).unwrap();
    let app2 = scan_app_directory(tmp.path()).unwrap();

    let json1 = generate_manifest_json(&app1, "test-app");
    let json2 = generate_manifest_json(&app2, "test-app");
    assert_eq!(
        json1, json2,
        "Manifest JSON must be 100% deterministic and byte-identical"
    );

    let code1 = generate_venta_code(&app1);
    let code2 = generate_venta_code(&app2);
    assert_eq!(
        code1, code2,
        "Generated Venta source must be 100% deterministic and byte-identical"
    );
}

#[test]
fn test_14_render_mode_classification() {
    let tmp = tempfile::tempdir().unwrap();
    create_temp_app_tree(tmp.path());

    let app = scan_app_directory(tmp.path()).unwrap();

    // Static pages without dynamic parameters -> RenderMode::Static
    let home = app
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/")
        .unwrap();
    assert_eq!(home.render_mode, RenderMode::Static);

    let about = app
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/about")
        .unwrap();
    assert_eq!(about.render_mode, RenderMode::Static);

    let pricing = app
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/pricing")
        .unwrap();
    assert_eq!(pricing.render_mode, RenderMode::Static);

    // Dynamic pages -> RenderMode::Dynamic
    let blog = app
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/blog/[slug]")
        .unwrap();
    assert_eq!(blog.render_mode, RenderMode::Dynamic);

    let docs = app
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/docs/[...path]")
        .unwrap();
    assert_eq!(docs.render_mode, RenderMode::Dynamic);

    // API routes -> RenderMode::Dynamic
    let api = app
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/api/health")
        .unwrap();
    assert_eq!(api.render_mode, RenderMode::Dynamic);
}

#[test]
fn test_15_static_export_pipeline() {
    let tmp_app = tempfile::tempdir().unwrap();
    create_temp_app_tree(tmp_app.path());
    let tmp_out = tempfile::tempdir().unwrap();

    let app = scan_app_directory(tmp_app.path()).unwrap();
    let export_res = export_static_site(tmp_app.path(), tmp_out.path(), &app).unwrap();

    assert!(
        export_res.files.len() >= 4,
        "Must export static HTML files and 404"
    );
    assert!(
        tmp_out.path().join("index.html").exists(),
        "Must export index.html"
    );
    assert!(
        tmp_out.path().join("about/index.html").exists(),
        "Must export about/index.html"
    );
    assert!(
        tmp_out.path().join("pricing/index.html").exists(),
        "Must export pricing/index.html"
    );
    assert!(
        tmp_out.path().join("404.html").exists(),
        "Must export 404.html"
    );
    assert!(
        tmp_out.path().join("export_manifest.json").exists(),
        "Must export export_manifest.json"
    );
}

#[test]
fn test_16_phase1_1_earlier_static_vs_later_static_sorting() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("foo/[...rest]")).unwrap();
    fs::create_dir_all(tmp.path().join("[id]/bar")).unwrap();
    fs::write(tmp.path().join("foo/[...rest]/page.va"), "").unwrap();
    fs::write(tmp.path().join("[id]/bar/page.va"), "").unwrap();

    let app = scan_app_directory(tmp.path()).expect("Scan must succeed");
    assert_eq!(app.routes.len(), 2);

    let idx_foo = app
        .routes
        .iter()
        .position(|r| r.canonical_pattern == "/foo/[...rest]")
        .unwrap();
    let idx_bar = app
        .routes
        .iter()
        .position(|r| r.canonical_pattern == "/[id]/bar")
        .unwrap();

    assert!(
        idx_foo < idx_bar,
        "Segment-level precedence: /foo/[...rest] (rank 4 at segment 0) must sort before /[id]/bar (rank 3 at segment 0)"
    );
}

#[test]
fn test_17_phase1_1_precedence_ordering() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("users/new")).unwrap();
    fs::create_dir_all(tmp.path().join("users/[id]")).unwrap();
    fs::create_dir_all(tmp.path().join("posts/[id]")).unwrap();
    fs::create_dir_all(tmp.path().join("posts/[...slug]")).unwrap();
    fs::create_dir_all(tmp.path().join("shop")).unwrap();
    fs::create_dir_all(tmp.path().join("shop/[[...filter]]")).unwrap();

    fs::write(tmp.path().join("users/new/page.va"), "").unwrap();
    fs::write(tmp.path().join("users/[id]/page.va"), "").unwrap();
    fs::write(tmp.path().join("posts/[id]/page.va"), "").unwrap();
    fs::write(tmp.path().join("posts/[...slug]/page.va"), "").unwrap();
    fs::write(tmp.path().join("shop/page.va"), "").unwrap();
    fs::write(tmp.path().join("shop/[[...filter]]/page.va"), "").unwrap();

    let app = scan_app_directory(tmp.path()).expect("Scan must succeed");

    // 1. Static vs Dynamic (/users/new vs /users/[id])
    let idx_users_new = app
        .routes
        .iter()
        .position(|r| r.canonical_pattern == "/users/new")
        .unwrap();
    let idx_users_id = app
        .routes
        .iter()
        .position(|r| r.canonical_pattern == "/users/[id]")
        .unwrap();
    assert!(
        idx_users_new < idx_users_id,
        "Static /users/new must sort before dynamic /users/[id]"
    );

    // 2. Dynamic vs Catch-All (/posts/[id] vs /posts/[...slug])
    let idx_posts_id = app
        .routes
        .iter()
        .position(|r| r.canonical_pattern == "/posts/[id]")
        .unwrap();
    let idx_posts_slug = app
        .routes
        .iter()
        .position(|r| r.canonical_pattern == "/posts/[...slug]")
        .unwrap();
    assert!(
        idx_posts_id < idx_posts_slug,
        "Dynamic /posts/[id] must sort before catch-all /posts/[...slug]"
    );

    // 3. Exact route vs Optional Catch-All (/shop vs /shop/[[...filter]])
    let idx_shop = app
        .routes
        .iter()
        .position(|r| r.canonical_pattern == "/shop")
        .unwrap();
    let idx_shop_filter = app
        .routes
        .iter()
        .position(|r| r.canonical_pattern == "/shop/[[...filter]]")
        .unwrap();
    assert!(
        idx_shop < idx_shop_filter,
        "Exact route /shop must sort before optional catch-all /shop/[[...filter]]"
    );
}

#[test]
fn test_18_phase1_1_conflict_detection() {
    // Ambiguous dynamic parameters at same segment position -> conflict
    let tmp_conf = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp_conf.path().join("blog/[slug]")).unwrap();
    fs::create_dir_all(tmp_conf.path().join("blog/[id]")).unwrap();
    fs::write(tmp_conf.path().join("blog/[slug]/page.va"), "").unwrap();
    fs::write(tmp_conf.path().join("blog/[id]/page.va"), "").unwrap();
    let res_conf = scan_app_directory(tmp_conf.path());
    assert!(
        res_conf.is_err(),
        "/blog/[slug] and /blog/[id] must be detected as conflict"
    );

    // Static route alongside dynamic parameter -> permitted under precedence rules
    let tmp_ok = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp_ok.path().join("blog/new")).unwrap();
    fs::create_dir_all(tmp_ok.path().join("blog/[id]")).unwrap();
    fs::write(tmp_ok.path().join("blog/new/page.va"), "").unwrap();
    fs::write(tmp_ok.path().join("blog/[id]/page.va"), "").unwrap();
    let res_ok = scan_app_directory(tmp_ok.path());
    assert!(
        res_ok.is_ok(),
        "/blog/new and /blog/[id] must be permitted without conflict"
    );
}

// ----------------------------------------------------------------------------
// Phase 1.1 Follow-up: Route Conflict Consistency Regression Tests (1-9)
// ----------------------------------------------------------------------------

#[test]
fn test_19_conflict_duplicate_dynamic_different_names() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("blog/[id]")).unwrap();
    fs::create_dir_all(tmp.path().join("blog/[slug]")).unwrap();
    fs::write(tmp.path().join("blog/[id]/page.va"), "").unwrap();
    fs::write(tmp.path().join("blog/[slug]/page.va"), "").unwrap();

    let res = scan_app_directory(tmp.path());
    assert!(
        res.is_err(),
        "/blog/[id] and /blog/[slug] must be rejected as conflicting"
    );
}

#[test]
fn test_20_conflict_duplicate_required_catch_all_different_names() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("docs/[...path]")).unwrap();
    fs::create_dir_all(tmp.path().join("docs/[...rest]")).unwrap();
    fs::write(tmp.path().join("docs/[...path]/page.va"), "").unwrap();
    fs::write(tmp.path().join("docs/[...rest]/page.va"), "").unwrap();

    let res = scan_app_directory(tmp.path());
    assert!(
        res.is_err(),
        "/docs/[...path] and /docs/[...rest] must be rejected as conflicting"
    );
}

#[test]
fn test_21_conflict_duplicate_optional_catch_all_different_names() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("store/[[...filter]]")).unwrap();
    fs::create_dir_all(tmp.path().join("store/[[...category]]")).unwrap();
    fs::write(tmp.path().join("store/[[...filter]]/page.va"), "").unwrap();
    fs::write(tmp.path().join("store/[[...category]]/page.va"), "").unwrap();

    let res = scan_app_directory(tmp.path());
    assert!(
        res.is_err(),
        "/store/[[...filter]] and /store/[[...category]] must be rejected as conflicting"
    );
}

#[test]
fn test_22_conflict_mixed_dynamic_prefix_plus_required_catch_all() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("docs/[id]/[...path]")).unwrap();
    fs::create_dir_all(tmp.path().join("docs/[slug]/[...rest]")).unwrap();
    fs::write(tmp.path().join("docs/[id]/[...path]/page.va"), "").unwrap();
    fs::write(tmp.path().join("docs/[slug]/[...rest]/page.va"), "").unwrap();

    let res = scan_app_directory(tmp.path());
    assert!(
        res.is_err(),
        "/docs/[id]/[...path] and /docs/[slug]/[...rest] must be rejected as conflicting"
    );
}

#[test]
fn test_23_conflict_route_groups_normalize_to_equivalent() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("(marketing)/docs/[...path]")).unwrap();
    fs::create_dir_all(tmp.path().join("(docs_group)/docs/[...rest]")).unwrap();
    fs::write(tmp.path().join("(marketing)/docs/[...path]/page.va"), "").unwrap();
    fs::write(tmp.path().join("(docs_group)/docs/[...rest]/page.va"), "").unwrap();

    let res = scan_app_directory(tmp.path());
    assert!(
        res.is_err(),
        "Route groups normalizing to /docs/[...path] and /docs/[...rest] must be rejected as conflicting"
    );
}

#[test]
fn test_24_valid_static_vs_dynamic() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("blog/new")).unwrap();
    fs::create_dir_all(tmp.path().join("blog/[id]")).unwrap();
    fs::write(tmp.path().join("blog/new/page.va"), "").unwrap();
    fs::write(tmp.path().join("blog/[id]/page.va"), "").unwrap();

    let res = scan_app_directory(tmp.path());
    assert!(
        res.is_ok(),
        "/blog/new and /blog/[id] have distinct precedence and must remain valid"
    );
}

#[test]
fn test_25_valid_dynamic_vs_required_catch_all() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("posts/[id]")).unwrap();
    fs::create_dir_all(tmp.path().join("posts/[...slug]")).unwrap();
    fs::write(tmp.path().join("posts/[id]/page.va"), "").unwrap();
    fs::write(tmp.path().join("posts/[...slug]/page.va"), "").unwrap();

    let res = scan_app_directory(tmp.path());
    assert!(
        res.is_ok(),
        "/posts/[id] and /posts/[...slug] have distinct precedence and must remain valid"
    );
}

#[test]
fn test_26_valid_required_vs_optional_catch_all() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("docs/[...path]")).unwrap();
    fs::create_dir_all(tmp.path().join("docs/[[...filter]]")).unwrap();
    fs::write(tmp.path().join("docs/[...path]/page.va"), "").unwrap();
    fs::write(tmp.path().join("docs/[[...filter]]/page.va"), "").unwrap();

    let res = scan_app_directory(tmp.path());
    assert!(
        res.is_ok(),
        "/docs/[...path] and /docs/[[...filter]] have distinct precedence and must remain valid"
    );
}

#[test]
fn test_27_valid_exact_route_vs_optional_catch_all() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("shop")).unwrap();
    fs::create_dir_all(tmp.path().join("shop/[[...filter]]")).unwrap();
    fs::write(tmp.path().join("shop/page.va"), "").unwrap();
    fs::write(tmp.path().join("shop/[[...filter]]/page.va"), "").unwrap();

    let res = scan_app_directory(tmp.path());
    assert!(
        res.is_ok(),
        "/shop and /shop/[[...filter]] have distinct precedence and must remain valid"
    );
}

#[test]
fn test_28_method_discovery_from_route_file() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("api/items")).unwrap();
    fs::write(
        tmp.path().join("api/items/route.va"),
        "pub fn GET() {}\npub fn POST() {}",
    )
    .unwrap();

    let res = scan_app_directory(tmp.path()).expect("Scan must succeed");
    let items = res
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/api/items")
        .expect("/api/items must be found");

    assert!(items.methods.contains(&"GET".to_string()));
    assert!(items.methods.contains(&"POST".to_string()));
    assert!(items.methods.contains(&"HEAD".to_string())); // GET implicitly supports HEAD
    assert!(!items.methods.contains(&"DELETE".to_string()));
    assert!(!items.methods.contains(&"PUT".to_string()));
}
