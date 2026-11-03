# 🚀 Modern SaaS Website Template for VentaNext

A high-converting, performance-first SaaS marketing and product application built **100% in pure Venta** with VentaNext App Router.

## ✨ Features
- **Deterministic App Routing**: Static pages (`/`, `/features`, `/pricing`, `/changelog`) and typed JSON API endpoints (`/api/health`, `/api/stats`).
- **Hierarchical Layouts**: RootLayout composing header navigation, ambient canvas glow, and footer across all pages.
- **Design Tokens**: Centralized color variables (`--gold-core`, `--bg-void`), typography, and spacing tokens in `app/components/tokens.va`.
- **Reusable Components**: Hero banner, bento feature grid, interactive billing tiers, and live changelog updates.
- **Sub-Second Compilation**: Compiles directly to native bare-metal AMD64 machine code with zero garbage collection pauses.

## 🛠 Project Structure
```text
.
├── ventanext.config.json    # Application configuration
├── package.json             # Package metadata and scripts
├── main.va                  # Standalone application entry point
└── app/
    ├── layout.va            # Global root layout with navigation and footer
    ├── page.va              # Home page with hero, bento grid, and metrics
    ├── features/page.va     # Detailed feature comparison and architectural highlights
    ├── pricing/page.va      # Transparent pricing tiers with billing frequency toggle
    ├── changelog/page.va    # Product release timeline and milestone notes
    ├── api/health/route.va  # JSON health probe endpoint
    ├── api/stats/route.va   # Live system statistics endpoint
    └── components/
        └── tokens.va        # CSS design token constants
```

## 🚀 Getting Started
```bash
# Start local development server
vanta run main.va --port 3000

# Compile to standalone binary
vantac main.va -o saas-app && ./saas-app
```

## 🎨 Customizing Styles & Content
1. **Colors & Design Tokens**: Edit `app/components/tokens.va` to modify `--gold-core`, `--bg-void`, and typography.
2. **Hero Copy & Value Proposition**: Modify `app/page.va` in the `render_page()` function.
3. **Pricing Plans**: Add or adjust tiers in `app/pricing/page.va`.
