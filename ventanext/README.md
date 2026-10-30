# ⚡ VentaNext Framework

**A Next.js-Class Full-Stack Web Framework for the Venta Language — Written 100% in Pure Venta**

---

## 🚀 Overview

**VentaNext** brings modern full-stack web application architecture — declarative filesystem-modeled App Routing, deterministic route manifests, hierarchical nested layouts, dynamic parameter extraction (`[param]`), catch-all routing (`[...path]`), optional catch-all routing (`[[...filter]]`), route groups (`(group)`), conflict validation, typed API routes, cookie & query parsing, and composable middleware — to the **Venta** programming language.

Unlike JavaScript/TypeScript frameworks running on V8/Node.js or Python frameworks with interpreter overhead, VentaNext executes 100% in pure Venta and compiles directly to a **standalone bare-metal native executable** with **sub-second compilation**, zero garbage collection pauses, and **100% guaranteed memory safety**.

---

## 🏛 Phase 1 Architecture: Framework Foundation

VentaNext Phase 1 delivers the full foundational web engine:

### 1. Deterministic App Router Manifest
- **Static Routes**: Exact path matching (`/`, `/about`, `/dashboard`).
- **Dynamic Parameter Routes (`[param]`)**: Captures URL segments into `req.params` (e.g. `/posts/[id]` captures `req.params.get("id")`).
- **Catch-All Routes (`[...path]`)**: Captures one or more trailing segments (e.g. `/docs/[...path]` with `/docs/a/b` captures `path = "a/b"`).
- **Optional Catch-All Routes (`[[...filter]]`)**: Matches both base path and nested paths (`/shop` and `/shop/shoes/nike`).
- **Route Groups (`(group)`)**: Omitted from canonical URL paths while establishing layout boundaries (e.g. `/(marketing)/pricing` matches `/pricing`).
- **Specificity Precedence**: Evaluated deterministically:
  $$\text{Static} > \text{Dynamic } [param] > \text{Catch-all } [...param] > \text{Optional Catch-all } [[...param]]$$
- **Conflict Detection**: Detects duplicate static routes and ambiguous parameter collisions at manifest validation time.

### 2. Hierarchical Nested Layout Resolution
Layouts are composed hierarchically outside-in:
```text
Request: /dashboard
Layout Chain: RootLayout > DashboardLayout > PageComponent

Step 1: page_html        = render_dashboard_page(&ctx)
Step 2: dashboard_layout = render_dashboard_layout(&ctx, page_html)
Step 3: root_layout      = render_root_layout(&ctx, dashboard_layout)
```

### 3. HTTP Lifecycle & Data Models
- **`Request`**:
  - `method`, `path`, `raw_query`, `body`
  - `headers`: Case-insensitive header dictionary with `get(key)` and `has(key)`.
  - `query`: Parsed query string dictionary (`?key=val&sort=desc`).
  - `params`: Captured dynamic route parameters.
  - `cookies`: Parsed `Cookie` header dictionary.
- **`Response`**:
  - `status`, `content_type`, `headers`, `body`
  - Factory helpers: `response_html`, `response_json`, `response_text`, `response_redirect`, `response_not_found`, `response_error`.
  - `set_cookie`: Configures `Set-Cookie` with `Max-Age`, `Path`, `Secure`, `HttpOnly`, and `SameSite` flags.
  - `to_wire_format`: Serializes full HTTP/1.1 response frames.

---

## 📁 Directory Structure

```text
ventanext/
├── mod.va                 # Core module declarations
├── http.va                # Request/Response models, headers, query, cookies, wire format
├── router.va              # App Router manifest, segment parser, specificity, conflict check
├── ssr.va                 # Component interface, nested layout resolution, showcase pages
├── middleware.va          # Security headers, CORS headers, OPTIONS preflight, request logger
├── server.va              # Server lifecycle, connection loop, manifest request dispatch
├── main.va                # Application entry point CLI
├── test_framework.va      # 10-suite automated unit & integration test runner
└── README.md              # Framework documentation
```

---

## 💻 Running & Testing

### 1. Run the Framework Test Suite
```bash
# Runs all 10 automated test suites:
venta run ventanext/test_framework.va
```

### 2. Start the Server
```bash
# Starts development server on http://127.0.0.1:3000
venta run ventanext/main.va

# Custom port and headless single-request mode for CI/testing:
venta run ventanext/main.va -- --port 8080 --once
```

### 3. Verify Endpoints
```bash
# Health JSON API:
curl http://127.0.0.1:3000/api/status

# Dynamic Post Detail (SSR):
curl http://127.0.0.1:3000/posts/42

# Echo API:
curl -X POST http://127.0.0.1:3000/api/echo -d '{"hello":"venta"}'
```

---

## 🧪 Verified Phase 1 Test Coverage

The test suite in [`ventanext/test_framework.va`](file:///c:/Users/mumer/OneDrive/Documents/Venta/ventanext/test_framework.va) validates:
1. `test_route_manifest_registration_and_validation` (Static, Dynamic, Catch-All, Optional Catch-All, API routes).
2. `test_route_conflict_detection` (Catches ambiguous collisions like `/blog/[id]` vs `/blog/[slug]` and duplicate paths).
3. `test_specificity_precedence` (Static `/posts/new` reliably beats dynamic `/posts/[slug]`).
4. `test_dynamic_param_extraction` (Extracts parameterized segments into `req.params`).
5. `test_catch_all_param_extraction` (Multi-segment capture into `[...path]`).
6. `test_optional_catch_all_param_extraction` (Handles both 0 segments and multiple segments).
7. `test_route_group_normalization` (`/(marketing)/pricing` matches `/pricing`).
8. `test_nested_layout_resolution` (Root > Dashboard > Page nested HTML composition).
9. `test_http_request_parsing_lifecycle` (Headers, query params, cookies, body).
10. `test_http_response_and_wire_formatting` (Status lines, cookies, redirects, wire serialization).
