const fs = require('fs');
const path = require('path');

const ROOT_DIR = path.resolve(__dirname, '..');
const TEMPLATES_DIR = path.join(ROOT_DIR, 'templates');

// 1. Ledgerly Admin preview.svg
const adminSvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 500" width="800" height="500">
  <rect width="800" height="500" rx="16" fill="#f1f2f5"/>
  <!-- Top Nav -->
  <rect x="0" y="0" width="800" height="56" fill="#ffffff" fill-opacity="0.9" stroke="#d9dde4" stroke-width="1"/>
  <text x="36" y="36" fill="#0f172a" font-family="Inter, system-ui, sans-serif" font-weight="800" font-size="18" letter-spacing="-0.04em">Ledgerly</text>
  <rect x="140" y="16" width="76" height="26" rx="6" fill="#ffffff" stroke="#d9dde4" stroke-width="1"/>
  <text x="152" y="33" fill="#0f172a" font-family="Inter, system-ui, sans-serif" font-weight="600" font-size="12">Overview</text>
  <text x="235" y="33" fill="#64748b" font-family="Inter, system-ui, sans-serif" font-size="12">Analytics</text>
  <text x="310" y="33" fill="#64748b" font-family="Inter, system-ui, sans-serif" font-size="12">Customers</text>
  <text x="390" y="33" fill="#64748b" font-family="Inter, system-ui, sans-serif" font-size="12">Settings</text>
  <circle cx="750" cy="28" r="16" fill="#0f172a"/>
  <text x="742" y="32" fill="#ffffff" font-family="Inter, system-ui, sans-serif" font-weight="bold" font-size="10">AM</text>

  <!-- Hero Headline -->
  <text x="36" y="94" fill="#64748b" font-family="Inter, system-ui, sans-serif" font-size="13">Revenue, last 30 days</text>
  <text x="36" y="152" fill="#0f172a" font-family="Inter, system-ui, sans-serif" font-weight="900" font-size="52" letter-spacing="-0.06em">$128,430</text>
  
  <!-- KPI Pills -->
  <text x="36" y="190" fill="#0f172a" font-family="Inter, system-ui, sans-serif" font-weight="700" font-size="16">3,284</text>
  <text x="36" y="206" fill="#64748b" font-family="Inter, system-ui, sans-serif" font-size="11">Active customers</text>

  <text x="170" y="190" fill="#0f172a" font-family="Inter, system-ui, sans-serif" font-weight="700" font-size="16">4.6%</text>
  <text x="170" y="206" fill="#64748b" font-family="Inter, system-ui, sans-serif" font-size="11">Conversion</text>

  <text x="270" y="190" fill="#047857" font-family="Inter, system-ui, sans-serif" font-weight="700" font-size="16">+12.4%</text>
  <text x="270" y="206" fill="#64748b" font-family="Inter, system-ui, sans-serif" font-size="11">vs previous</text>

  <!-- Metric Graph Cards -->
  <rect x="36" y="234" width="225" height="110" rx="12" fill="#ffffff" stroke="#d9dde4" stroke-width="1"/>
  <path d="M50 310 C80 290, 110 300, 140 270 S 210 260, 240 250" fill="none" stroke="#2563eb" stroke-width="3"/>
  <text x="50" y="260" fill="#0f172a" font-family="Inter, system-ui, sans-serif" font-weight="800" font-size="20">$128k</text>

  <rect x="281" y="234" width="225" height="110" rx="12" fill="#ffffff" stroke="#d9dde4" stroke-width="1"/>
  <path d="M295 310 C325 300, 360 280, 400 270 S 460 250, 485 245" fill="none" stroke="#2563eb" stroke-width="3"/>
  <text x="295" y="260" fill="#0f172a" font-family="Inter, system-ui, sans-serif" font-weight="800" font-size="20">3,284</text>

  <rect x="526" y="234" width="238" height="110" rx="12" fill="#ffffff" stroke="#d9dde4" stroke-width="1"/>
  <path d="M540 270 C570 280, 610 260, 650 290 S 710 300, 745 320" fill="none" stroke="#b91c1c" stroke-width="3"/>
  <text x="540" y="260" fill="#0f172a" font-family="Inter, system-ui, sans-serif" font-weight="800" font-size="20">$2.1k</text>

  <!-- Orders Table Strip -->
  <rect x="36" y="364" width="728" height="106" rx="12" fill="#ffffff" stroke="#d9dde4" stroke-width="1"/>
  <text x="56" y="394" fill="#64748b" font-family="Inter, system-ui, sans-serif" font-weight="600" font-size="11">RECENT ORDERS</text>
  <text x="56" y="420" fill="#0f172a" font-family="Inter, system-ui, sans-serif" font-size="13">#10482 · Northwind Ltd · $1,240</text>
  <rect x="680" y="408" width="54" height="20" rx="10" fill="#d1fae5"/>
  <text x="694" y="422" fill="#065f46" font-family="Inter, system-ui, sans-serif" font-weight="bold" font-size="10">Paid</text>
  <text x="56" y="450" fill="#0f172a" font-family="Inter, system-ui, sans-serif" font-size="13">#10481 · Tessera Inc · $860</text>
  <rect x="680" y="438" width="54" height="20" rx="10" fill="#fef3c7"/>
  <text x="688" y="452" fill="#92400e" font-family="Inter, system-ui, sans-serif" font-weight="bold" font-size="10">Pending</text>
</svg>`;
fs.writeFileSync(path.join(TEMPLATES_DIR, 'admin-dashboard', 'preview.svg'), adminSvg);

// 2. Kiln Docs preview.svg
const docsSvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 500" width="800" height="500">
  <rect width="800" height="500" rx="16" fill="#101214"/>
  <!-- Nav -->
  <rect x="0" y="0" width="800" height="56" fill="#16191c" stroke="#262a2e" stroke-width="1"/>
  <text x="36" y="36" fill="#eceae4" font-family="'Segoe UI', sans-serif" font-weight="800" font-size="18" letter-spacing="-0.04em">Kiln</text>
  <text x="110" y="34" fill="#8d9298" font-family="'Segoe UI', sans-serif" font-size="13">Guides</text>
  <text x="180" y="34" fill="#8d9298" font-family="'Segoe UI', sans-serif" font-size="13">Reference</text>
  <text x="270" y="34" fill="#8d9298" font-family="'Segoe UI', sans-serif" font-size="13">Plugins</text>
  <rect x="710" y="16" width="54" height="26" rx="99" fill="#7ee0b5"/>
  <text x="723" y="33" fill="#0a1511" font-family="'Segoe UI', sans-serif" font-weight="bold" font-size="12">v2.4</text>

  <!-- Hero Headline -->
  <text x="36" y="120" fill="#eceae4" font-family="'Segoe UI', sans-serif" font-weight="900" font-size="44" letter-spacing="-0.05em">Docs for builders.</text>
  <text x="36" y="150" fill="#8d9298" font-family="'Segoe UI', sans-serif" font-size="14">Kiln bundles your project in milliseconds. From zero to production in five minutes.</text>

  <!-- Search Bar -->
  <rect x="36" y="174" width="440" height="42" rx="10" fill="#16191c" stroke="#33383d" stroke-width="1"/>
  <text x="56" y="200" fill="#8d9298" font-family="'Segoe UI', sans-serif" font-size="14">Search the docs...</text>
  <rect x="420" y="184" width="44" height="22" rx="4" fill="#262a2e"/>
  <text x="426" y="199" fill="#eceae4" font-family="monospace" font-size="10">Ctrl K</text>

  <!-- Code Blocks (Steps) -->
  <rect x="36" y="238" width="350" height="220" rx="12" fill="#0a0c0e" stroke="#262a2e" stroke-width="1"/>
  <text x="56" y="268" fill="#7ee0b5" font-family="monospace" font-weight="bold" font-size="12">STEP 1: INSTALL</text>
  <text x="56" y="300" fill="#89b4fa" font-family="monospace" font-size="14">npm <tspan fill="#eceae4">install --save-dev kiln</tspan></text>
  <text x="56" y="340" fill="#8d9298" font-family="'Segoe UI', sans-serif" font-size="12">Installs native Venta toolchain and runtime</text>
  <text x="56" y="360" fill="#8d9298" font-family="'Segoe UI', sans-serif" font-size="12">with deterministic cryptographic caching.</text>

  <rect x="410" y="238" width="354" height="220" rx="12" fill="#0a0c0e" stroke="#262a2e" stroke-width="1"/>
  <text x="430" y="268" fill="#7ee0b5" font-family="monospace" font-weight="bold" font-size="12">STEP 2: CONFIGURE</text>
  <text x="430" y="300" fill="#f2a1c4" font-family="monospace" font-size="13">export default <tspan fill="#eceae4">{</tspan></text>
  <text x="446" y="324" fill="#89b4fa" font-family="monospace" font-size="13">entry<tspan fill="#eceae4">: </tspan><tspan fill="#a6e3a1">"src/index.js"</tspan><tspan fill="#eceae4">,</tspan></text>
  <text x="446" y="348" fill="#89b4fa" font-family="monospace" font-size="13">outDir<tspan fill="#eceae4">: </tspan><tspan fill="#a6e3a1">"dist"</tspan><tspan fill="#eceae4">,</tspan></text>
  <text x="446" y="372" fill="#89b4fa" font-family="monospace" font-size="13">minify<tspan fill="#eceae4">: </tspan><tspan fill="#f9c27b">true</tspan></text>
  <text x="430" y="396" fill="#eceae4" font-family="monospace" font-size="13">};</text>
</svg>`;
fs.writeFileSync(path.join(TEMPLATES_DIR, 'docs-website', 'preview.svg'), docsSvg);

// 3. Fieldnote Goods preview.svg
const ecomSvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 500" width="800" height="500">
  <rect width="800" height="500" rx="16" fill="#f4efe6"/>
  <!-- Top Announcement Bar -->
  <rect x="0" y="0" width="800" height="30" fill="#1f3d2b"/>
  <text x="400" y="20" fill="#f1ead9" font-family="'Avenir Next', sans-serif" font-size="11" text-anchor="middle">Free shipping on orders over $60. Made in small batches in Portugal.</text>

  <!-- Nav -->
  <text x="36" y="66" fill="#231f1a" font-family="Georgia, serif" font-weight="600" font-size="22">Fieldnote</text>
  <text x="170" y="64" fill="#6b645a" font-family="'Avenir Next', sans-serif" font-size="13">Dinnerware</text>
  <text x="260" y="64" fill="#6b645a" font-family="'Avenir Next', sans-serif" font-size="13">Linen</text>
  <text x="320" y="64" fill="#6b645a" font-family="'Avenir Next', sans-serif" font-size="13">Kitchen</text>
  <text x="390" y="64" fill="#6b645a" font-family="'Avenir Next', sans-serif" font-size="13">Gifts</text>
  <rect x="690" y="46" width="74" height="28" rx="99" fill="#231f1a"/>
  <text x="708" y="64" fill="#f4efe6" font-family="'Avenir Next', sans-serif" font-weight="600" font-size="12">Bag (0)</text>

  <!-- Hero Headline -->
  <text x="36" y="130" fill="#231f1a" font-family="Georgia, serif" font-weight="400" font-size="52" letter-spacing="-0.04em">Made to be used.</text>
  <text x="36" y="160" fill="#6b645a" font-family="'Avenir Next', sans-serif" font-size="15">Hand-glazed stoneware and washed linen in soft, lived-in colors.</text>

  <!-- Product Cards -->
  <rect x="36" y="196" width="166" height="260" rx="12" fill="#f4efe6" stroke="#ddd4c2" stroke-width="1"/>
  <rect x="46" y="206" width="146" height="140" rx="8" fill="#e9dfcb"/>
  <path d="M75 250 h88 l-8 60 a36 10 0 0 1-72 0 z" fill="#f8f3e9"/>
  <text x="46" y="366" fill="#231f1a" font-family="'Avenir Next', sans-serif" font-weight="600" font-size="13">Stoneware Bowl</text>
  <text x="46" y="384" fill="#6b645a" font-family="'Avenir Next', sans-serif" font-size="11">Set of 2</text>
  <text x="46" y="416" fill="#231f1a" font-family="'Avenir Next', sans-serif" font-weight="bold" font-size="14">$48</text>

  <rect x="222" y="196" width="166" height="260" rx="12" fill="#f4efe6" stroke="#ddd4c2" stroke-width="1"/>
  <rect x="232" y="206" width="146" height="140" rx="8" fill="#e9dfcb"/>
  <path d="M280 245 h50 v70 a25 8 0 0 1-50 0 z" fill="#2d5a3f"/>
  <path d="M330 260 c20 0 20 36 0 36" fill="none" stroke="#2d5a3f" stroke-width="7"/>
  <text x="232" y="366" fill="#231f1a" font-family="'Avenir Next', sans-serif" font-weight="600" font-size="13">Everyday Mug</text>
  <text x="232" y="384" fill="#6b645a" font-family="'Avenir Next', sans-serif" font-size="11">350 ml, Moss</text>
  <text x="232" y="416" fill="#231f1a" font-family="'Avenir Next', sans-serif" font-weight="bold" font-size="14">$26</text>

  <rect x="408" y="196" width="166" height="260" rx="12" fill="#f4efe6" stroke="#ddd4c2" stroke-width="1"/>
  <rect x="418" y="206" width="146" height="140" rx="8" fill="#e9dfcb"/>
  <rect x="438" y="236" width="106" height="70" rx="4" fill="#d8c3a5"/>
  <path d="M438 260 h106 M438 284 h106" stroke="#c4ab86" stroke-width="2"/>
  <text x="418" y="366" fill="#231f1a" font-family="'Avenir Next', sans-serif" font-weight="600" font-size="13">Washed Linen</text>
  <text x="418" y="384" fill="#6b645a" font-family="'Avenir Next', sans-serif" font-size="11">180 cm, Flax</text>
  <text x="418" y="416" fill="#231f1a" font-family="'Avenir Next', sans-serif" font-weight="bold" font-size="14">$64</text>

  <rect x="594" y="196" width="166" height="260" rx="12" fill="#f4efe6" stroke="#ddd4c2" stroke-width="1"/>
  <rect x="604" y="206" width="146" height="140" rx="8" fill="#e9dfcb"/>
  <path d="M660 230 h34 v20 c24 16 28 44 28 64 H632 c0-20 4-48 28-64 z" fill="#b4532a"/>
  <text x="604" y="366" fill="#231f1a" font-family="'Avenir Next', sans-serif" font-weight="600" font-size="13">Carafe</text>
  <text x="604" y="384" fill="#6b645a" font-family="'Avenir Next', sans-serif" font-size="11">1 litre, Terracotta</text>
  <text x="604" y="416" fill="#231f1a" font-family="'Avenir Next', sans-serif" font-weight="bold" font-size="14">$54</text>
</svg>`;
fs.writeFileSync(path.join(TEMPLATES_DIR, 'ecommerce-storefront', 'preview.svg'), ecomSvg);

// 4. Maya Okafor Portfolio preview.svg
const portSvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 500" width="800" height="500">
  <rect width="800" height="500" rx="16" fill="#efe9dc"/>
  <!-- Nav -->
  <text x="36" y="44" fill="#1d1b16" font-family="Georgia, serif" font-weight="bold" font-size="18">Maya Okafor</text>
  <text x="180" y="42" fill="#6b6558" font-family="Georgia, serif" font-size="13">About</text>
  <text x="240" y="42" fill="#6b6558" font-family="Georgia, serif" font-size="13">Experience</text>
  <text x="330" y="42" fill="#6b6558" font-family="Georgia, serif" font-size="13">Contact</text>

  <!-- Hero Headline -->
  <text x="36" y="128" fill="#1d1b16" font-family="Georgia, serif" font-weight="400" font-size="56" letter-spacing="-0.05em">Maya Okafor.</text>
  <text x="36" y="162" fill="#6b6558" font-family="Georgia, serif" font-size="16">Software engineer in Lisbon building fast, quiet tools for teams that ship.</text>

  <!-- Projects Track Cards -->
  <rect x="36" y="200" width="225" height="160" rx="12" fill="#efe9dc" stroke="#d2cab8" stroke-width="1.5"/>
  <path d="M50 280 L100 230 L150 260 L200 220 L240 250" fill="none" stroke="#1d1b16" stroke-width="2"/>
  <circle cx="100" cy="230" r="6" fill="#c2410c"/>
  <circle cx="200" cy="220" r="6" fill="#c2410c"/>
  <text x="50" y="320" fill="#1d1b16" font-family="Georgia, serif" font-weight="bold" font-size="18">Quill</text>
  <text x="50" y="340" fill="#6b6558" font-family="monospace" font-size="11">Rust · 3.1k stars</text>

  <rect x="281" y="200" width="225" height="160" rx="12" fill="#efe9dc" stroke="#d2cab8" stroke-width="1.5"/>
  <rect x="300" y="230" width="80" height="55" fill="none" stroke="#1d1b16" stroke-width="2"/>
  <rect x="400" y="230" width="80" height="55" fill="#c2410c"/>
  <text x="300" y="320" fill="#1d1b16" font-family="Georgia, serif" font-weight="bold" font-size="18">Mosaic</text>
  <text x="300" y="340" fill="#6b6558" font-family="monospace" font-size="11">TypeScript · 1.4k stars</text>

  <rect x="526" y="200" width="238" height="160" rx="12" fill="#efe9dc" stroke="#d2cab8" stroke-width="1.5"/>
  <circle cx="580" cy="255" r="28" fill="none" stroke="#1d1b16" stroke-width="2"/>
  <circle cx="645" cy="255" r="28" fill="#c2410c"/>
  <circle cx="710" cy="255" r="28" fill="none" stroke="#1d1b16" stroke-width="2"/>
  <text x="546" y="320" fill="#1d1b16" font-family="Georgia, serif" font-weight="bold" font-size="18">Tidepool</text>
  <text x="546" y="340" fill="#6b6558" font-family="monospace" font-size="11">Go · 540 stars</text>

  <!-- Experience Timeline -->
  <line x1="36" y1="390" x2="764" y2="390" stroke="#d2cab8" stroke-width="1"/>
  <text x="36" y="420" fill="#c2410c" font-family="monospace" font-size="12">2023 to now</text>
  <text x="160" y="420" fill="#1d1b16" font-family="Georgia, serif" font-weight="bold" font-size="14">Staff Engineer, Fathom</text>
  <text x="160" y="438" fill="#6b6558" font-family="Georgia, serif" font-size="12">Rewrote query planner. P95 latency fell from 1.8 s to 240 ms.</text>

  <text x="36" y="470" fill="#c2410c" font-family="monospace" font-size="12">2020 to 2023</text>
  <text x="160" y="470" fill="#1d1b16" font-family="Georgia, serif" font-weight="bold" font-size="14">Senior Engineer, Basecamp&amp;Co</text>
</svg>`;
fs.writeFileSync(path.join(TEMPLATES_DIR, 'developer-portfolio', 'preview.svg'), portSvg);

// 5. Lumen SaaS preview.svg
const saasSvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 500" width="800" height="500">
  <rect width="800" height="500" rx="16" fill="#f6f5f0"/>
  <!-- Nav -->
  <text x="36" y="42" fill="#14151a" font-family="system-ui, sans-serif" font-weight="800" font-size="18">Lumen</text>
  <text x="120" y="40" fill="#6b6b64" font-family="system-ui, sans-serif" font-size="13">Product</text>
  <text x="190" y="40" fill="#6b6b64" font-family="system-ui, sans-serif" font-size="13">Customers</text>
  <text x="280" y="40" fill="#6b6b64" font-family="system-ui, sans-serif" font-size="13">Pricing</text>
  <text x="350" y="40" fill="#6b6b64" font-family="system-ui, sans-serif" font-size="13">Docs</text>
  <rect x="680" y="24" width="84" height="28" rx="99" fill="#14151a"/>
  <text x="696" y="42" fill="#f6f5f0" font-family="system-ui, sans-serif" font-weight="bold" font-size="11">Start free</text>

  <!-- Hero Headline -->
  <text x="36" y="116" fill="#14151a" font-family="system-ui, sans-serif" font-weight="800" font-size="44" letter-spacing="-0.05em">Know why revenue moves.</text>
  <text x="36" y="146" fill="#6b6b64" font-family="system-ui, sans-serif" font-size="15">Lumen connects billing, product usage, and support data so finance and growth see the same numbers.</text>

  <!-- Pricing Cards -->
  <rect x="36" y="184" width="225" height="280" rx="14" fill="#ffffff" stroke="#dcdbd2" stroke-width="1"/>
  <text x="56" y="218" fill="#14151a" font-family="system-ui, sans-serif" font-weight="bold" font-size="16">Starter</text>
  <text x="56" y="264" fill="#14151a" font-family="system-ui, sans-serif" font-weight="900" font-size="36" letter-spacing="-0.04em">$0</text>
  <text x="56" y="300" fill="#6b6b64" font-family="system-ui, sans-serif" font-size="12">• 1 billing source</text>
  <text x="56" y="324" fill="#6b6b64" font-family="system-ui, sans-serif" font-size="12">• 30 days history</text>
  <rect x="56" y="390" width="185" height="34" rx="99" fill="#14151a"/>
  <text x="110" y="412" fill="#ffffff" font-family="system-ui, sans-serif" font-weight="600" font-size="12">Get started</text>

  <rect x="281" y="174" width="238" height="295" rx="14" fill="#14151a"/>
  <text x="305" y="210" fill="#ffffff" font-family="system-ui, sans-serif" font-weight="bold" font-size="16">Growth</text>
  <text x="305" y="258" fill="#ffffff" font-family="system-ui, sans-serif" font-weight="900" font-size="40" letter-spacing="-0.04em">$149</text>
  <text x="305" y="296" fill="#dcdbd2" font-family="system-ui, sans-serif" font-size="12">• 5 billing sources</text>
  <text x="305" y="320" fill="#dcdbd2" font-family="system-ui, sans-serif" font-size="12">• Unlimited history</text>
  <text x="305" y="344" fill="#dcdbd2" font-family="system-ui, sans-serif" font-size="12">• Cohort retention</text>
  <rect x="305" y="396" width="190" height="36" rx="99" fill="#2f4bff"/>
  <text x="350" y="419" fill="#ffffff" font-family="system-ui, sans-serif" font-weight="bold" font-size="12">Start free trial</text>

  <rect x="539" y="184" width="225" height="280" rx="14" fill="#ffffff" stroke="#dcdbd2" stroke-width="1"/>
  <text x="559" y="218" fill="#14151a" font-family="system-ui, sans-serif" font-weight="bold" font-size="16">Scale</text>
  <text x="559" y="264" fill="#14151a" font-family="system-ui, sans-serif" font-weight="900" font-size="36" letter-spacing="-0.04em">$499</text>
  <text x="559" y="300" fill="#6b6b64" font-family="system-ui, sans-serif" font-size="12">• SSO and audit log</text>
  <text x="559" y="324" fill="#6b6b64" font-family="system-ui, sans-serif" font-size="12">• Warehouse sync</text>
  <rect x="559" y="390" width="185" height="34" rx="99" fill="#14151a"/>
  <text x="610" y="412" fill="#ffffff" font-family="system-ui, sans-serif" font-weight="600" font-size="12">Talk to sales</text>
</svg>`;
fs.writeFileSync(path.join(TEMPLATES_DIR, 'modern-saas', 'preview.svg'), saasSvg);

// 6. Halftone Studio preview.svg
const agencySvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 500" width="800" height="500">
  <rect width="800" height="500" rx="16" fill="#0e0e0e"/>
  <!-- Nav -->
  <text x="36" y="44" fill="#ffffff" font-family="Helvetica, Arial, sans-serif" font-weight="800" font-size="18">Halftone</text>
  <text x="140" y="42" fill="#9d978b" font-family="Helvetica, Arial, sans-serif" font-size="13">Work</text>
  <text x="200" y="42" fill="#9d978b" font-family="Helvetica, Arial, sans-serif" font-size="13">Services</text>
  <text x="280" y="42" fill="#9d978b" font-family="Helvetica, Arial, sans-serif" font-size="13">Team</text>
  <text x="340" y="42" fill="#9d978b" font-family="Helvetica, Arial, sans-serif" font-size="13">Contact</text>

  <!-- Hero Headline -->
  <text x="36" y="128" fill="#f3f0e8" font-family="Helvetica, Arial, sans-serif" font-weight="900" font-size="48" letter-spacing="-0.05em">Brands <tspan fill="#ff5a1f">people</tspan> remember.</text>
  <text x="36" y="162" fill="#9d978b" font-family="Helvetica, Arial, sans-serif" font-size="14">An independent studio in Berlin and Lagos. Identities and digital flagships.</text>

  <!-- Case Highlight Box -->
  <rect x="36" y="196" width="728" height="150" rx="12" fill="#ff5a1f"/>
  <text x="60" y="244" fill="#0e0e0e" font-family="Helvetica, Arial, sans-serif" font-weight="900" font-size="30" letter-spacing="-0.04em">Marrow Coffee</text>
  <text x="60" y="270" fill="#0e0e0e" font-family="Helvetica, Arial, sans-serif" font-size="14">New brand identity and e-commerce rebuild.</text>

  <text x="540" y="260" fill="#0e0e0e" font-family="Helvetica, Arial, sans-serif" font-weight="900" font-size="56" letter-spacing="-0.05em">+212%</text>
  <text x="540" y="284" fill="#0e0e0e" font-family="Helvetica, Arial, sans-serif" font-weight="bold" font-size="12">online subscription revenue</text>

  <!-- Team Row -->
  <circle cx="70" cy="400" r="26" fill="#f3f0e8"/>
  <text x="60" y="406" fill="#0e0e0e" font-family="Helvetica, Arial, sans-serif" font-weight="bold" font-size="14">AK</text>
  <text x="110" y="396" fill="#f3f0e8" font-family="Helvetica, Arial, sans-serif" font-weight="bold" font-size="13">Amara Kone</text>
  <text x="110" y="414" fill="#9d978b" font-family="Helvetica, Arial, sans-serif" font-size="11">Creative Director</text>

  <circle cx="270" cy="400" r="26" fill="#f3f0e8"/>
  <text x="261" y="406" fill="#0e0e0e" font-family="Helvetica, Arial, sans-serif" font-weight="bold" font-size="14">LS</text>
  <text x="310" y="396" fill="#f3f0e8" font-family="Helvetica, Arial, sans-serif" font-weight="bold" font-size="13">Lukas Stein</text>
  <text x="310" y="414" fill="#9d978b" font-family="Helvetica, Arial, sans-serif" font-size="11">Head of Design</text>

  <circle cx="470" cy="400" r="26" fill="#f3f0e8"/>
  <text x="460" y="406" fill="#0e0e0e" font-family="Helvetica, Arial, sans-serif" font-weight="bold" font-size="14">NO</text>
  <text x="510" y="396" fill="#f3f0e8" font-family="Helvetica, Arial, sans-serif" font-weight="bold" font-size="13">Ngozi Obi</text>
  <text x="510" y="414" fill="#9d978b" font-family="Helvetica, Arial, sans-serif" font-size="11">Engineering Lead</text>

  <circle cx="670" cy="400" r="26" fill="#f3f0e8"/>
  <text x="661" y="406" fill="#0e0e0e" font-family="Helvetica, Arial, sans-serif" font-weight="bold" font-size="14">JM</text>
  <text x="710" y="396" fill="#f3f0e8" font-family="Helvetica, Arial, sans-serif" font-weight="bold" font-size="13">Jonas Miller</text>
  <text x="710" y="414" fill="#9d978b" font-family="Helvetica, Arial, sans-serif" font-size="11">Strategy</text>
</svg>`;
fs.writeFileSync(path.join(TEMPLATES_DIR, 'agency-business', 'preview.svg'), agencySvg);

console.log('All 6 preview.svg files generated successfully!');
