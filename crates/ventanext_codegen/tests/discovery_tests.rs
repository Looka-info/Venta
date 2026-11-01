// ============================================================================
// ⚡ VentaNext Framework — Automated Route Discovery & Generator Tests
// Tests covering all 13 discovery, conflict, and manifest generation scenarios
// ============================================================================

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use ventanext_codegen::{
    adapter_filename_len_for_bytes, discover_api_methods, export_static_site,
    generate_api_adapter_code, generate_manifest_json, generate_venta_code, parse_segment,
    sanitize_module_name, scan_app_directory, validate_api_route_handlers, validate_conflicts,
    write_generated_routes, DiscoveredApp, RenderMode, RouteKind, RouteMetadata, Segment,
    SegmentKind, SpecialFilesMetadata, ADAPTER_FILENAME_WRAPPER_LEN,
    MAX_PORTABLE_ADAPTER_FILENAME_LEN, MAX_PORTABLE_ROUTE_PATTERN_BYTES,
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

    fs::write(
        base.join("api/health/route.va"),
        "import std::string::String;\npub fn handle_api() -> String { String::from_str(\"{\\\"status\\\":\\\"ok\\\"}\") }",
    )
    .unwrap();
    fs::write(
        base.join("api/users/route.va"),
        "import std::string::String;\npub fn GET() -> String { String::from_str(\"{\\\"users\\\":[]}\") }",
    )
    .unwrap();
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
        "pub fn GET() -> String { String::from_str(\"\") }\npub fn POST() -> String { String::from_str(\"\") }",
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

#[test]
fn test_29_exported_get_and_post_discovered() {
    let code = r#"
        pub fn GET() -> String { "get" }
        pub fn POST() -> String { "post" }
    "#;
    let methods = discover_api_methods(code);
    assert!(methods.contains(&"GET".to_string()));
    assert!(methods.contains(&"POST".to_string()));
    assert!(methods.contains(&"HEAD".to_string())); // Implied HEAD
    assert!(!methods.contains(&"DELETE".to_string()));
    assert!(!methods.contains(&"PUT".to_string()));
}

#[test]
fn test_30_private_get_is_ignored() {
    let code = r#"
        fn GET() -> String { "private get" }
        pub fn POST() -> String { "exported post" }
    "#;
    let methods = discover_api_methods(code);
    assert!(!methods.contains(&"GET".to_string()));
    assert!(!methods.contains(&"HEAD".to_string())); // No GET, so no implied HEAD
    assert!(methods.contains(&"POST".to_string()));
    assert_eq!(methods, vec!["POST".to_string()]);
}

#[test]
fn test_31_commented_out_delete_is_ignored() {
    let code = r#"
        // pub fn DELETE() -> String { "deleted" }
        /*
        pub fn DELETE() -> String { "block deleted" }
        */
        pub fn POST() -> String { "post" }
    "#;
    let methods = discover_api_methods(code);
    assert!(!methods.contains(&"DELETE".to_string()));
    assert_eq!(methods, vec!["POST".to_string()]);
}

#[test]
fn test_32_string_literal_containing_put_is_ignored() {
    let code = r#"
        pub fn POST() -> String {
            let fake_decl = "pub fn PUT() -> String { \"fake\" }";
            fake_decl
        }
    "#;
    let methods = discover_api_methods(code);
    assert!(!methods.contains(&"PUT".to_string()));
    assert_eq!(methods, vec!["POST".to_string()]);
}

#[test]
fn test_33_similar_identifier_getting_does_not_register_get() {
    let code = r#"
        pub fn GETTING() -> String { "getting" }
        pub fn POST() -> String { "post" }
    "#;
    let methods = discover_api_methods(code);
    assert!(!methods.contains(&"GET".to_string()));
    assert!(!methods.contains(&"HEAD".to_string()));
    assert_eq!(methods, vec!["POST".to_string()]);
}

#[test]
fn test_34_multiline_exported_function_declarations_recognized() {
    let code = r#"
        pub
            fn
                GET(
                    req: &Request
                ) -> Response {
            Response::ok()
        }
        pub
            fn
                POST(
                    req: &Request
                ) -> Response {
            Response::ok()
        }
    "#;
    let methods = discover_api_methods(code);
    assert!(methods.contains(&"GET".to_string()));
    assert!(methods.contains(&"POST".to_string()));
    assert!(methods.contains(&"HEAD".to_string())); // Implied HEAD
    assert_eq!(
        methods,
        vec!["GET".to_string(), "POST".to_string(), "HEAD".to_string()]
    );
}

#[test]
fn test_35_explicit_head_recognized() {
    let code = r#"
        pub fn HEAD() -> String { "" }
    "#;
    let methods = discover_api_methods(code);
    assert_eq!(methods, vec!["HEAD".to_string()]);
}

#[test]
fn test_36_get_automatically_adds_head() {
    let code = r#"
        pub fn GET() -> String { "only get declared" }
    "#;
    let methods = discover_api_methods(code);
    assert_eq!(methods, vec!["GET".to_string(), "HEAD".to_string()]);
}

#[test]
fn test_37_route_with_no_recognized_methods_retains_fallback() {
    let code = r#"
        // Generic handler pattern without individual HTTP method functions
        pub fn handle_api() -> String { "fallback" }
    "#;
    let methods = discover_api_methods(code);
    assert_eq!(
        methods,
        vec![
            "GET".to_string(),
            "POST".to_string(),
            "PUT".to_string(),
            "DELETE".to_string(),
            "PATCH".to_string(),
            "HEAD".to_string(),
            "OPTIONS".to_string(),
        ]
    );
}

#[test]
fn test_38_nested_function_declarations_ignored() {
    let code = r#"
        pub fn helper() {
            pub fn DELETE() {}
        }
        pub fn POST() {}
    "#;
    let methods = discover_api_methods(code);
    assert!(!methods.contains(&"DELETE".to_string()));
    assert_eq!(methods, vec!["POST".to_string()]);
}

#[test]
fn test_39_api_users_manifest_registers_only_get_and_head() {
    let app_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("app");
    if app_dir.exists() {
        let app = scan_app_directory(&app_dir).expect("Scan of real app/ directory must succeed");
        let users = app
            .routes
            .iter()
            .find(|r| r.canonical_pattern == "/api/users")
            .expect("/api/users must be registered in app/");
        assert_eq!(
            users.methods,
            vec!["GET".to_string(), "HEAD".to_string()],
            "/api/users route must only declare GET and implied HEAD"
        );
    }
}

#[test]
fn test_40_static_api_adapter_generation_for_users() {
    let app_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("app");
    if app_dir.exists() {
        let app = scan_app_directory(&app_dir).expect("Scan of real app/ directory must succeed");
        let users = app
            .routes
            .iter()
            .find(|r| r.canonical_pattern == "/api/users")
            .expect("/api/users must be registered");
        let content = std::fs::read_to_string(&users.source_file).unwrap();
        let adapter = generate_api_adapter_code(users, &content);

        assert!(adapter.contains("pub fn GET() -> String"));
        assert!(adapter.contains("Ada Lovelace"));
        assert!(adapter.contains("Alan Turing"));
        assert!(adapter.contains("pub fn invoke_get() -> String {\n    GET()\n}"));
        assert!(adapter.contains("pub fn invoke_head() -> String {\n    GET()\n}"));
    }
}

#[test]
fn test_41_static_api_adapter_generation_for_health() {
    let app_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("app");
    if app_dir.exists() {
        let app = scan_app_directory(&app_dir).expect("Scan of real app/ directory must succeed");
        let health = app
            .routes
            .iter()
            .find(|r| r.canonical_pattern == "/api/health")
            .expect("/api/health must be registered");
        let content = std::fs::read_to_string(&health.source_file).unwrap();
        let adapter = generate_api_adapter_code(health, &content);

        assert!(adapter.contains("pub fn handle_api() -> String"));
        assert!(adapter.contains("ventanext-health"));
        assert!(adapter.contains("pub fn invoke_handle_api() -> String {\n    handle_api()\n}"));
    }
}

#[test]
fn test_42_generated_dispatcher_contains_no_generic_placeholder() {
    let app_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("app");
    if app_dir.exists() {
        let app = scan_app_directory(&app_dir).expect("Scan of real app/ directory must succeed");
        let code = generate_venta_code(&app);

        // Must invoke static adapter bridges
        assert!(code.contains("mod route_2f6170692f7573657273;"));
        assert!(code.contains("mod route_2f6170692f6865616c7468;"));
        assert!(code.contains("route_2f6170692f7573657273::invoke_get()"));
        assert!(code.contains("route_2f6170692f7573657273::invoke_head()"));
        assert!(code.contains("route_2f6170692f6865616c7468::invoke_handle_api()"));

        // Must NOT contain placeholder responses
        assert!(!code.contains("{\"status\":\"ok\",\"endpoint\":\"/api/users\""));
        assert!(!code.contains("{\"status\":\"ok\",\"endpoint\":\"/api/health\""));
    }
}

#[test]
fn test_43_unsupported_handler_signature_with_parameters_produces_diagnostic() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("route.va");
    let code = r#"
        import http::Request;
        import http::Response;
        pub fn GET(req: &Request) -> Response {
            Response::ok()
        }
    "#;
    let res = validate_api_route_handlers(&file_path, code);
    assert!(
        res.is_err(),
        "Handlers with parameters must be rejected in Phase 1.3A"
    );
    let err = res.err().unwrap();
    assert!(
        err.message.contains("Unsupported handler signature"),
        "Error message must clearly state unsupported signature"
    );
    assert!(
        err.message.contains("1 parameter(s)"),
        "Error message must specify parameter count"
    );
}

#[test]
fn test_44_unsupported_handler_signature_with_non_string_return_produces_diagnostic() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("route.va");
    let code = r#"
        pub fn GET() -> i64 {
            42
        }
    "#;
    let res = validate_api_route_handlers(&file_path, code);
    assert!(
        res.is_err(),
        "Handlers with non-String return types must be rejected"
    );
    let err = res.err().unwrap();
    assert!(
        err.message.contains("Unsupported return type"),
        "Error message must clearly state unsupported return type"
    );
}

#[test]
fn test_45_unsupported_handler_missing_return_type_produces_diagnostic() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("route.va");
    let code = r#"
        pub fn GET() {
        }
    "#;
    let res = validate_api_route_handlers(&file_path, code);
    assert!(
        res.is_err(),
        "Handlers with unit return type must be rejected"
    );
    let err = res.err().unwrap();
    assert!(
        err.message.contains("Unsupported return type"),
        "Error message must clearly state unsupported return type"
    );
}

#[test]
fn test_46_write_generated_routes_creates_adapters_and_manifest() {
    let app_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("app");
    if app_dir.exists() {
        let app = scan_app_directory(&app_dir).expect("Scan must succeed");
        let tmp = tempfile::tempdir().unwrap();
        let out_va = tmp.path().join("generated_routes.va");

        write_generated_routes(&app, &out_va).expect("write_generated_routes must succeed");

        assert!(out_va.exists(), "generated_routes.va must be created");
        let adapter_dir = tmp.path().join("generated_routes");
        assert!(
            adapter_dir.exists(),
            "generated_routes directory must be created"
        );
        assert!(
            adapter_dir.join("route_2f6170692f7573657273.va").exists(),
            "route_2f6170692f7573657273.va must be created"
        );
        assert!(
            adapter_dir.join("route_2f6170692f6865616c7468.va").exists(),
            "route_2f6170692f6865616c7468.va must be created"
        );

        let users_content =
            std::fs::read_to_string(adapter_dir.join("route_2f6170692f7573657273.va")).unwrap();
        assert!(users_content.contains("Ada Lovelace"));
        assert!(users_content.contains("invoke_get"));
    }
}

#[test]
fn test_47_changing_handler_data_changes_generated_adapter() {
    let tmp = tempfile::tempdir().unwrap();
    let app_dir = tmp.path().join("app");
    fs::create_dir_all(app_dir.join("api/data")).unwrap();

    let route_file = app_dir.join("api/data/route.va");
    fs::write(
        &route_file,
        r#"
        import std::string::String;
        pub fn GET() -> String {
            String::from_str("{\"author\":\"Ada Lovelace\"}")
        }
        "#,
    )
    .unwrap();

    let app1 = scan_app_directory(&app_dir).unwrap();
    let out_va = tmp.path().join("ventanext/generated_routes.va");
    write_generated_routes(&app1, &out_va).unwrap();

    let adapter_path = tmp
        .path()
        .join("ventanext/generated_routes/route_2f6170692f64617461.va");
    let content1 = fs::read_to_string(&adapter_path).unwrap();
    assert!(content1.contains("Ada Lovelace"));

    // Modify route file with new data
    fs::write(
        &route_file,
        r#"
        import std::string::String;
        pub fn GET() -> String {
            String::from_str("{\"author\":\"Grace Hopper\"}")
        }
        "#,
    )
    .unwrap();

    let app2 = scan_app_directory(&app_dir).unwrap();
    write_generated_routes(&app2, &out_va).unwrap();

    let content2 = fs::read_to_string(&adapter_path).unwrap();
    assert!(!content2.contains("Ada Lovelace"));
    assert!(content2.contains("Grace Hopper"));
}

#[test]
fn test_48_generation_determinism_across_runs() {
    let app_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("app");
    if app_dir.exists() {
        let app1 = scan_app_directory(&app_dir).unwrap();
        let app2 = scan_app_directory(&app_dir).unwrap();

        let tmp1 = tempfile::tempdir().unwrap();
        let out1 = tmp1.path().join("generated_routes.va");
        write_generated_routes(&app1, &out1).unwrap();

        let tmp2 = tempfile::tempdir().unwrap();
        let out2 = tmp2.path().join("generated_routes.va");
        write_generated_routes(&app2, &out2).unwrap();

        let code1 = fs::read_to_string(&out1).unwrap();
        let code2 = fs::read_to_string(&out2).unwrap();
        assert_eq!(code1, code2, "Generated code must be 100% deterministic");

        let adapter1_users = fs::read_to_string(
            tmp1.path()
                .join("generated_routes/route_2f6170692f7573657273.va"),
        )
        .unwrap();
        let adapter2_users = fs::read_to_string(
            tmp2.path()
                .join("generated_routes/route_2f6170692f7573657273.va"),
        )
        .unwrap();
        assert_eq!(
            adapter1_users, adapter2_users,
            "Adapter files must be 100% deterministic"
        );
    }
}

// ----------------------------------------------------------------------------
// Phase 1.3A.1 Regression Test Suite: Fail-Closed API Handler Validation
// ----------------------------------------------------------------------------

#[test]
fn test_49_valid_exported_get_handler_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("route.va");
    let code = r#"
        import std::string::String;
        pub fn GET() -> String {
            String::from_str("{\"users\":[]}")
        }
    "#;
    let res = validate_api_route_handlers(&file_path, code);
    assert!(res.is_ok(), "Valid exported GET handler must succeed");
    let val = res.unwrap();
    assert_eq!(val.explicit_methods, vec!["GET".to_string()]);
    assert_eq!(val.methods, vec!["GET".to_string(), "HEAD".to_string()]);
    assert!(!val.has_generic_handler);
}

#[test]
fn test_50_valid_exported_generic_handle_api_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("route.va");
    let code = r#"
        import std::string::String;
        pub fn handle_api() -> String {
            String::from_str("{\"status\":\"ok\"}")
        }
    "#;
    let res = validate_api_route_handlers(&file_path, code);
    assert!(
        res.is_ok(),
        "Valid exported generic handle_api must succeed"
    );
    let val = res.unwrap();
    assert!(val.has_generic_handler);
    assert!(val.explicit_methods.is_empty());
    assert_eq!(
        val.methods,
        vec![
            "GET".to_string(),
            "POST".to_string(),
            "PUT".to_string(),
            "DELETE".to_string(),
            "PATCH".to_string(),
            "HEAD".to_string(),
            "OPTIONS".to_string(),
        ]
    );
}

#[test]
fn test_51_private_get_alone_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("route.va");
    let code = r#"
        import std::string::String;
        fn GET() -> String {
            String::from_str("private")
        }
    "#;
    let res = validate_api_route_handlers(&file_path, code);
    assert!(res.is_err(), "Private GET alone must be rejected");
    let err = res.err().unwrap();
    assert!(
        err.message.contains("No valid API handler found"),
        "Diagnostic must state no valid API handler was found: {}",
        err.message
    );
}

#[test]
fn test_52_unrelated_public_helper_alone_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("route.va");
    let code = r#"
        import std::string::String;
        pub fn calculate_tax() -> String {
            String::from_str("10%")
        }
    "#;
    let res = validate_api_route_handlers(&file_path, code);
    assert!(
        res.is_err(),
        "Unrelated public helper alone must be rejected"
    );
    let err = res.err().unwrap();
    assert!(
        err.message.contains("No valid API handler found"),
        "Diagnostic must state no valid API handler was found: {}",
        err.message
    );
}

#[test]
fn test_53_empty_route_file_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("route.va");
    let code = "   \n\n  // just comments\n";
    let res = validate_api_route_handlers(&file_path, code);
    assert!(res.is_err(), "Empty route file must be rejected");
    let err = res.err().unwrap();
    assert!(
        err.message.contains("No valid API handler found"),
        "Diagnostic must state no valid API handler was found: {}",
        err.message
    );
}

#[test]
fn test_54_malformed_venta_source_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("route.va");
    let code = r#"
        pub fn GET( {
            broken syntax
    "#;
    let res = validate_api_route_handlers(&file_path, code);
    assert!(res.is_err(), "Malformed Venta source must be rejected");
    let err = res.err().unwrap();
    assert!(
        err.message.contains("Parse error in API route file")
            || err.message.contains("Lexer error in API route file"),
        "Diagnostic must indicate parse or lexer error: {}",
        err.message
    );
}

#[test]
fn test_55_parser_errors_cannot_trigger_compatibility_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("route.va");
    let code = r#"
        pub fn handle_api() -> String {
            let x = ;
        }
    "#;
    let res = validate_api_route_handlers(&file_path, code);
    assert!(
        res.is_err(),
        "Parser errors must reject route rather than triggering fallback"
    );
    let err = res.err().unwrap();
    assert!(
        err.message.contains("Parse error in API route file"),
        "Diagnostic must indicate parse error: {}",
        err.message
    );
}

#[test]
fn test_56_mixed_handle_api_and_explicit_get_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("route.va");
    let code = r#"
        import std::string::String;
        pub fn handle_api() -> String {
            String::from_str("fallback")
        }
        pub fn GET() -> String {
            String::from_str("explicit get")
        }
    "#;
    let res = validate_api_route_handlers(&file_path, code);
    assert!(
        res.is_err(),
        "Mixed handle_api and explicit GET must be rejected"
    );
    let err = res.err().unwrap();
    assert!(
        err.message.contains("Mixed API handler conventions"),
        "Diagnostic must identify mixed API handler conventions: {}",
        err.message
    );
    assert!(
        err.message.contains(
            "route contains both generic 'pub fn handle_api()' and explicit HTTP handler(s)"
        ),
        "Diagnostic must detail both handler kinds: {}",
        err.message
    );
}

#[test]
fn test_57_private_helper_alongside_valid_get_allowed() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("route.va");
    let code = r#"
        import std::string::String;
        fn internal_fetch() -> String {
            String::from_str("{\"result\":\"ok\"}")
        }
        pub fn GET() -> String {
            internal_fetch()
        }
    "#;
    let res = validate_api_route_handlers(&file_path, code);
    assert!(
        res.is_ok(),
        "Private helper alongside valid public GET must be allowed"
    );
    let val = res.unwrap();
    assert_eq!(val.explicit_methods, vec!["GET".to_string()]);
    assert_eq!(val.methods, vec!["GET".to_string(), "HEAD".to_string()]);
}

#[test]
fn test_58_unsupported_parameters_and_return_types_fail() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("route.va");

    // 1. handle_api with parameter
    let code1 = r#"
        import std::string::String;
        pub fn handle_api(flag: i64) -> String {
            String::from_str("ok")
        }
    "#;
    let res1 = validate_api_route_handlers(&file_path, code1);
    assert!(res1.is_err());
    assert!(res1.unwrap_err().message.contains("1 parameter(s)"));

    // 2. handle_api with non-String return
    let code2 = r#"
        pub fn handle_api() -> i64 {
            200
        }
    "#;
    let res2 = validate_api_route_handlers(&file_path, code2);
    assert!(res2.is_err());
    assert!(res2
        .unwrap_err()
        .message
        .contains("Unsupported return type"));

    // 3. handle_api with unit return
    let code3 = r#"
        pub fn handle_api() {}
    "#;
    let res3 = validate_api_route_handlers(&file_path, code3);
    assert!(res3.is_err());
    assert!(res3
        .unwrap_err()
        .message
        .contains("Unsupported return type"));

    // 4. POST with parameter
    let code4 = r#"
        import std::string::String;
        pub fn POST(body: String) -> String {
            body
        }
    "#;
    let res4 = validate_api_route_handlers(&file_path, code4);
    assert!(res4.is_err());
    assert!(res4.unwrap_err().message.contains("1 parameter(s)"));

    // 5. POST with non-String return
    let code5 = r#"
        pub fn POST() -> bool {
            true
        }
    "#;
    let res5 = validate_api_route_handlers(&file_path, code5);
    assert!(res5.is_err());
    assert!(res5
        .unwrap_err()
        .message
        .contains("Unsupported return type"));
}

#[test]
fn test_59_generated_adapters_never_contain_fake_success_response() {
    let route_users = RouteMetadata {
        id: 1,
        canonical_pattern: "/api/users".to_string(),
        normalized_url: "/api/users".to_string(),
        route_kind: RouteKind::Api,
        render_mode: RenderMode::Dynamic,
        revalidate_seconds: None,
        source_file: "app/api/users/route.va".to_string(),
        segments: vec![],
        layout_chain: vec![],
        methods: vec!["GET".to_string(), "HEAD".to_string()],
        specificity: 1000,
        has_generic_handler: false,
        explicit_methods: vec!["GET".to_string()],
        adapter_module_name: "route_2f6170692f7573657273".to_string(),
        source_content: "pub fn GET() -> String { String::from_str(\"users\") }".to_string(),
    };

    let adapter_users = generate_api_adapter_code(&route_users, &route_users.source_content);
    assert!(
        !adapter_users.contains("pub fn invoke_handle_api() -> String {\n    String::from_str(\"{\\\"status\\\":\\\"ok\\\"}\")\n}"),
        "Generated adapter for explicit route must NEVER contain fake success response"
    );
    assert!(adapter_users.contains("invoke_get"));
    assert!(adapter_users.contains("invoke_head"));

    let route_health = RouteMetadata {
        id: 0,
        canonical_pattern: "/api/health".to_string(),
        normalized_url: "/api/health".to_string(),
        route_kind: RouteKind::Api,
        render_mode: RenderMode::Dynamic,
        revalidate_seconds: None,
        source_file: "app/api/health/route.va".to_string(),
        segments: vec![],
        layout_chain: vec![],
        methods: vec![
            "GET".to_string(),
            "POST".to_string(),
            "PUT".to_string(),
            "DELETE".to_string(),
            "PATCH".to_string(),
            "HEAD".to_string(),
            "OPTIONS".to_string(),
        ],
        specificity: 1000,
        has_generic_handler: true,
        explicit_methods: vec![],
        adapter_module_name: "route_2f6170692f6865616c7468".to_string(),
        source_content: "pub fn handle_api() -> String { String::from_str(\"health\") }"
            .to_string(),
    };

    let adapter_health = generate_api_adapter_code(&route_health, &route_health.source_content);
    assert!(
        !adapter_health.contains("String::from_str(\"{\\\"status\\\":\\\"ok\\\"}\")"),
        "Generated adapter for generic route must delegate to real handle_api(), not fake status:ok"
    );
    assert!(adapter_health.contains("handle_api()"));
}

#[test]
fn test_60_valid_existing_routes_generate_and_execute_correctly() {
    let app_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("app");
    if app_dir.exists() {
        let app = scan_app_directory(&app_dir).expect("Real app directory must scan successfully");

        let users = app
            .routes
            .iter()
            .find(|r| r.canonical_pattern == "/api/users")
            .expect("/api/users must exist");
        assert_eq!(users.explicit_methods, vec!["GET".to_string()]);
        assert_eq!(users.methods, vec!["GET".to_string(), "HEAD".to_string()]);
        assert!(!users.has_generic_handler);

        let health = app
            .routes
            .iter()
            .find(|r| r.canonical_pattern == "/api/health")
            .expect("/api/health must exist");
        assert!(health.has_generic_handler);
        assert!(health.explicit_methods.is_empty());
        assert_eq!(health.methods.len(), 7);

        let tmp = tempfile::tempdir().unwrap();
        let out_va = tmp.path().join("generated_routes.va");
        write_generated_routes(&app, &out_va).expect("write_generated_routes must succeed");

        let code = fs::read_to_string(&out_va).unwrap();
        assert!(code.contains("mod route_2f6170692f7573657273;"));
        assert!(code.contains("mod route_2f6170692f6865616c7468;"));
        assert!(code.contains("route_2f6170692f7573657273::invoke_get()"));
        assert!(code.contains("route_2f6170692f7573657273::invoke_head()"));
        assert!(code.contains("route_2f6170692f6865616c7468::invoke_handle_api()"));
        assert!(!code.contains("status\":\"ok\""));
    }
}

// ----------------------------------------------------------------------------
// Phase 1.3A.2 Regression Test Suite: Collision-Resistant Adapter Module Names
// ----------------------------------------------------------------------------

#[test]
fn test_61_api_users_and_api_dash_users_receive_different_module_identifiers() {
    let mod_users = sanitize_module_name("/api/users");
    let mod_dash = sanitize_module_name("/api-users");

    assert_eq!(mod_users, "route_2f6170692f7573657273");
    assert_eq!(mod_dash, "route_2f6170692d7573657273");
    assert_ne!(
        mod_users, mod_dash,
        "Distinct routes '/api/users' and '/api-users' must receive different module identifiers"
    );
    assert!(mod_users.starts_with("route_"));
    assert!(mod_dash.starts_with("route_"));
}

#[test]
fn test_62_api_bracket_id_and_api_dash_id_receive_different_identifiers() {
    let mod_param = sanitize_module_name("/api/[id]");
    let mod_dash = sanitize_module_name("/api-id");

    assert_eq!(mod_param, "route_2f6170692f5b69645d");
    assert_eq!(mod_dash, "route_2f6170692d6964");
    assert_ne!(
        mod_param, mod_dash,
        "'/api/[id]' and '/api-id' must receive different module identifiers"
    );
}

#[test]
fn test_63_underscores_and_hyphens_remain_distinct() {
    let mod_underscore = sanitize_module_name("/api_users");
    let mod_hyphen = sanitize_module_name("/api-users");
    let mod_slash = sanitize_module_name("/api/users");

    assert_eq!(mod_underscore, "route_2f6170695f7573657273");
    assert_eq!(mod_hyphen, "route_2f6170692d7573657273");
    assert_eq!(mod_slash, "route_2f6170692f7573657273");

    assert_ne!(mod_underscore, mod_hyphen);
    assert_ne!(mod_underscore, mod_slash);
    assert_ne!(mod_hyphen, mod_slash);
}

#[test]
fn test_64_multiple_separators_remain_distinct() {
    let mod1 = sanitize_module_name("/a/b/c");
    let mod2 = sanitize_module_name("/a_b_c");
    let mod3 = sanitize_module_name("/a-b-c");
    let mod4 = sanitize_module_name("/a//b/c");

    let set: BTreeSet<String> = [mod1, mod2, mod3, mod4].into_iter().collect();
    assert_eq!(
        set.len(),
        4,
        "All variations with different separators must be uniquely encoded"
    );
}

#[test]
fn test_65_unicode_route_patterns_encoded_deterministically() {
    let mod_cafe = sanitize_module_name("/api/café");
    let mod_unicode = sanitize_module_name("/api/ünicode");

    assert!(mod_cafe.starts_with("route_"));
    assert!(mod_unicode.starts_with("route_"));
    // Ensure identifier contains only safe hex characters after prefix
    assert!(mod_cafe["route_".len()..]
        .chars()
        .all(|c: char| c.is_ascii_hexdigit()));
    assert!(mod_unicode["route_".len()..]
        .chars()
        .all(|c: char| c.is_ascii_hexdigit()));

    // Deterministic across repeated calls
    assert_eq!(mod_cafe, sanitize_module_name("/api/café"));
    assert_eq!(mod_unicode, sanitize_module_name("/api/ünicode"));
}

#[test]
fn test_66_same_route_produces_identical_identifier_across_repeated_runs() {
    let patterns = vec![
        "/api/users",
        "/api-users",
        "/api/posts/[id]",
        "/shop/[[...filter]]",
        "/docs/[...path]",
    ];
    for p in patterns {
        let first = sanitize_module_name(p);
        for _ in 0..50 {
            assert_eq!(first, sanitize_module_name(p));
        }
    }
}

#[test]
fn test_67_temporary_app_with_colliding_routes_pipeline() {
    let tmp = tempfile::tempdir().unwrap();
    let app_dir = tmp.path().join("app");
    fs::create_dir_all(app_dir.join("api/users")).unwrap();
    fs::create_dir_all(app_dir.join("api-users")).unwrap();

    fs::write(
        app_dir.join("api/users/route.va"),
        "import std::string::String;\npub fn GET() -> String { String::from_str(\"users\") }",
    )
    .unwrap();
    fs::write(
        app_dir.join("api-users/route.va"),
        "import std::string::String;\npub fn GET() -> String { String::from_str(\"customers\") }",
    )
    .unwrap();

    let discovered = scan_app_directory(&app_dir).expect("Both distinct routes must be discovered");
    assert_eq!(discovered.routes.len(), 2);

    let route_users = discovered
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/api/users")
        .unwrap();
    let route_dash = discovered
        .routes
        .iter()
        .find(|r| r.canonical_pattern == "/api-users")
        .unwrap();

    assert_ne!(
        route_users.adapter_module_name,
        route_dash.adapter_module_name
    );
    assert_eq!(
        route_users.adapter_module_name,
        "route_2f6170692f7573657273"
    );
    assert_eq!(route_dash.adapter_module_name, "route_2f6170692d7573657273");

    let out_va = tmp.path().join("ventanext/generated_routes.va");
    write_generated_routes(&discovered, &out_va).expect("write_generated_routes must succeed");

    let adapter_dir = tmp.path().join("ventanext/generated_routes");
    let file_users = adapter_dir.join("route_2f6170692f7573657273.va");
    let file_dash = adapter_dir.join("route_2f6170692d7573657273.va");

    assert!(
        file_users.exists(),
        "route_2f6170692f7573657273.va must exist"
    );
    assert!(
        file_dash.exists(),
        "route_2f6170692d7573657273.va must exist"
    );

    let content_users = fs::read_to_string(&file_users).unwrap();
    let content_dash = fs::read_to_string(&file_dash).unwrap();

    assert!(content_users.contains("String::from_str(\"users\")"));
    assert!(!content_users.contains("customers"));
    assert!(content_dash.contains("customers"));
    assert!(!content_dash.contains("String::from_str(\"users\")"));
}

#[test]
fn test_68_dispatcher_declares_each_adapter_exactly_once() {
    let tmp = tempfile::tempdir().unwrap();
    let app_dir = tmp.path().join("app");
    fs::create_dir_all(app_dir.join("api/users")).unwrap();
    fs::create_dir_all(app_dir.join("api-users")).unwrap();

    fs::write(
        app_dir.join("api/users/route.va"),
        "import std::string::String;\npub fn GET() -> String { String::from_str(\"users\") }",
    )
    .unwrap();
    fs::write(
        app_dir.join("api-users/route.va"),
        "import std::string::String;\npub fn GET() -> String { String::from_str(\"customers\") }",
    )
    .unwrap();

    let discovered = scan_app_directory(&app_dir).unwrap();
    let code = generate_venta_code(&discovered);

    assert_eq!(code.matches("mod route_2f6170692f7573657273;").count(), 1);
    assert_eq!(code.matches("mod route_2f6170692d7573657273;").count(), 1);
    assert!(code.contains("route_2f6170692f7573657273::invoke_get()"));
    assert!(code.contains("route_2f6170692d7573657273::invoke_get()"));
}

#[test]
fn test_69_enforce_uniqueness_rejects_duplicate_adapter_module_names() {
    let r1 = RouteMetadata {
        id: 0,
        canonical_pattern: "/api/one".to_string(),
        normalized_url: "/api/one".to_string(),
        route_kind: RouteKind::Api,
        render_mode: RenderMode::Dynamic,
        revalidate_seconds: None,
        source_file: "app/api/one/route.va".to_string(),
        segments: vec![Segment {
            raw: "one".to_string(),
            kind: SegmentKind::Static,
            param_name: None,
        }],
        layout_chain: vec![],
        methods: vec!["GET".to_string()],
        specificity: 1000,
        has_generic_handler: false,
        explicit_methods: vec!["GET".to_string()],
        adapter_module_name: "route_collision".to_string(),
        source_content: "pub fn GET() -> String { String::from_str(\"\") }".to_string(),
    };
    let r2 = RouteMetadata {
        id: 1,
        canonical_pattern: "/api/two".to_string(),
        normalized_url: "/api/two".to_string(),
        route_kind: RouteKind::Api,
        render_mode: RenderMode::Dynamic,
        revalidate_seconds: None,
        source_file: "app/api/two/route.va".to_string(),
        segments: vec![Segment {
            raw: "two".to_string(),
            kind: SegmentKind::Static,
            param_name: None,
        }],
        layout_chain: vec![],
        methods: vec!["GET".to_string()],
        specificity: 1000,
        has_generic_handler: false,
        explicit_methods: vec!["GET".to_string()],
        adapter_module_name: "route_collision".to_string(),
        source_content: "pub fn GET() -> String { String::from_str(\"\") }".to_string(),
    };

    let res = validate_conflicts(&[r1.clone(), r2.clone()]);
    assert!(
        res.is_err(),
        "Duplicate adapter module names must be rejected"
    );
    let err = res.err().unwrap();
    assert!(err.message.contains("API adapter module name collision"));

    let discovered = DiscoveredApp {
        routes: vec![r1, r2],
        special_files: SpecialFilesMetadata {
            root_layout: None,
            loading: None,
            error: None,
            not_found: None,
        },
    };
    let tmp = tempfile::tempdir().unwrap();
    let out_va = tmp.path().join("generated_routes.va");
    let write_res = write_generated_routes(&discovered, &out_va);
    assert!(
        write_res.is_err(),
        "write_generated_routes must reject duplicate adapter module names"
    );
}

#[test]
fn test_70_existing_users_and_health_generate_and_execute_with_injective_names() {
    let app_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("app");
    if app_dir.exists() {
        let app = scan_app_directory(&app_dir).unwrap();
        let users = app
            .routes
            .iter()
            .find(|r| r.canonical_pattern == "/api/users")
            .unwrap();
        let health = app
            .routes
            .iter()
            .find(|r| r.canonical_pattern == "/api/health")
            .unwrap();

        assert_eq!(users.adapter_module_name, "route_2f6170692f7573657273");
        assert_eq!(health.adapter_module_name, "route_2f6170692f6865616c7468");

        let tmp = tempfile::tempdir().unwrap();
        let out_va = tmp.path().join("generated_routes.va");
        write_generated_routes(&app, &out_va).unwrap();

        let adapter_dir = tmp.path().join("generated_routes");
        assert!(adapter_dir.join("route_2f6170692f7573657273.va").exists());
        assert!(adapter_dir.join("route_2f6170692f6865616c7468.va").exists());
    }
}

#[test]
fn test_71_route_at_maximum_supported_length_succeeds() {
    // MAX_PORTABLE_ROUTE_PATTERN_BYTES is 123 bytes.
    // Encoded filename length: 9 + 2 * 123 = 255 bytes (MAX_PORTABLE_ADAPTER_FILENAME_LEN).
    assert_eq!(MAX_PORTABLE_ROUTE_PATTERN_BYTES, 123);
    assert_eq!(adapter_filename_len_for_bytes(123), 255);

    let tmp = tempfile::tempdir().unwrap();
    let app_dir = tmp.path().join("app");
    // Canonical pattern: "/api/" (5 bytes) + 118 ASCII bytes = 123 bytes
    let seg118 = "a".repeat(118);
    let route_dir = app_dir.join("api").join(&seg118);
    fs::create_dir_all(&route_dir).unwrap();
    fs::write(
        route_dir.join("route.va"),
        "import std::string::String;\n\npub fn GET() -> String {\n    String::from_str(\"max_limit_reached\")\n}\n",
    )
    .unwrap();

    let discovered = scan_app_directory(&app_dir).expect("Route at 123 bytes must succeed");
    assert_eq!(discovered.routes.len(), 1);
    let r = &discovered.routes[0];

    assert_eq!(r.canonical_pattern.as_bytes().len(), 123);
    assert_eq!(r.canonical_pattern, format!("/api/{}", seg118));

    let expected_mod_name = sanitize_module_name(&r.canonical_pattern);
    assert_eq!(r.adapter_module_name, expected_mod_name);
    // 6 chars ("route_") + 246 hex chars = 252 chars
    assert_eq!(expected_mod_name.len(), 252);

    let adapter_filename = format!("{}.va", expected_mod_name);
    assert_eq!(adapter_filename.len(), 255);
    assert_eq!(adapter_filename.len(), MAX_PORTABLE_ADAPTER_FILENAME_LEN);

    let out_va = tmp.path().join("generated_routes.va");
    write_generated_routes(&discovered, &out_va)
        .expect("Writing adapter at 255 bytes limit must succeed");

    let adapter_path = tmp.path().join("generated_routes").join(&adapter_filename);
    assert!(adapter_path.exists());
    let adapter_content = fs::read_to_string(&adapter_path).unwrap();
    assert!(adapter_content.contains("pub fn invoke_get() -> String"));
}

#[test]
fn test_72_route_one_byte_beyond_maximum_is_rejected() {
    // 124 bytes canonical pattern -> 9 + 2 * 124 = 257 bytes filename (> 255 limit).
    assert_eq!(adapter_filename_len_for_bytes(124), 257);

    let tmp = tempfile::tempdir().unwrap();
    let app_dir = tmp.path().join("app");
    // Canonical pattern: "/api/" (5 bytes) + 119 ASCII bytes = 124 bytes
    let seg119 = "a".repeat(119);
    let route_dir = app_dir.join("api").join(&seg119);
    fs::create_dir_all(&route_dir).unwrap();
    let file_path = route_dir.join("route.va");
    fs::write(
        &file_path,
        "import std::string::String;\n\npub fn GET() -> String {\n    String::from_str(\"overflow\")\n}\n",
    )
    .unwrap();

    let res = scan_app_directory(&app_dir);
    assert!(res.is_err(), "Route at 124 bytes must be rejected");

    let err = res.unwrap_err();
    let expected_pattern = format!("/api/{}", seg119);
    // 1. Preserves original canonical pattern
    assert!(
        err.message.contains(&expected_pattern),
        "Error diagnostic must preserve the original route pattern: {}",
        err.message
    );
    // 2. Mentions source file
    assert!(
        err.first_source.is_some() || err.message.contains("route.va"),
        "Error diagnostic must include the source file: {}",
        err.message
    );
    // 3. Mentions actual encoded length (257)
    assert!(
        err.message.contains("257"),
        "Error diagnostic must report actual encoded length 257: {}",
        err.message
    );
    // 4. Mentions maximum permitted length (123 bytes / 255 bytes)
    assert!(
        err.message.contains("123"),
        "Error diagnostic must report maximum pattern length 123: {}",
        err.message
    );
    assert!(
        err.message.contains("255"),
        "Error diagnostic must report maximum filename length 255: {}",
        err.message
    );
}

#[test]
fn test_73_unicode_routes_measured_by_utf8_bytes_not_char_count() {
    // 'é' is 2 UTF-8 bytes (0xc3, 0xa9).
    // Pattern: "/api/" (5 bytes, 5 chars) + 60 'é' characters (120 bytes, 60 chars)
    // Total char count = 65 chars (well under 123 characters!).
    // Total UTF-8 byte length = 125 bytes (> 123 byte limit).
    // Filename length = 9 + 2 * 125 = 259 bytes (> 255 limit).
    let tmp = tempfile::tempdir().unwrap();
    let app_dir = tmp.path().join("app");
    let sixty_es: String = "é".repeat(60);
    assert_eq!(sixty_es.chars().count(), 60);
    assert_eq!(sixty_es.as_bytes().len(), 120);

    let route_dir = app_dir.join("api").join(&sixty_es);
    fs::create_dir_all(&route_dir).unwrap();
    let file_path = route_dir.join("route.va");
    fs::write(
        &file_path,
        "import std::string::String;\n\npub fn GET() -> String {\n    String::from_str(\"unicode\")\n}\n",
    )
    .unwrap();

    let res = scan_app_directory(&app_dir);
    assert!(
        res.is_err(),
        "Route with 65 characters but 125 UTF-8 bytes must be rejected"
    );

    let err = res.unwrap_err();
    assert!(
        err.message.contains("125 UTF-8 bytes"),
        "Error message must specify 125 UTF-8 bytes: {}",
        err.message
    );
    assert!(
        err.message.contains("259"),
        "Error message must report encoded filename length 259: {}",
        err.message
    );
    assert!(
        err.message.contains("123"),
        "Error message must report maximum limit 123: {}",
        err.message
    );
    assert!(
        err.message.contains("255"),
        "Error message must report maximum filename limit 255: {}",
        err.message
    );
}

#[test]
fn test_74_multibyte_unicode_at_boundary_handled_correctly() {
    // 1. Boundary 2-byte UTF-8 sequence: 'é' (2 bytes)
    // Pattern A: "/api/" (5 bytes) + 116 ASCII 'b' + 'é' (2 bytes) = 123 UTF-8 bytes (122 chars).
    // Filename: 9 + 2 * 123 = 255 bytes -> SUCCEEDS.
    {
        let tmp = tempfile::tempdir().unwrap();
        let app_dir = tmp.path().join("app");
        let seg = format!("{}é", "b".repeat(116));
        assert_eq!(seg.as_bytes().len(), 118);
        assert_eq!(seg.chars().count(), 117);

        let route_dir = app_dir.join("api").join(&seg);
        fs::create_dir_all(&route_dir).unwrap();
        fs::write(
            route_dir.join("route.va"),
            "import std::string::String;\n\npub fn GET() -> String {\n    String::from_str(\"boundary_2byte_ok\")\n}\n",
        )
        .unwrap();

        let discovered =
            scan_app_directory(&app_dir).expect("2-byte Unicode at 123 bytes must succeed");
        assert_eq!(discovered.routes[0].canonical_pattern.as_bytes().len(), 123);
        let mod_name = &discovered.routes[0].adapter_module_name;
        assert_eq!(format!("{}.va", mod_name).len(), 255);
    }

    // Pattern B: "/api/" (5 bytes) + 117 ASCII 'b' + 'é' (2 bytes) = 124 UTF-8 bytes (123 chars!).
    // Notice char count is exactly 123, but byte count is 124!
    // Filename: 9 + 2 * 124 = 257 bytes -> REJECTED.
    {
        let tmp = tempfile::tempdir().unwrap();
        let app_dir = tmp.path().join("app");
        let seg = format!("{}é", "b".repeat(117));
        assert_eq!(seg.as_bytes().len(), 119);
        assert_eq!(seg.chars().count(), 118);

        let route_dir = app_dir.join("api").join(&seg);
        fs::create_dir_all(&route_dir).unwrap();
        fs::write(
            route_dir.join("route.va"),
            "import std::string::String;\n\npub fn GET() -> String {\n    String::from_str(\"boundary_2byte_overflow\")\n}\n",
        )
        .unwrap();

        let res = scan_app_directory(&app_dir);
        assert!(res.is_err(), "2-byte Unicode at 124 bytes must be rejected");
        let err = res.unwrap_err();
        assert!(err.message.contains("124 UTF-8 bytes"));
        assert!(err.message.contains("257"));
    }

    // 2. Boundary 3-byte UTF-8 sequence: 'あ' (3 bytes: 0xe3, 0x81, 0x82)
    // Pattern C: "/api/" (5 bytes) + 115 ASCII 'c' + 'あ' (3 bytes) = 123 UTF-8 bytes (121 chars).
    // Filename: 9 + 2 * 123 = 255 bytes -> SUCCEEDS.
    {
        let tmp = tempfile::tempdir().unwrap();
        let app_dir = tmp.path().join("app");
        let seg = format!("{}あ", "c".repeat(115));
        assert_eq!(seg.as_bytes().len(), 118);

        let route_dir = app_dir.join("api").join(&seg);
        fs::create_dir_all(&route_dir).unwrap();
        fs::write(
            route_dir.join("route.va"),
            "import std::string::String;\n\npub fn GET() -> String {\n    String::from_str(\"boundary_3byte_ok\")\n}\n",
        )
        .unwrap();

        let discovered =
            scan_app_directory(&app_dir).expect("3-byte Unicode at 123 bytes must succeed");
        assert_eq!(discovered.routes[0].canonical_pattern.as_bytes().len(), 123);
        let mod_name = &discovered.routes[0].adapter_module_name;
        assert_eq!(format!("{}.va", mod_name).len(), 255);
    }

    // Pattern D: "/api/" (5 bytes) + 116 ASCII 'c' + 'あ' (3 bytes) = 124 UTF-8 bytes (122 chars).
    // Filename: 9 + 2 * 124 = 257 bytes -> REJECTED.
    {
        let tmp = tempfile::tempdir().unwrap();
        let app_dir = tmp.path().join("app");
        let seg = format!("{}あ", "c".repeat(116));
        assert_eq!(seg.as_bytes().len(), 119);

        let route_dir = app_dir.join("api").join(&seg);
        fs::create_dir_all(&route_dir).unwrap();
        fs::write(
            route_dir.join("route.va"),
            "import std::string::String;\n\npub fn GET() -> String {\n    String::from_str(\"boundary_3byte_overflow\")\n}\n",
        )
        .unwrap();

        let res = scan_app_directory(&app_dir);
        assert!(res.is_err(), "3-byte Unicode at 124 bytes must be rejected");
        let err = res.unwrap_err();
        assert!(err.message.contains("124 UTF-8 bytes"));
        assert!(err.message.contains("257"));
    }
}

#[test]
fn test_75_rejected_oversized_routes_do_not_create_or_overwrite_adapter_files() {
    let tmp = tempfile::tempdir().unwrap();
    let app_dir = tmp.path().join("app");
    let out_va = tmp.path().join("generated_routes.va");
    let adapter_dir = tmp.path().join("generated_routes");
    fs::create_dir_all(&adapter_dir).unwrap();

    // 1. Seed an existing adapter file with pre-existing content
    let existing_adapter = adapter_dir.join("route_existing.va");
    let seed_content = "// PRE_EXISTING_ADAPTER_DO_NOT_TOUCH";
    fs::write(&existing_adapter, seed_content).unwrap();

    // 2. Create an oversized route in app_dir
    let seg120 = "x".repeat(120); // /api/ + 120 = 125 bytes -> 259 filename chars
    let route_dir = app_dir.join("api").join(&seg120);
    fs::create_dir_all(&route_dir).unwrap();
    fs::write(
        route_dir.join("route.va"),
        "import std::string::String;\n\npub fn GET() -> String {\n    String::from_str(\"bad\")\n}\n",
    )
    .unwrap();

    // 3. Scan fails during discovery
    let scan_res = scan_app_directory(&app_dir);
    assert!(scan_res.is_err());

    // 4. Verify existing file untouched and no new file created
    assert_eq!(fs::read_to_string(&existing_adapter).unwrap(), seed_content);
    let entries: Vec<_> = fs::read_dir(&adapter_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(entries, vec!["route_existing.va"]);

    // 5. Direct write_generated_routes call with an oversized route also fails closed
    let oversized_route = RouteMetadata {
        id: 0,
        canonical_pattern: format!("/api/{}", seg120),
        normalized_url: format!("/api/{}", seg120),
        route_kind: RouteKind::Api,
        render_mode: RenderMode::Dynamic,
        revalidate_seconds: None,
        source_file: "oversized/route.va".to_string(),
        segments: vec![Segment {
            raw: "api".to_string(),
            kind: SegmentKind::Static,
            param_name: None,
        }],
        layout_chain: vec![],
        methods: vec!["GET".to_string()],
        specificity: 1000,
        has_generic_handler: false,
        explicit_methods: vec!["GET".to_string()],
        adapter_module_name: sanitize_module_name(&format!("/api/{}", seg120)),
        source_content: "pub fn GET() -> String { String::from_str(\"oversized\") }".to_string(),
    };

    let fake_app = DiscoveredApp {
        routes: vec![oversized_route],
        special_files: SpecialFilesMetadata {
            root_layout: None,
            loading: None,
            error: None,
            not_found: None,
        },
    };

    let write_res = write_generated_routes(&fake_app, &out_va);
    assert!(
        write_res.is_err(),
        "write_generated_routes must reject oversized routes before writing files"
    );

    // Existing file still untouched, and no other files written
    assert_eq!(fs::read_to_string(&existing_adapter).unwrap(), seed_content);
    let entries_after: Vec<_> = fs::read_dir(&adapter_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(entries_after, vec!["route_existing.va"]);
}

#[test]
fn test_76_existing_users_and_health_remain_unaffected() {
    // /api/users is 10 UTF-8 bytes
    let users_pattern = "/api/users";
    assert_eq!(users_pattern.as_bytes().len(), 10);
    let users_filename_len = adapter_filename_len_for_bytes(users_pattern.as_bytes().len());
    assert_eq!(users_filename_len, 29);
    assert!(users_filename_len <= MAX_PORTABLE_ADAPTER_FILENAME_LEN);

    // /api/health is 11 UTF-8 bytes
    let health_pattern = "/api/health";
    assert_eq!(health_pattern.as_bytes().len(), 11);
    let health_filename_len = adapter_filename_len_for_bytes(health_pattern.as_bytes().len());
    assert_eq!(health_filename_len, 31);
    assert!(health_filename_len <= MAX_PORTABLE_ADAPTER_FILENAME_LEN);

    let app_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("app");
    if app_dir.exists() {
        let app = scan_app_directory(&app_dir).unwrap();
        let users = app
            .routes
            .iter()
            .find(|r| r.canonical_pattern == "/api/users")
            .unwrap();
        let health = app
            .routes
            .iter()
            .find(|r| r.canonical_pattern == "/api/health")
            .unwrap();

        assert_eq!(users.adapter_module_name, "route_2f6170692f7573657273");
        assert_eq!(health.adapter_module_name, "route_2f6170692f6865616c7468");

        let tmp = tempfile::tempdir().unwrap();
        let out_va = tmp.path().join("generated_routes.va");
        write_generated_routes(&app, &out_va).unwrap();

        let adapter_dir = tmp.path().join("generated_routes");
        let users_file = adapter_dir.join("route_2f6170692f7573657273.va");
        let health_file = adapter_dir.join("route_2f6170692f6865616c7468.va");

        assert!(users_file.exists());
        assert!(health_file.exists());

        let users_content = fs::read_to_string(&users_file).unwrap();
        assert!(users_content.contains("pub fn invoke_get() -> String"));
        assert!(users_content.contains("pub fn invoke_head() -> String"));

        let health_content = fs::read_to_string(&health_file).unwrap();
        assert!(health_content.contains("pub fn invoke_handle_api() -> String"));
    }
}

#[test]
fn test_77_collision_resistant_module_identifiers_remain_deterministic() {
    // Determinism checks across repeated invocations
    let patterns = vec![
        "/api/users",
        "/api-users",
        "/api_users",
        "/api/health",
        "/api/posts/[id]",
        "/api/docs/[...slug]",
        "/api/shop/[[...filter]]",
    ];

    for pattern in patterns {
        let id1 = sanitize_module_name(pattern);
        let id2 = sanitize_module_name(pattern);
        assert_eq!(
            id1, id2,
            "Identifier must be deterministic for '{}'",
            pattern
        );
        assert!(id1.starts_with("route_"));

        let expected_filename_len = ADAPTER_FILENAME_WRAPPER_LEN + (pattern.as_bytes().len() * 2);
        assert_eq!(
            adapter_filename_len_for_bytes(pattern.as_bytes().len()),
            expected_filename_len
        );
        let filename = format!("{}.va", id1);
        assert_eq!(filename.len(), expected_filename_len);
    }

    // Boundary constant mathematical invariant
    assert_eq!(
        MAX_PORTABLE_ADAPTER_FILENAME_LEN,
        ADAPTER_FILENAME_WRAPPER_LEN + (MAX_PORTABLE_ROUTE_PATTERN_BYTES * 2)
    );
    assert_eq!(MAX_PORTABLE_ROUTE_PATTERN_BYTES, 123);
    assert_eq!(MAX_PORTABLE_ADAPTER_FILENAME_LEN, 255);
    assert_eq!(ADAPTER_FILENAME_WRAPPER_LEN, 9);
}
