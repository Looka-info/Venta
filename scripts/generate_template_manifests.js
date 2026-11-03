const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const ROOT_DIR = path.resolve(__dirname, '..');
const TEMPLATES_DIR = path.join(ROOT_DIR, 'templates');

const templates = [
  {
    id: 'modern-saas',
    name: 'Modern SaaS Website',
    category: 'SaaS landing pages',
    description: 'Lumen SaaS revenue analytics platform with live MRR metrics track, three-tier pricing cards, interactive FAQ accordion, and clean modern aesthetic.',
    version: '1.0.0',
    framework_compatibility: '>=1.0.0-rc.2',
    entry_point: 'main.va',
    features: [
      'App Router Manifest',
      'Live MRR Metric Track',
      'Three-Tier Pricing Cards',
      'Interactive FAQ Accordion',
      'Health & Stats JSON API',
      'Zero-GC Native Telemetry'
    ],
    pages: [
      { path: '/', title: 'Overview', description: 'Hero headline, live revenue track, pricing tiers, and FAQ accordion' },
      { path: '/features', title: 'Features', description: 'Platform capabilities, zero-config webhooks, and cohort retention' },
      { path: '/pricing', title: 'Pricing', description: 'Starter $0, Growth $149, and Scale $499 comparison' },
      { path: '/changelog', title: 'Changelog', description: 'Release history and performance milestone notes' },
      { path: '/api/health', title: 'Health API', description: 'JSON status endpoint' },
      { path: '/api/stats', title: 'Stats API', description: 'Live cluster throughput metrics' }
    ]
  },
  {
    id: 'developer-portfolio',
    name: 'Developer Portfolio',
    category: 'Developer portfolios',
    description: 'Maya Okafor systems software engineer portfolio with horizontal project track (Quill, Mosaic, Ledgerline, Tidepool), about section, and career experience timeline.',
    version: '1.0.0',
    framework_compatibility: '>=1.0.0-rc.2',
    entry_point: 'main.va',
    features: [
      'Georgia Typographic Identity',
      'Horizontal Project Track',
      'Open-Source Star Metrics',
      'Experience Career Timeline',
      'Technical Writing Blog',
      'Contact GPG & Inquiry API'
    ],
    pages: [
      { path: '/', title: 'Home', description: 'Hero bio, project showcase track, about section, and experience timeline' },
      { path: '/projects', title: 'Projects', description: 'Selected open-source libraries and compiler systems' },
      { path: '/blog', title: 'Writing', description: 'Technical articles on systems programming and affine types' },
      { path: '/about', title: 'About', description: 'Engineering background and philosophy' },
      { path: '/api/contact', title: 'Contact API', description: 'GPG fingerprint and contact endpoint' }
    ]
  },
  {
    id: 'agency-business',
    name: 'Business & Agency Website',
    category: 'Agency websites',
    description: 'Halftone Studio independent brand and digital agency in Berlin and Lagos with horizontal showcase track, revenue case study (+212%), and creative team grid.',
    version: '1.0.0',
    framework_compatibility: '>=1.0.0-rc.2',
    entry_point: 'main.va',
    features: [
      'Difference-Blend Navigation',
      'Horizontal Client Track',
      'Animated Impact Metric (+212%)',
      'Creative Team Profile Grid',
      'Client Testimonial Callouts',
      'Studio Project Quote API'
    ],
    pages: [
      { path: '/', title: 'Agency', description: 'Global studio intro, showcase track, revenue case study, and team' },
      { path: '/services', title: 'Capabilities', description: 'Brand architecture, digital flagships, motion, and packaging' },
      { path: '/work', title: 'Case Studies', description: 'Alpine Rail and Plume Bank impact highlights' },
      { path: '/contact', title: 'Contact', description: 'Collaboration inquiry and Berlin/Lagos studio locations' },
      { path: '/api/quote', title: 'Quote API', description: 'Studio engagement parameters and availability' }
    ]
  },
  {
    id: 'ecommerce-storefront',
    name: 'E-commerce Storefront',
    category: 'E-commerce storefronts',
    description: 'Fieldnote Goods everyday stoneware ceramics and washed linen store with interactive shopping bag counter, horizontal category track, and bestseller product cards.',
    version: '1.0.0',
    framework_compatibility: '>=1.0.0-rc.2',
    entry_point: 'main.va',
    features: [
      'Free Shipping Announcement Bar',
      'Horizontal Ceramic Collection Track',
      'Inline SVG Product Illustrations',
      'Interactive Bag (N) Counter',
      'Full Shop Catalog Grid',
      'Shopping Cart Simulator'
    ],
    pages: [
      { path: '/', title: 'Home', description: 'Artisanal stoneware hero, collection track, and bestseller cards' },
      { path: '/shop', title: 'Catalog', description: 'Complete collection of ceramics, linen, and kitchenware' },
      { path: '/cart', title: 'Bag', description: 'Active shopping bag summary with free shipping threshold' },
      { path: '/api/products', title: 'Products API', description: 'Live product catalog endpoint' },
      { path: '/api/cart', title: 'Cart API', description: 'Current session cart totals endpoint' }
    ]
  },
  {
    id: 'admin-dashboard',
    name: 'Admin Dashboard',
    category: 'Admin dashboards',
    description: 'Ledgerly Admin financial dashboard with real-time revenue counters ($128,430), horizontal metric charts, recent orders table, and account preferences.',
    version: '1.0.0',
    framework_compatibility: '>=1.0.0-rc.2',
    entry_point: 'main.va',
    features: [
      'Top Navigation with User Avatar',
      'Live Revenue Counter Animation',
      'Horizontal Metric Vector Charts',
      'Recent Orders Status Table',
      'Toggle Preference Switches',
      'Financial Telemetry JSON API'
    ],
    pages: [
      { path: '/', title: 'Overview', description: 'Revenue counter, KPI pills, metric vector charts, and orders' },
      { path: '/analytics', title: 'Analytics', description: 'Throughput metrics and latency percentile SLAs' },
      { path: '/users', title: 'Customers', description: 'Subscriber accounts and billing status table' },
      { path: '/settings', title: 'Preferences', description: 'Notification settings and security toggles' },
      { path: '/api/metrics', title: 'Metrics API', description: 'Revenue and conversion telemetry endpoint' },
      { path: '/api/users', title: 'Users API', description: 'Customer accounts list endpoint' }
    ]
  },
  {
    id: 'docs-website',
    name: 'Documentation Website',
    category: 'Documentation websites',
    description: 'Kiln technical documentation platform with instant client-side search filtering, syntax-highlighted code blocks, horizontal step track, and options reference.',
    version: '1.0.0',
    framework_compatibility: '>=1.0.0-rc.2',
    entry_point: 'main.va',
    features: [
      'Instant Search Filter Input',
      'Horizontal Step-by-Step Track',
      'Syntax-Highlighted Code Blocks',
      'Config Options Reference Table',
      'Admonition Tip Callout',
      'Fast Incremental Build Pipeline'
    ],
    pages: [
      { path: '/', title: 'Quickstart', description: 'Docs hero, search input, 4-step quickstart track, and options table' },
      { path: '/guide/architecture', title: 'Architecture', description: 'AST pipeline and 14-pass SSA optimizer details' },
      { path: '/guide/components', title: 'Components', description: 'Native Venta SSR component composition' },
      { path: '/reference/api', title: 'API Reference', description: 'Kiln configuration schema and build flags' },
      { path: '/api/search', title: 'Search API', description: 'Documentation query endpoint' }
    ]
  }
];

function collectFiles(dir, baseDir = dir) {
  let results = [];
  const list = fs.readdirSync(dir);
  for (const file of list) {
    const fullPath = path.join(dir, file);
    const stat = fs.statSync(fullPath);
    if (stat.isDirectory()) {
      results = results.concat(collectFiles(fullPath, baseDir));
    } else {
      const relPath = path.relative(baseDir, fullPath).replace(/\\/g, '/');
      const content = fs.readFileSync(fullPath);
      const hash = crypto.createHash('sha256').update(content).digest('hex');
      results.push({
        path: relPath,
        size_bytes: stat.size,
        sha256: hash
      });
    }
  }
  return results;
}

const registry = {
  version: '1.0.0',
  schema_version: '1.0.0',
  last_updated: '2026-11-04T00:00:00Z',
  categories: [
    'Business websites',
    'SaaS landing pages',
    'Developer portfolios',
    'E-commerce storefronts',
    'Admin dashboards',
    'Blogs and publishing',
    'Documentation websites',
    'Agency websites',
    'Startup landing pages',
    'Personal websites'
  ],
  templates: []
};

for (const t of templates) {
  const projDir = path.join(TEMPLATES_DIR, t.id, 'project');
  const inventory = collectFiles(projDir);
  
  const manifest = {
    id: t.id,
    name: t.name,
    category: t.category,
    description: t.description,
    version: t.version,
    framework_compatibility: t.framework_compatibility,
    entry_point: t.entry_point,
    preview_asset: 'preview.svg',
    download_source: `local://templates/${t.id}`,
    features: t.features,
    pages: t.pages,
    dependencies: {
      vanta: '>=1.0.0-rc.2',
      ventanext: '>=1.0.0-rc.2'
    },
    file_inventory: inventory
  };

  const manifestPath = path.join(TEMPLATES_DIR, t.id, 'template.json');
  fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n', 'utf-8');

  registry.templates.push({
    id: t.id,
    name: t.name,
    category: t.category,
    description: t.description,
    version: t.version,
    framework_compatibility: t.framework_compatibility,
    preview_asset: `templates/${t.id}/preview.svg`,
    features: t.features,
    page_count: t.pages.length,
    file_count: inventory.length
  });

  console.log(`[Manifest] Generated ${t.id}/template.json with ${inventory.length} verified files.`);
}

const registryPath = path.join(TEMPLATES_DIR, 'registry.json');
fs.writeFileSync(registryPath, JSON.stringify(registry, null, 2) + '\n', 'utf-8');
console.log(`[Registry] Generated templates/registry.json successfully with ${registry.templates.length} templates.`);
