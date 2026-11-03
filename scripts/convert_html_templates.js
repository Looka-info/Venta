const fs = require('fs');
const path = require('path');

const ROOT_DIR = path.resolve(__dirname, '..');
const TEMPLATES_DIR = path.join(ROOT_DIR, 'templates');
const TEMP_DIR = 'C:\\Users\\mumer\\AppData\\Local\\Temp';

function toVentaLines(str, varName = 'html', isInitial = false) {
  const lines = str.split('\n');
  const out = [];
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const escaped = line
      .replace(/\\/g, '\\\\')
      .replace(/"/g, '\\"')
      .replace(/\r/g, '');
    
    const suffix = (i < lines.length - 1) ? '\\n' : '';
    const content = escaped + suffix;
    
    if (i === 0 && isInitial) {
      out.push(`    let mut ${varName} = String::from_str("${content}");`);
    } else {
      out.push(`    ${varName}.push_str("${content}");`);
    }
  }
  return out.join('\n');
}

function generateVentaFile(chunks) {
  return `import std::string::String;\n\n${chunks}\n`;
}

// ----------------------------------------------------------------------------
// 1. Dashboard -> admin-dashboard
// ----------------------------------------------------------------------------
function convertDashboard() {
  const html = fs.readFileSync(path.join(TEMP_DIR, 'dashboard.html'), 'utf-8');
  
  // Layout head & nav
  const headNav = `<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Ledgerly Admin</title><style>
:root{--bg:#f1f2f5;--fg:#0f172a;--mut:#64748b;--line:#d9dde4;--acc:#2563eb;--font:Inter,"Segoe UI",Helvetica,Arial,sans-serif}*{box-sizing:border-box}html{background:var(--bg)}body{margin:0;background:var(--bg);color:var(--fg);font:17px/1.5 var(--font);overflow-x:hidden;-webkit-font-smoothing:antialiased}a{color:inherit;text-decoration:none}:focus-visible{outline:2px solid var(--acc);outline-offset:3px}
.pad{padding-left:clamp(20px,4vw,56px);padding-right:clamp(20px,4vw,56px)}
nav{position:fixed;top:0;left:0;right:0;z-index:20;display:flex;justify-content:space-between;align-items:center;gap:20px;padding-top:16px;padding-bottom:16px}nav b{font-size:20px;letter-spacing:-.04em}nav ul{display:flex;gap:26px;list-style:none;margin:0;padding:0;font-size:15px}
.btn{display:inline-block;background:var(--fg);color:var(--bg);padding:12px 22px;border-radius:99px;font:600 15px var(--font);border:0;cursor:pointer}
.big{font-family:var(--disp,var(--font));font-weight:800;line-height:.86;letter-spacing:-.06em}
.hero{min-height:100vh;min-height:100svh;display:flex;flex-direction:column;justify-content:flex-end;padding-top:110px;padding-bottom:36px}.hero h1{font-size:clamp(58px,14vw,260px);margin:0}.m{display:block;overflow:hidden;padding-bottom:.06em}.m>span{display:block}
.hero .r{display:flex;justify-content:space-between;align-items:flex-end;gap:24px;flex-wrap:wrap;margin-top:34px}.hero p{margin:0;max-width:34ch;font-size:clamp(18px,2vw,24px);line-height:1.3}
.mq{overflow:hidden;white-space:nowrap;border-block:1px solid var(--line);padding:14px 0}.mq div{display:inline-block;font-size:clamp(44px,8vw,130px)}.mq span{margin-right:.45em}
.hs{overflow:hidden}.track{display:flex;width:max-content;max-width:100%;height:100vh;height:100svh;overflow-x:auto}.js .track{overflow:visible;max-width:none}
.p{flex:none;width:min(82vw,920px);height:100%;display:flex;flex-direction:column;justify-content:flex-end;padding:100px clamp(24px,4vw,56px) clamp(40px,8vh,90px);border-left:1px solid var(--line)}.p h3{font-size:clamp(50px,9vw,160px);margin:0 0 18px}.p p{margin:0;max-width:40ch;color:var(--mut);font-size:clamp(16px,1.6vw,21px)}.p svg{width:min(100%,520px);max-height:34vh;margin-bottom:auto}
.sec{padding-top:clamp(64px,12vh,130px);padding-bottom:clamp(64px,12vh,130px);border-top:1px solid var(--line)}.sec h2{font-size:clamp(44px,8.5vw,150px);margin:0 0 clamp(28px,5vh,60px)}
footer{border-top:1px solid var(--line);padding-top:60px;padding-bottom:28px;overflow:hidden}.fw{font-size:clamp(70px,24vw,440px);margin:0 0 36px;white-space:nowrap}.fl{display:flex;justify-content:space-between;flex-wrap:wrap;gap:12px;color:var(--mut);font-size:14px}
details{border-top:1px solid var(--line);padding:18px 0}summary{cursor:pointer;font-size:22px;letter-spacing:-.02em;font-weight:600}details p{color:var(--mut);max-width:60ch}
.g{display:grid;gap:16px}.c3{grid-template-columns:repeat(3,1fr)}.c4{grid-template-columns:repeat(4,1fr)}.card{border:1px solid var(--line);border-radius:14px;padding:26px}
@media(max-width:820px){nav ul{display:none}.p{width:86vw}.c3,.c4{grid-template-columns:1fr 1fr}}@media(max-width:560px){.c3,.c4{grid-template-columns:1fr}}

nav{background:rgba(241,242,245,.92);border-bottom:1px solid var(--line)}nav ul{gap:6px}nav li{padding:6px 12px;border-radius:8px;color:var(--mut)}nav li.on{background:#fff;color:var(--fg);box-shadow:0 1px 2px #0001}.av{width:34px;height:34px;border-radius:50%;background:var(--fg);color:#fff;display:grid;place-items:center;font-size:12px;font-weight:600}.hero small{color:var(--mut);font-size:16px;margin-bottom:8px}.hero h1{font-size:clamp(64px,17vw,320px)}.kp{display:flex;gap:clamp(20px,4vw,64px);flex-wrap:wrap}.kp b{display:block;font-size:clamp(24px,3vw,40px);letter-spacing:-.04em}.kp span{color:var(--mut);font-size:14px}.up{color:#047857!important}.mq div{color:#c3c8d2}.p svg{fill:none;stroke:var(--acc);stroke-width:4;width:100%;max-width:none}.p h3{margin-bottom:6px}.pill{padding:3px 10px;border-radius:99px;font-size:13px;font-weight:500}.ok{background:#d1fae5;color:#065f46}.pe{background:#fef3c7;color:#92400e}.fa{background:#fee2e2;color:#991b1b}table{width:100%;border-collapse:collapse;font-size:clamp(15px,1.8vw,22px)}th{text-align:left;color:var(--mut);font-weight:500;font-size:13px;padding:10px 8px;border-bottom:1px solid var(--line)}td{padding:16px 8px;border-bottom:1px solid var(--line)}.tw{overflow-x:auto}.set label{display:flex;justify-content:space-between;align-items:center;padding:18px 0;border-top:1px solid var(--line);font-size:clamp(18px,2.4vw,30px);letter-spacing:-.03em}.set input{appearance:none;width:52px;height:30px;border-radius:99px;background:#cbd2dc;position:relative;cursor:pointer;transition:.2s;flex:none}.set input:before{content:"";position:absolute;left:3px;top:3px;width:24px;height:24px;border-radius:50%;background:#fff;transition:.2s}.set input:checked{background:var(--acc)}.set input:checked:before{left:25px}
</style></head><body><nav class="pad"><b><a href="/">Ledgerly</a></b><ul><li class="on"><a href="/">Overview</a></li><li><a href="/analytics">Analytics</a></li><li><a href="/users">Customers</a></li><li><a href="/settings">Settings</a></li></ul><div class="av">AM</div></nav>
<main id="main">`;

  const footScripts = `</main>
<footer class="pad"><div class="fw big">Ledgerly</div><div class="fl"><span>&copy; 2026 Ledgerly, Inc.</span><span>Status &middot; Security &middot; API</span></div></footer><script src="https://cdnjs.cloudflare.com/ajax/libs/gsap/3.12.5/gsap.min.js"></script><script src="https://cdnjs.cloudflare.com/ajax/libs/gsap/3.12.5/ScrollTrigger.min.js"></script><script src="https://cdn.jsdelivr.net/npm/lenis@1.1.13/dist/lenis.min.js"></script><script>(function(){var rm=matchMedia("(prefers-reduced-motion:reduce)").matches;if(!window.gsap||!window.ScrollTrigger||rm)return;document.documentElement.classList.add("js");gsap.registerPlugin(ScrollTrigger);
try{var l=new Lenis({lerp:.09});l.on("scroll",ScrollTrigger.update);gsap.ticker.add(function(t){l.raf(t*1000)});gsap.ticker.lagSmoothing(0)}catch(e){}
gsap.from(".hero h1 .m>span",{yPercent:110,duration:1.3,ease:"expo.out",stagger:.1,delay:.1});
var t=document.querySelector(".track");if(t){var d=function(){return Math.max(0,t.scrollWidth-innerWidth)};gsap.to(t,{x:function(){return -d()},ease:"none",scrollTrigger:{trigger:".hs",start:"top top",end:function(){return"+="+d()},pin:true,scrub:.8,invalidateOnRefresh:true,anticipatePin:1}})}
gsap.to(".mq div",{xPercent:-40,ease:"none",scrollTrigger:{trigger:".mq",start:"top bottom",end:"bottom top",scrub:true}});
gsap.utils.toArray(".cnt").forEach(function(c){var to=+c.dataset.to,d=+(c.dataset.d||0),o={v:0};gsap.to(o,{v:to,duration:1.8,ease:"power2.out",delay:.3,scrollTrigger:{trigger:c,start:"top 95%",once:true},onUpdate:function(){c.textContent=(c.dataset.pre||"")+o.v.toLocaleString(undefined,{minimumFractionDigits:d,maximumFractionDigits:d})+(c.dataset.suf||"")}})});
gsap.from(".fw",{yPercent:35,opacity:0,ease:"none",scrollTrigger:{trigger:"footer",start:"top 92%",end:"top 35%",scrub:true}});
var r=function(){ScrollTrigger.refresh()};document.fonts&&document.fonts.ready.then(r);addEventListener("load",r)})();
</script></body></html>`;

  // Layout.va
  const layoutCode = `import std::string::String;

pub fn render_layout(child_content: &str) -> String {
${toVentaLines(headNav, 'html', true)}
    html.push_str(child_content);
${toVentaLines(footScripts, 'html', false)}
    html
}
`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'admin-dashboard', 'project', 'app', 'layout.va'), layoutCode);

  // Page content
  const pageContent = `<header class="hero pad"><small>Revenue, last 30 days</small><h1 class="big"><span class="cnt" data-to="128430" data-pre="$">$128,430</span></h1><div class="r"><div class="kp"><div><b>3,284</b><span>Active customers</span></div><div><b>4.6%</b><span>Conversion</span></div><div><b class="up">+12.4%</b><span>vs previous period</span></div></div><a class="btn" href="#orders">View orders</a></div></header>
<div class="mq big" aria-hidden="true"><div><span>Revenue</span><span>Customers</span><span>Refunds</span><span>Payouts</span><span>Revenue</span><span>Customers</span><span>Refunds</span></div></div>
<section class="hs"><div class="track">
<div class="p"><svg viewBox="0 0 400 140"><path d="M0 110C50 100 80 70 130 80S220 40 280 50 360 20 400 10"/></svg><h3 class="big">$128k</h3><p>Revenue is up 12.4%, led by annual plans.</p></div>
<div class="p"><svg viewBox="0 0 400 140"><path d="M0 120C60 110 100 100 160 80S270 70 330 40 380 30 400 20"/></svg><h3 class="big">3,284</h3><p>Active customers, with 212 new this month.</p></div>
<div class="p"><svg viewBox="0 0 400 140" style="stroke:#b91c1c"><path d="M0 40C60 50 100 30 160 60S270 50 330 80 380 70 400 90"/></svg><h3 class="big">$2.1k</h3><p>Refunds ticked up 0.8%, mostly one product line.</p></div>
<div class="p"><svg viewBox="0 0 400 140"><path d="M0 100C70 90 110 110 170 70S280 60 340 40 390 40 400 30"/></svg><h3 class="big">4.6%</h3><p>Checkout conversion improved after the new payment form.</p></div></div></section>
<section class="sec pad" id="orders"><h2 class="big">Recent orders.</h2><div class="tw"><table><tr><th>Order</th><th>Customer</th><th>Amount</th><th>Status</th></tr><tr><td>#10482</td><td>Northwind Ltd</td><td>$1,240</td><td><span class="pill ok">Paid</span></td></tr><tr><td>#10481</td><td>Tessera Inc</td><td>$860</td><td><span class="pill pe">Pending</span></td></tr><tr><td>#10480</td><td>Orbit Labs</td><td>$3,100</td><td><span class="pill ok">Paid</span></td></tr><tr><td>#10479</td><td>Fathom Co</td><td>$420</td><td><span class="pill fa">Failed</span></td></tr></table></div></section>
<section class="sec pad set"><h2 class="big">Settings.</h2><label>Email me weekly summaries<input type="checkbox" checked></label><label>Notify on failed payments<input type="checkbox" checked></label><label>Require two-factor sign-in<input type="checkbox"></label></section>`;

  const pageCode = `import std::string::String;

pub fn render_page() -> String {
${toVentaLines(pageContent, 'html', true)}
    html
}
`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'admin-dashboard', 'project', 'app', 'page.va'), pageCode);

  // Subpages: analytics, users, settings
  const analyticsContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">Analytics &amp; Metrics.</h2><div class="kp" style="margin-bottom:40px;"><div><b>142,890</b><span>Daily Requests</span></div><div><b>0.12 ms</b><span>Median Latency (p50)</span></div><div><b>0.38 ms</b><span>Tail Latency (p99)</span></div><div><b class="up">99.995%</b><span>Service Uptime</span></div></div><div class="tw"><table><tr><th>Metric</th><th>24h Average</th><th>7d Trend</th><th>Target SLA</th></tr><tr><td>Ingestion Throughput</td><td>18,420 RPS</td><td><span class="pill ok">+8.4%</span></td><td>&gt; 15,000 RPS</td></tr><tr><td>Compute Node Health</td><td>16 / 16 online</td><td><span class="pill ok">Optimal</span></td><td>16 Nodes</td></tr><tr><td>SSL Handshake Time</td><td>14 ms</td><td><span class="pill ok">-2 ms</span></td><td>&lt; 25 ms</td></tr><tr><td>Edge Cache Hit Rate</td><td>94.2%</td><td><span class="pill ok">+1.1%</span></td><td>&gt; 90.0%</td></tr></table></div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'admin-dashboard', 'project', 'app', 'analytics', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(analyticsContent, 'html', true)}\n    html\n}\n`);

  const usersContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">Customers &amp; Accounts.</h2><p style="color:var(--mut); margin-bottom:30px;">Manage active enterprise subscribers, billing tiers, and administrative roles.</p><div class="tw"><table><tr><th>Account</th><th>Primary Contact</th><th>Plan</th><th>Monthly Spend</th><th>Status</th></tr><tr><td>Northwind Ltd</td><td>sarah@northwind.com</td><td>Enterprise Tier</td><td>$1,240 / mo</td><td><span class="pill ok">Active</span></td></tr><tr><td>Tessera Inc</td><td>alex@tessera.io</td><td>Growth Plan</td><td>$860 / mo</td><td><span class="pill pe">Review</span></td></tr><tr><td>Orbit Labs</td><td>elena@orbitlabs.dev</td><td>Enterprise Tier</td><td>$3,100 / mo</td><td><span class="pill ok">Active</span></td></tr><tr><td>Fathom Co</td><td>marcus@fathom.co</td><td>Starter Plan</td><td>$420 / mo</td><td><span class="pill fa">Overdue</span></td></tr></table></div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'admin-dashboard', 'project', 'app', 'users', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(usersContent, 'html', true)}\n    html\n}\n`);

  const settingsContent = `<section class="sec pad set" style="padding-top:120px;"><h2 class="big">Preferences.</h2><label>Email me weekly summaries<input type="checkbox" checked></label><label>Notify on failed payments<input type="checkbox" checked></label><label>Require two-factor sign-in<input type="checkbox"></label><label>Real-time Slack webhooks<input type="checkbox" checked></label><label>Automated daily backup export<input type="checkbox" checked></label></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'admin-dashboard', 'project', 'app', 'settings', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(settingsContent, 'html', true)}\n    html\n}\n`);

  console.log('[OK] admin-dashboard converted');
}

// ----------------------------------------------------------------------------
// 2. Documentation -> docs-website
// ----------------------------------------------------------------------------
function convertDocumentation() {
  const headNav = `<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Kiln Docs: Getting started</title><style>
:root{--bg:#101214;--fg:#eceae4;--mut:#8d9298;--line:#262a2e;--acc:#7ee0b5;--font:"Segoe UI",Helvetica,Arial,sans-serif}*{box-sizing:border-box}html{background:var(--bg)}body{margin:0;background:var(--bg);color:var(--fg);font:17px/1.5 var(--font);overflow-x:hidden;-webkit-font-smoothing:antialiased}a{color:inherit;text-decoration:none}:focus-visible{outline:2px solid var(--acc);outline-offset:3px}
.pad{padding-left:clamp(20px,4vw,56px);padding-right:clamp(20px,4vw,56px)}
nav{position:fixed;top:0;left:0;right:0;z-index:20;display:flex;justify-content:space-between;align-items:center;gap:20px;padding-top:16px;padding-bottom:16px}nav b{font-size:20px;letter-spacing:-.04em}nav ul{display:flex;gap:26px;list-style:none;margin:0;padding:0;font-size:15px}
.btn{display:inline-block;background:var(--fg);color:var(--bg);padding:12px 22px;border-radius:99px;font:600 15px var(--font);border:0;cursor:pointer}
.big{font-family:var(--disp,var(--font));font-weight:800;line-height:.86;letter-spacing:-.06em}
.hero{min-height:100vh;min-height:100svh;display:flex;flex-direction:column;justify-content:flex-end;padding-top:110px;padding-bottom:36px}.hero h1{font-size:clamp(58px,14vw,260px);margin:0}.m{display:block;overflow:hidden;padding-bottom:.06em}.m>span{display:block}
.hero .r{display:flex;justify-content:space-between;align-items:flex-end;gap:24px;flex-wrap:wrap;margin-top:34px}.hero p{margin:0;max-width:34ch;font-size:clamp(18px,2vw,24px);line-height:1.3}
.mq{overflow:hidden;white-space:nowrap;border-block:1px solid var(--line);padding:14px 0}.mq div{display:inline-block;font-size:clamp(44px,8vw,130px)}.mq span{margin-right:.45em}
.hs{overflow:hidden}.track{display:flex;width:max-content;max-width:100%;height:100vh;height:100svh;overflow-x:auto}.js .track{overflow:visible;max-width:none}
.p{flex:none;width:min(82vw,920px);height:100%;display:flex;flex-direction:column;justify-content:flex-end;padding:100px clamp(24px,4vw,56px) clamp(40px,8vh,90px);border-left:1px solid var(--line)}.p h3{font-size:clamp(50px,9vw,160px);margin:0 0 18px}.p p{margin:0;max-width:40ch;color:var(--mut);font-size:clamp(16px,1.6vw,21px)}.p svg{width:min(100%,520px);max-height:34vh;margin-bottom:auto}
.sec{padding-top:clamp(64px,12vh,130px);padding-bottom:clamp(64px,12vh,130px);border-top:1px solid var(--line)}.sec h2{font-size:clamp(44px,8.5vw,150px);margin:0 0 clamp(28px,5vh,60px)}
footer{border-top:1px solid var(--line);padding-top:60px;padding-bottom:28px;overflow:hidden}.fw{font-size:clamp(70px,24vw,440px);margin:0 0 36px;white-space:nowrap}.fl{display:flex;justify-content:space-between;flex-wrap:wrap;gap:12px;color:var(--mut);font-size:14px}
details{border-top:1px solid var(--line);padding:18px 0}summary{cursor:pointer;font-size:22px;letter-spacing:-.02em;font-weight:600}details p{color:var(--mut);max-width:60ch}
.g{display:grid;gap:16px}.c3{grid-template-columns:repeat(3,1fr)}.c4{grid-template-columns:repeat(4,1fr)}.card{border:1px solid var(--line);border-radius:14px;padding:26px}
@media(max-width:820px){nav ul{display:none}.p{width:86vw}.c3,.c4{grid-template-columns:1fr 1fr}}@media(max-width:560px){.c3,.c4{grid-template-columns:1fr}}

nav{background:rgba(16,18,20,.92);border-bottom:1px solid var(--line)}.btn{background:var(--acc);color:#0a1511}.sb{display:flex;align-items:center;gap:10px;border:1px solid #33383d;border-radius:12px;padding:14px 18px;background:#16191c;min-width:min(420px,100%)}.sb input{flex:1;background:none;border:0;color:var(--fg);font:18px var(--font);outline:0}.sb kbd{font:12px ui-monospace,Menlo,monospace;color:var(--mut);border:1px solid #33383d;border-radius:4px;padding:2px 6px}.mq div{color:transparent;-webkit-text-stroke:1px #3a4046}pre{margin:0 0 22px;background:#0a0c0e;border:1px solid var(--line);border-radius:12px;padding:20px 22px;overflow-x:auto;font:clamp(14px,1.4vw,18px)/1.7 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;max-width:100%}.k{color:#f2a1c4}.s{color:#a6e3a1}.c{color:#6b7480}.f{color:#89b4fa}.t{color:#f9c27b}.p h3 small{display:block;font:500 16px ui-monospace,Menlo,monospace;letter-spacing:0;color:var(--acc);margin-bottom:12px}code.i{background:#1d2125;border-radius:4px;padding:1px 6px;font:.9em ui-monospace,Menlo,monospace}table{width:100%;border-collapse:collapse;font-size:clamp(15px,1.8vw,22px)}th{text-align:left;color:var(--mut);font-weight:500;font-size:13px;padding:10px 8px;border-bottom:1px solid var(--line)}td{padding:16px 8px;border-bottom:1px solid var(--line)}.tw{overflow-x:auto}.note{border-left:3px solid var(--acc);background:#14201b;padding:18px 22px;border-radius:0 10px 10px 0;margin-top:28px;font-size:18px;max-width:60ch}
</style></head><body><nav class="pad"><b><a href="/">Kiln</a></b><ul><li><a href="/">Guides</a></li><li><a href="/guide/architecture">Architecture</a></li><li><a href="/guide/components">Components</a></li><li><a href="/reference/api">Reference</a></li></ul><a class="btn" href="/#ref">v2.4</a></nav>
<main id="main">`;

  const footScripts = `</main>
<footer class="pad"><div class="fw big">Kiln</div><div class="fl"><span>Docs v2.4 &middot; Last updated Nov 2026</span><span>GitHub &middot; Discord &middot; Changelog</span></div></footer>
<script>document.getElementById("q")&&document.getElementById("q").addEventListener("input",function(e){var v=e.target.value.toLowerCase();document.querySelectorAll(".p").forEach(function(p){p.style.opacity=!v||p.textContent.toLowerCase().indexOf(v)>-1?1:.25})});</script><script src="https://cdnjs.cloudflare.com/ajax/libs/gsap/3.12.5/gsap.min.js"></script><script src="https://cdnjs.cloudflare.com/ajax/libs/gsap/3.12.5/ScrollTrigger.min.js"></script><script src="https://cdn.jsdelivr.net/npm/lenis@1.1.13/dist/lenis.min.js"></script><script>(function(){var rm=matchMedia("(prefers-reduced-motion:reduce)").matches;if(!window.gsap||!window.ScrollTrigger||rm)return;document.documentElement.classList.add("js");gsap.registerPlugin(ScrollTrigger);
try{var l=new Lenis({lerp:.09});l.on("scroll",ScrollTrigger.update);gsap.ticker.add(function(t){l.raf(t*1000)});gsap.ticker.lagSmoothing(0)}catch(e){}
gsap.from(".hero h1 .m>span",{yPercent:110,duration:1.3,ease:"expo.out",stagger:.1,delay:.1});
var t=document.querySelector(".track");if(t){var d=function(){return Math.max(0,t.scrollWidth-innerWidth)};gsap.to(t,{x:function(){return -d()},ease:"none",scrollTrigger:{trigger:".hs",start:"top top",end:function(){return"+="+d()},pin:true,scrub:.8,invalidateOnRefresh:true,anticipatePin:1}})}
gsap.to(".mq div",{xPercent:-40,ease:"none",scrollTrigger:{trigger:".mq",start:"top bottom",end:"bottom top",scrub:true}});
gsap.utils.toArray(".cnt").forEach(function(c){var to=+c.dataset.to,d=+(c.dataset.d||0),o={v:0};gsap.to(o,{v:to,duration:1.8,ease:"power2.out",delay:.3,scrollTrigger:{trigger:c,start:"top 95%",once:true},onUpdate:function(){c.textContent=(c.dataset.pre||"")+o.v.toLocaleString(undefined,{minimumFractionDigits:d,maximumFractionDigits:d})+(c.dataset.suf||"")}})});
gsap.from(".fw",{yPercent:35,opacity:0,ease:"none",scrollTrigger:{trigger:"footer",start:"top 92%",end:"top 35%",scrub:true}});
var r=function(){ScrollTrigger.refresh()};document.fonts&&document.fonts.ready.then(r);addEventListener("load",r)})();
</script></body></html>`;

  fs.writeFileSync(path.join(TEMPLATES_DIR, 'docs-website', 'project', 'app', 'layout.va'), `import std::string::String;\n\npub fn render_layout(child_content: &str) -> String {\n${toVentaLines(headNav, 'html', true)}\n    html.push_str(child_content);\n${toVentaLines(footScripts, 'html', false)}\n    html\n}\n`);

  const pageContent = `<header class="hero pad"><h1 class="big"><span class="m"><span>Docs for</span></span><span class="m"><span>builders.</span></span></h1><div class="r"><p>Kiln compiles and bundles your project in milliseconds. Get from zero to a production build in five minutes.</p><div class="sb"><input id="q" placeholder="Search the docs" aria-label="Search the docs"><kbd>Ctrl K</kbd></div></div></header>
<div class="mq big" aria-hidden="true"><div><span>Install</span><span>Configure</span><span>Build</span><span>Deploy</span><span>Install</span><span>Configure</span><span>Build</span></div></div>
<section class="hs"><div class="track">
<div class="p"><h3 class="big"><small>Step 1</small>Install</h3><pre><span class="f">npm</span> install --save-dev kiln</pre><p>Kiln needs Node 20 or newer. Install it as a dev dependency so everyone on the team uses the same version.</p></div>
<div class="p"><h3 class="big"><small>Step 2</small>Configure</h3><pre><span class="c">// kiln.config.js</span>
<span class="k">export default</span> {
  <span class="f">entry</span>: <span class="s">"src/index.js"</span>,
  <span class="f">outDir</span>: <span class="s">"dist"</span>,
  <span class="f">minify</span>: <span class="t">true</span>,
};</pre><p>Every option is optional. Run <code class="i">kiln init</code> to generate this file.</p></div>
<div class="p"><h3 class="big"><small>Step 3</small>Build</h3><pre><span class="f">npx</span> kiln build
<span class="c"># built 128 modules in 212 ms</span></pre><p>Incremental builds reuse the cache and usually finish in under 50 ms.</p></div>
<div class="p"><h3 class="big"><small>Step 4</small>Deploy</h3><pre><span class="f">npx</span> kiln deploy --target static</pre><p>Upload the <code class="i">dist</code> folder to any static host, or use a built-in adapter.</p></div></div></section>
<section class="sec pad" id="ref"><h2 class="big">Options.</h2><div class="tw"><table><tr><th>Option</th><th>Type</th><th>Default</th><th>Description</th></tr><tr><td><code class="i">entry</code></td><td>string</td><td>"src/index.js"</td><td>The file Kiln starts from.</td></tr><tr><td><code class="i">outDir</code></td><td>string</td><td>"dist"</td><td>Where built files are written.</td></tr><tr><td><code class="i">minify</code></td><td>boolean</td><td>true</td><td>Shrink output for production.</td></tr></table></div><div class="note"><b>Tip.</b> Set <code class="i">minify: false</code> while debugging to keep readable output.</div></section>`;

  fs.writeFileSync(path.join(TEMPLATES_DIR, 'docs-website', 'project', 'app', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(pageContent, 'html', true)}\n    html\n}\n`);

  // Subpages
  const archContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">Architecture.</h2><p style="color:var(--mut); margin-bottom:28px;">Kiln is structured around a three-tier pipeline: AST transformation, multi-threaded asset graph resolution, and deterministic dead-code stripping.</p><pre><span class="c">// Compiler stage pipeline</span>
<span class="k">Stage 1</span>: Source lexical analysis &amp; parser
<span class="k">Stage 2</span>: Intermediate Representation (MIR)
<span class="k">Stage 3</span>: Static Single Assignment (14-pass SSA optimizer)
<span class="k">Stage 4</span>: Machine code emission &amp; asset packaging</pre><div class="note"><b>Zero GC Overheads.</b> Every intermediate buffer uses stack-allocated affine slices.</div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'docs-website', 'project', 'app', 'guide', 'architecture', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(archContent, 'html', true)}\n    html\n}\n`);

  const compContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">Components.</h2><p style="color:var(--mut); margin-bottom:28px;">Kiln components are written in native Venta syntax with server-side rendered layouts and streaming boundary support.</p><pre><span class="k">import</span> std::string::String;

<span class="k">pub fn</span> <span class="f">render_button</span>(label: &amp;str) -&gt; String {
    <span class="k">let mut</span> b = String::from_str(<span class="s">"&lt;button class=\\"btn\\"&gt;"</span>);
    b.push_str(label);
    b.push_str(<span class="s">"&lt;/button&gt;"</span>);
    b
}</pre></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'docs-website', 'project', 'app', 'guide', 'components', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(compContent, 'html', true)}\n    html\n}\n`);

  const refContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">API Reference.</h2><p style="color:var(--mut); margin-bottom:28px;">Detailed options and environment flags for <code class="i">kiln.config.js</code>.</p><div class="tw"><table><tr><th>Key</th><th>Type</th><th>Description</th></tr><tr><td><code class="i">target</code></td><td>string</td><td>Platform compilation target (e.g. "static", "edge", "node").</td></tr><tr><td><code class="i">sourcemap</code></td><td>boolean</td><td>Emit inline cryptographic debug mappings.</td></tr><tr><td><code class="i">clean</code></td><td>boolean</td><td>Purge destination directory prior to bundle pass.</td></tr></table></div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'docs-website', 'project', 'app', 'reference', 'api', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(refContent, 'html', true)}\n    html\n}\n`);

  console.log('[OK] docs-website converted');
}

// ----------------------------------------------------------------------------
// 3. E-commerce -> ecommerce-storefront
// ----------------------------------------------------------------------------
function convertEcommerce() {
  const headNav = `<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Fieldnote Goods: everyday ceramics and linen</title><style>
:root{--bg:#f4efe6;--fg:#231f1a;--mut:#6b645a;--line:#ddd4c2;--acc:#1f3d2b;--font:"Avenir Next",Avenir,"Segoe UI",Helvetica,Arial,sans-serif;--disp:Georgia,serif}*{box-sizing:border-box}html{background:var(--bg)}body{margin:0;background:var(--bg);color:var(--fg);font:17px/1.5 var(--font);overflow-x:hidden;-webkit-font-smoothing:antialiased}a{color:inherit;text-decoration:none}:focus-visible{outline:2px solid var(--acc);outline-offset:3px}
.pad{padding-left:clamp(20px,4vw,56px);padding-right:clamp(20px,4vw,56px)}
nav{position:fixed;top:0;left:0;right:0;z-index:20;display:flex;justify-content:space-between;align-items:center;gap:20px;padding-top:16px;padding-bottom:16px}nav b{font-size:20px;letter-spacing:-.04em}nav ul{display:flex;gap:26px;list-style:none;margin:0;padding:0;font-size:15px}
.btn{display:inline-block;background:var(--fg);color:var(--bg);padding:12px 22px;border-radius:99px;font:600 15px var(--font);border:0;cursor:pointer}
.big{font-family:var(--disp,var(--font));font-weight:800;line-height:.86;letter-spacing:-.06em}
.hero{min-height:100vh;min-height:100svh;display:flex;flex-direction:column;justify-content:flex-end;padding-top:110px;padding-bottom:36px}.hero h1{font-size:clamp(58px,14vw,260px);margin:0}.m{display:block;overflow:hidden;padding-bottom:.06em}.m>span{display:block}
.hero .r{display:flex;justify-content:space-between;align-items:flex-end;gap:24px;flex-wrap:wrap;margin-top:34px}.hero p{margin:0;max-width:34ch;font-size:clamp(18px,2vw,24px);line-height:1.3}
.mq{overflow:hidden;white-space:nowrap;border-block:1px solid var(--line);padding:14px 0}.mq div{display:inline-block;font-size:clamp(44px,8vw,130px)}.mq span{margin-right:.45em}
.hs{overflow:hidden}.track{display:flex;width:max-content;max-width:100%;height:100vh;height:100svh;overflow-x:auto}.js .track{overflow:visible;max-width:none}
.p{flex:none;width:min(82vw,920px);height:100%;display:flex;flex-direction:column;justify-content:flex-end;padding:100px clamp(24px,4vw,56px) clamp(40px,8vh,90px);border-left:1px solid var(--line)}.p h3{font-size:clamp(50px,9vw,160px);margin:0 0 18px}.p p{margin:0;max-width:40ch;color:var(--mut);font-size:clamp(16px,1.6vw,21px)}.p svg{width:min(100%,520px);max-height:34vh;margin-bottom:auto}
.sec{padding-top:clamp(64px,12vh,130px);padding-bottom:clamp(64px,12vh,130px);border-top:1px solid var(--line)}.sec h2{font-size:clamp(44px,8.5vw,150px);margin:0 0 clamp(28px,5vh,60px)}
footer{border-top:1px solid var(--line);padding-top:60px;padding-bottom:28px;overflow:hidden}.fw{font-size:clamp(70px,24vw,440px);margin:0 0 36px;white-space:nowrap}.fl{display:flex;justify-content:space-between;flex-wrap:wrap;gap:12px;color:var(--mut);font-size:14px}
details{border-top:1px solid var(--line);padding:18px 0}summary{cursor:pointer;font-size:22px;letter-spacing:-.02em;font-weight:600}details p{color:var(--mut);max-width:60ch}
.g{display:grid;gap:16px}.c3{grid-template-columns:repeat(3,1fr)}.c4{grid-template-columns:repeat(4,1fr)}.card{border:1px solid var(--line);border-radius:14px;padding:26px}
@media(max-width:820px){nav ul{display:none}.p{width:86vw}.c3,.c4{grid-template-columns:1fr 1fr}}@media(max-width:560px){.c3,.c4{grid-template-columns:1fr}}

.big{font-weight:400;letter-spacing:-.045em}.ann{position:fixed;top:0;left:0;right:0;z-index:21;background:var(--acc);color:#f1ead9;text-align:center;font-size:13px;padding:8px 12px}nav{top:34px;background:rgba(244,239,230,.94)}nav b{font:600 26px Georgia,serif}.hero{padding-top:150px}.mq div{color:var(--acc)}.p svg{width:100%;max-width:none;height:34vh;max-height:none;border-radius:14px}.pd svg{display:block;width:100%;aspect-ratio:1;border-radius:14px;background:#e9dfcb}.pd h4{margin:12px 0 2px;font-size:17px}.pd p{margin:0;color:var(--mut);font-size:14px}.pd .r2{display:flex;justify-content:space-between;align-items:center;margin-top:10px}.pd button{border:1px solid var(--fg);background:none;border-radius:99px;padding:7px 16px;cursor:pointer;font:600 13px var(--font)}.pd button:hover{background:var(--fg);color:var(--bg)}
</style></head><body><div class="ann">Free shipping on orders over $60. Made in small batches in Portugal.</div><nav class="pad"><b><a href="/">Fieldnote</a></b><ul><li><a href="/shop">Dinnerware</a></li><li><a href="/shop">Linen</a></li><li><a href="/shop">Kitchen</a></li><li><a href="/cart">Gifts</a></li></ul><a href="/cart" class="btn" id="bag" style="text-decoration:none;">Bag (0)</a></nav>
<main id="main">`;

  const footScripts = `</main>
<footer class="pad"><div class="fw big">Fieldnote</div><div class="fl"><span>Free returns within 30 days</span><span>Shipping &middot; Returns &middot; Journal &middot; Contact</span></div></footer>
<script>var n=0,b=document.getElementById("bag");document.querySelectorAll(".pd button").forEach(function(x){x.onclick=function(){n++;b.textContent="Bag ("+n+")";x.textContent="Added"}})</script><script src="https://cdnjs.cloudflare.com/ajax/libs/gsap/3.12.5/gsap.min.js"></script><script src="https://cdnjs.cloudflare.com/ajax/libs/gsap/3.12.5/ScrollTrigger.min.js"></script><script src="https://cdn.jsdelivr.net/npm/lenis@1.1.13/dist/lenis.min.js"></script><script>(function(){var rm=matchMedia("(prefers-reduced-motion:reduce)").matches;if(!window.gsap||!window.ScrollTrigger||rm)return;document.documentElement.classList.add("js");gsap.registerPlugin(ScrollTrigger);
try{var l=new Lenis({lerp:.09});l.on("scroll",ScrollTrigger.update);gsap.ticker.add(function(t){l.raf(t*1000)});gsap.ticker.lagSmoothing(0)}catch(e){}
gsap.from(".hero h1 .m>span",{yPercent:110,duration:1.3,ease:"expo.out",stagger:.1,delay:.1});
var t=document.querySelector(".track");if(t){var d=function(){return Math.max(0,t.scrollWidth-innerWidth)};gsap.to(t,{x:function(){return -d()},ease:"none",scrollTrigger:{trigger:".hs",start:"top top",end:function(){return"+="+d()},pin:true,scrub:.8,invalidateOnRefresh:true,anticipatePin:1}})}
gsap.to(".mq div",{xPercent:-40,ease:"none",scrollTrigger:{trigger:".mq",start:"top bottom",end:"bottom top",scrub:true}});
gsap.utils.toArray(".cnt").forEach(function(c){var to=+c.dataset.to,d=+(c.dataset.d||0),o={v:0};gsap.to(o,{v:to,duration:1.8,ease:"power2.out",delay:.3,scrollTrigger:{trigger:c,start:"top 95%",once:true},onUpdate:function(){c.textContent=(c.dataset.pre||"")+o.v.toLocaleString(undefined,{minimumFractionDigits:d,maximumFractionDigits:d})+(c.dataset.suf||"")}})});
gsap.from(".fw",{yPercent:35,opacity:0,ease:"none",scrollTrigger:{trigger:"footer",start:"top 92%",end:"top 35%",scrub:true}});
var r=function(){ScrollTrigger.refresh()};document.fonts&&document.fonts.ready.then(r);addEventListener("load",r)})();
</script></body></html>`;

  fs.writeFileSync(path.join(TEMPLATES_DIR, 'ecommerce-storefront', 'project', 'app', 'layout.va'), `import std::string::String;\n\npub fn render_layout(child_content: &str) -> String {\n${toVentaLines(headNav, 'html', true)}\n    html.push_str(child_content);\n${toVentaLines(footScripts, 'html', false)}\n    html\n}\n`);

  const pageContent = `<header class="hero pad"><h1 class="big"><span class="m"><span>Made to</span></span><span class="m"><span>be used.</span></span></h1><div class="r"><p>Hand-glazed stoneware and washed linen in soft, lived-in colors.</p><a class="btn" href="#shop">Shop bestsellers</a></div></header>
<div class="mq big" aria-hidden="true"><div><span>Stoneware</span><span>Linen</span><span>Small batch</span><span>Hand glazed</span><span>Stoneware</span><span>Linen</span><span>Small batch</span></div></div>
<section class="hs"><div class="track">
<div class="p"><svg viewBox="0 0 400 200" preserveAspectRatio="xMidYMid slice"><rect width="400" height="200" fill="#d6c6a6"/><path d="M130 50h140l-14 110a64 20 0 0 1-112 0z" fill="#f4ede0"/></svg><h3 class="big">Dinnerware</h3><p>Bowls, plates and mugs with a soft, uneven glaze.</p></div>
<div class="p"><svg viewBox="0 0 400 200" preserveAspectRatio="xMidYMid slice"><rect width="400" height="200" fill="#c8b290"/><g stroke="#b39a72" stroke-width="3"><path d="M0 60H400M0 100H400M0 140H400"/></g></svg><h3 class="big">Linen</h3><p>Washed flax runners, napkins and aprons that soften with every wash.</p></div>
<div class="p"><svg viewBox="0 0 400 200" preserveAspectRatio="xMidYMid slice"><rect width="400" height="200" fill="#b4532a"/><path d="M180 30h40v30c40 24 46 60 46 110H134c0-50 6-86 46-110z" fill="#f4ede0"/></svg><h3 class="big">Kitchen</h3><p>Carafes, crocks and serving boards for everyday cooking.</p></div>
<div class="p"><svg viewBox="0 0 400 200" preserveAspectRatio="xMidYMid slice"><rect width="400" height="200" fill="#2d5a3f"/><circle cx="200" cy="100" r="62" fill="#f4ede0"/><circle cx="200" cy="100" r="36" fill="#2d5a3f"/></svg><h3 class="big">Gifts</h3><p>Boxed sets, ready to give, with a handwritten card.</p></div></div></section>
<section class="sec pad" id="shop"><h2 class="big">Bestsellers.</h2><div class="g c4">
<div class="pd"><svg viewBox="0 0 200 200"><path d="M50 70h100l-10 70a40 12 0 0 1-80 0z" fill="#f8f3e9"/></svg><h4>Stoneware Bowl, Oat</h4><p>Set of 2</p><div class="r2"><b>$48</b><button>Add</button></div></div>
<div class="pd"><svg viewBox="0 0 200 200"><path d="M70 60h60v85a30 10 0 0 1-60 0z" fill="#2d5a3f"/><path d="M130 80c26 0 26 44 0 44" fill="none" stroke="#2d5a3f" stroke-width="9"/></svg><h4>Everyday Mug, Moss</h4><p>350 ml</p><div class="r2"><b>$26</b><button>Add</button></div></div>
<div class="pd"><svg viewBox="0 0 200 200"><rect x="30" y="60" width="140" height="90" rx="6" fill="#d8c3a5"/><path d="M30 90h140M30 120h140" stroke="#c4ab86" stroke-width="3"/></svg><h4>Washed Linen Runner</h4><p>180 cm, Flax</p><div class="r2"><b>$64</b><button>Add</button></div></div>
<div class="pd"><svg viewBox="0 0 200 200"><path d="M85 40h30v25c30 20 34 55 34 80H51c0-25 4-60 34-80z" fill="#b4532a"/></svg><h4>Carafe, Terracotta</h4><p>1 litre</p><div class="r2"><b>$54</b><button>Add</button></div></div></div></section>`;

  fs.writeFileSync(path.join(TEMPLATES_DIR, 'ecommerce-storefront', 'project', 'app', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(pageContent, 'html', true)}\n    html\n}\n`);

  // Subpages
  const shopContent = `<section class="sec pad" style="padding-top:140px;"><h2 class="big">The Collection.</h2><p style="color:var(--mut); margin-bottom:32px;">Handcrafted stoneware pottery, tableware, and washed European flax textiles.</p><div class="g c4">
<div class="pd"><svg viewBox="0 0 200 200"><path d="M50 70h100l-10 70a40 12 0 0 1-80 0z" fill="#f8f3e9"/></svg><h4>Stoneware Bowl, Oat</h4><p>Set of 2</p><div class="r2"><b>$48</b><button>Add</button></div></div>
<div class="pd"><svg viewBox="0 0 200 200"><path d="M70 60h60v85a30 10 0 0 1-60 0z" fill="#2d5a3f"/><path d="M130 80c26 0 26 44 0 44" fill="none" stroke="#2d5a3f" stroke-width="9"/></svg><h4>Everyday Mug, Moss</h4><p>350 ml</p><div class="r2"><b>$26</b><button>Add</button></div></div>
<div class="pd"><svg viewBox="0 0 200 200"><rect x="30" y="60" width="140" height="90" rx="6" fill="#d8c3a5"/><path d="M30 90h140M30 120h140" stroke="#c4ab86" stroke-width="3"/></svg><h4>Washed Linen Runner</h4><p>180 cm, Flax</p><div class="r2"><b>$64</b><button>Add</button></div></div>
<div class="pd"><svg viewBox="0 0 200 200"><path d="M85 40h30v25c30 20 34 55 34 80H51c0-25 4-60 34-80z" fill="#b4532a"/></svg><h4>Carafe, Terracotta</h4><p>1 litre</p><div class="r2"><b>$54</b><button>Add</button></div></div>
<div class="pd"><svg viewBox="0 0 200 200"><circle cx="100" cy="100" r="70" fill="#f4ede0"/></svg><h4>Dinner Plate, Dune</h4><p>Set of 4</p><div class="r2"><b>$82</b><button>Add</button></div></div>
<div class="pd"><svg viewBox="0 0 200 200"><rect x="40" y="40" width="120" height="120" rx="10" fill="#c8b290"/></svg><h4>Flax Napkins, Olive</h4><p>Set of 4</p><div class="r2"><b>$36</b><button>Add</button></div></div>
</div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'ecommerce-storefront', 'project', 'app', 'shop', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(shopContent, 'html', true)}\n    html\n}\n`);

  const cartContent = `<section class="sec pad" style="padding-top:140px;"><h2 class="big">Your Shopping Bag.</h2><div class="g" style="grid-template-columns: 2fr 1fr; gap: 32px;"><div class="tw"><table><tr><th>Item</th><th>Quantity</th><th>Unit Price</th><th>Total</th></tr><tr><td>Stoneware Bowl, Oat</td><td>1</td><td>$48</td><td>$48</td></tr><tr><td>Everyday Mug, Moss</td><td>2</td><td>$26</td><td>$52</td></tr></table></div><div class="card"><h4>Order Summary</h4><p style="margin:12px 0;">Subtotal: <b>$100</b></p><p style="margin:12px 0; color:var(--acc);">Shipping: <b>Free (Orders &gt; $60)</b></p><hr style="border:0; border-top:1px solid var(--line); margin:16px 0;"><button class="btn" style="width:100%;">Proceed to Checkout</button></div></div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'ecommerce-storefront', 'project', 'app', 'cart', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(cartContent, 'html', true)}\n    html\n}\n`);

  console.log('[OK] ecommerce-storefront converted');
}

// ----------------------------------------------------------------------------
// 4. Portfolio -> developer-portfolio
// ----------------------------------------------------------------------------
function convertPortfolio() {
  const headNav = `<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Maya Okafor: Software Engineer</title><style>
:root{--bg:#efe9dc;--fg:#1d1b16;--mut:#6b6558;--line:#d2cab8;--acc:#c2410c;--font:Georgia,"Times New Roman",serif}*{box-sizing:border-box}html{background:var(--bg)}body{margin:0;background:var(--bg);color:var(--fg);font:17px/1.5 var(--font);overflow-x:hidden;-webkit-font-smoothing:antialiased}a{color:inherit;text-decoration:none}:focus-visible{outline:2px solid var(--acc);outline-offset:3px}
.pad{padding-left:clamp(20px,4vw,56px);padding-right:clamp(20px,4vw,56px)}
nav{position:fixed;top:0;left:0;right:0;z-index:20;display:flex;justify-content:space-between;align-items:center;gap:20px;padding-top:16px;padding-bottom:16px}nav b{font-size:20px;letter-spacing:-.04em}nav ul{display:flex;gap:26px;list-style:none;margin:0;padding:0;font-size:15px}
.btn{display:inline-block;background:var(--fg);color:var(--bg);padding:12px 22px;border-radius:99px;font:600 15px var(--font);border:0;cursor:pointer}
.big{font-family:var(--disp,var(--font));font-weight:800;line-height:.86;letter-spacing:-.06em}
.hero{min-height:100vh;min-height:100svh;display:flex;flex-direction:column;justify-content:flex-end;padding-top:110px;padding-bottom:36px}.hero h1{font-size:clamp(58px,14vw,260px);margin:0}.m{display:block;overflow:hidden;padding-bottom:.06em}.m>span{display:block}
.hero .r{display:flex;justify-content:space-between;align-items:flex-end;gap:24px;flex-wrap:wrap;margin-top:34px}.hero p{margin:0;max-width:34ch;font-size:clamp(18px,2vw,24px);line-height:1.3}
.mq{overflow:hidden;white-space:nowrap;border-block:1px solid var(--line);padding:14px 0}.mq div{display:inline-block;font-size:clamp(44px,8vw,130px)}.mq span{margin-right:.45em}
.hs{overflow:hidden}.track{display:flex;width:max-content;max-width:100%;height:100vh;height:100svh;overflow-x:auto}.js .track{overflow:visible;max-width:none}
.p{flex:none;width:min(82vw,920px);height:100%;display:flex;flex-direction:column;justify-content:flex-end;padding:100px clamp(24px,4vw,56px) clamp(40px,8vh,90px);border-left:1px solid var(--line)}.p h3{font-size:clamp(50px,9vw,160px);margin:0 0 18px}.p p{margin:0;max-width:40ch;color:var(--mut);font-size:clamp(16px,1.6vw,21px)}.p svg{width:min(100%,520px);max-height:34vh;margin-bottom:auto}
.sec{padding-top:clamp(64px,12vh,130px);padding-bottom:clamp(64px,12vh,130px);border-top:1px solid var(--line)}.sec h2{font-size:clamp(44px,8.5vw,150px);margin:0 0 clamp(28px,5vh,60px)}
footer{border-top:1px solid var(--line);padding-top:60px;padding-bottom:28px;overflow:hidden}.fw{font-size:clamp(70px,24vw,440px);margin:0 0 36px;white-space:nowrap}.fl{display:flex;justify-content:space-between;flex-wrap:wrap;gap:12px;color:var(--mut);font-size:14px}
details{border-top:1px solid var(--line);padding:18px 0}summary{cursor:pointer;font-size:22px;letter-spacing:-.02em;font-weight:600}details p{color:var(--mut);max-width:60ch}
.g{display:grid;gap:16px}.c3{grid-template-columns:repeat(3,1fr)}.c4{grid-template-columns:repeat(4,1fr)}.card{border:1px solid var(--line);border-radius:14px;padding:26px}
@media(max-width:820px){nav ul{display:none}.p{width:86vw}.c3,.c4{grid-template-columns:1fr 1fr}}@media(max-width:560px){.c3,.c4{grid-template-columns:1fr}}

.big{font-weight:400;letter-spacing:-.05em}nav{background:rgba(239,233,220,.92);font-size:15px}.mq div{color:var(--acc)}.p svg{stroke:var(--fg);fill:none;stroke-width:2.5}.p svg .a{fill:var(--acc);stroke:none}.m2{font:13px ui-monospace,Menlo,monospace;color:var(--mut);margin-top:12px}.job{display:grid;grid-template-columns:200px 1fr;gap:20px;padding:24px 0;border-top:1px solid var(--line)}.job h4{margin:0;font-size:clamp(24px,3vw,40px);font-weight:400;letter-spacing:-.03em}.job p{margin:4px 0 0;color:var(--mut)}@media(max-width:700px){.job{grid-template-columns:1fr}}
</style></head><body><nav class="pad"><b><a href="/">Maya Okafor</a></b><ul><li><a href="/#about">About</a></li><li><a href="/projects">Projects</a></li><li><a href="/blog">Blog</a></li><li><a href="/#exp">Experience</a></li><li><a href="/#contact">Contact</a></li></ul></nav>
<main id="main">`;

  const footScripts = `</main>
<footer class="pad" id="contact"><div class="fw big">Say hello.</div><div class="fl"><a href="mailto:maya@okafor.dev">maya@okafor.dev</a><span>GitHub &middot; LinkedIn &middot; Read.cv</span></div></footer><script src="https://cdnjs.cloudflare.com/ajax/libs/gsap/3.12.5/gsap.min.js"></script><script src="https://cdnjs.cloudflare.com/ajax/libs/gsap/3.12.5/ScrollTrigger.min.js"></script><script src="https://cdn.jsdelivr.net/npm/lenis@1.1.13/dist/lenis.min.js"></script><script>(function(){var rm=matchMedia("(prefers-reduced-motion:reduce)").matches;if(!window.gsap||!window.ScrollTrigger||rm)return;document.documentElement.classList.add("js");gsap.registerPlugin(ScrollTrigger);
try{var l=new Lenis({lerp:.09});l.on("scroll",ScrollTrigger.update);gsap.ticker.add(function(t){l.raf(t*1000)});gsap.ticker.lagSmoothing(0)}catch(e){}
gsap.from(".hero h1 .m>span",{yPercent:110,duration:1.3,ease:"expo.out",stagger:.1,delay:.1});
var t=document.querySelector(".track");if(t){var d=function(){return Math.max(0,t.scrollWidth-innerWidth)};gsap.to(t,{x:function(){return -d()},ease:"none",scrollTrigger:{trigger:".hs",start:"top top",end:function(){return"+="+d()},pin:true,scrub:.8,invalidateOnRefresh:true,anticipatePin:1}})}
gsap.to(".mq div",{xPercent:-40,ease:"none",scrollTrigger:{trigger:".mq",start:"top bottom",end:"bottom top",scrub:true}});
gsap.utils.toArray(".cnt").forEach(function(c){var to=+c.dataset.to,d=+(c.dataset.d||0),o={v:0};gsap.to(o,{v:to,duration:1.8,ease:"power2.out",delay:.3,scrollTrigger:{trigger:c,start:"top 95%",once:true},onUpdate:function(){c.textContent=(c.dataset.pre||"")+o.v.toLocaleString(undefined,{minimumFractionDigits:d,maximumFractionDigits:d})+(c.dataset.suf||"")}})});
gsap.from(".fw",{yPercent:35,opacity:0,ease:"none",scrollTrigger:{trigger:"footer",start:"top 92%",end:"top 35%",scrub:true}});
var r=function(){ScrollTrigger.refresh()};document.fonts&&document.fonts.ready.then(r);addEventListener("load",r)})();
</script></body></html>`;

  fs.writeFileSync(path.join(TEMPLATES_DIR, 'developer-portfolio', 'project', 'app', 'layout.va'), `import std::string::String;\n\npub fn render_layout(child_content: &str) -> String {\n${toVentaLines(headNav, 'html', true)}\n    html.push_str(child_content);\n${toVentaLines(footScripts, 'html', false)}\n    html\n}\n`);

  const pageContent = `<header class="hero pad"><h1 class="big"><span class="m"><span>Maya</span></span><span class="m"><span>Okafor.</span></span></h1><div class="r"><p>Software engineer in Lisbon building fast, quiet tools for teams that ship.</p><a class="btn" href="#contact">Say hello</a></div></header>
<div class="mq big" aria-hidden="true"><div><span>Rust</span><span>TypeScript</span><span>Go</span><span>PostgreSQL</span><span>WebAssembly</span><span>Rust</span><span>TypeScript</span><span>Go</span></div></div>
<section class="hs"><div class="track">
<div class="p"><svg viewBox="0 0 400 160"><path d="M20 130L100 50L180 100L260 30L380 90"/><circle class="a" cx="100" cy="50" r="9"/><circle class="a" cx="260" cy="30" r="9"/></svg><h3 class="big">Quill</h3><p>A tiny columnar database for logs, written in Rust.</p><div class="m2">Rust &middot; 3.1k stars</div></div>
<div class="p"><svg viewBox="0 0 400 160"><rect x="40" y="30" width="140" height="100"/><rect class="a" x="220" y="30" width="140" height="100"/></svg><h3 class="big">Mosaic</h3><p>A layout engine for printable reports in the browser.</p><div class="m2">TypeScript &middot; 1.4k stars</div></div>
<div class="p"><svg viewBox="0 0 400 160"><path d="M20 130H380M20 90H380M20 50H380"/><path d="M130 20V140M260 20V140" stroke="#c2410c"/></svg><h3 class="big">Ledgerline</h3><p>Plain-text accounting for small teams, with a friendly web UI.</p><div class="m2">Go &middot; 860 stars</div></div>
<div class="p"><svg viewBox="0 0 400 160"><circle cx="110" cy="80" r="46"/><circle class="a" cx="200" cy="80" r="46"/><circle cx="290" cy="80" r="46"/></svg><h3 class="big">Tidepool</h3><p>Feature flags with audit trails and instant rollbacks.</p><div class="m2">Go &middot; 540 stars</div></div></div></section>
<section class="sec pad" id="about"><h2 class="big">About.</h2><p style="font-size:clamp(22px,3vw,40px);line-height:1.25;letter-spacing:-.02em;max-width:30ch;margin:0">Eight years in developer tools and data infrastructure. I care about small APIs, honest error messages, and software that is pleasant to maintain.</p></section>
<section class="sec pad" id="exp"><h2 class="big">Experience.</h2><div class="job"><span class="m2">2023 to now</span><div><h4>Staff Engineer, Fathom</h4><p>Rewrote the query planner. P95 latency fell from 1.8 s to 240 ms.</p></div></div><div class="job"><span class="m2">2020 to 2023</span><div><h4>Senior Engineer, Basecamp&amp;Co</h4><p>Built the realtime sync layer for 2M daily users.</p></div></div><div class="job"><span class="m2">2017 to 2020</span><div><h4>Engineer, Orbit Labs</h4><p>Shipped the first CLI and the plugin system.</p></div></div></section>`;

  fs.writeFileSync(path.join(TEMPLATES_DIR, 'developer-portfolio', 'project', 'app', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(pageContent, 'html', true)}\n    html\n}\n`);

  // Subpages
  const projContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">Selected Work.</h2><div class="g" style="grid-template-columns:1fr; gap:32px;">
<div class="card"><h4>Quill — Embedded Columnar Store (Rust)</h4><p style="color:var(--mut);">Zero-allocation SIMD vectorization engine designed for immutable append-only logs.</p><div class="m2">3,120 GitHub stars &middot; 48 contributors &middot; MIT</div></div>
<div class="card"><h4>Mosaic — Printable DOM Engine (TypeScript)</h4><p style="color:var(--mut);">High-precision pagination and typographic flow solver for PDF and client-side print drivers.</p><div class="m2">1,410 GitHub stars &middot; Apache 2.0</div></div>
<div class="card"><h4>Ledgerline — Double-Entry Engine (Go)</h4><p style="color:var(--mut);">Multi-currency ledger with cryptographically verifiable audit trails and hot-spare failover.</p><div class="m2">860 GitHub stars &middot; BSD-3</div></div>
</div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'developer-portfolio', 'project', 'app', 'projects', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(projContent, 'html', true)}\n    html\n}\n`);

  const blogContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">Writing.</h2><div class="job"><span class="m2">Oct 2026</span><div><h4>Why We Chose Affine Types Over Reference Counting</h4><p>How linear and affine ownership semantics eliminate destructor churn in low-latency servers.</p></div></div><div class="job"><span class="m2">Jun 2026</span><div><h4>Deterministic Query Engines at Scale</h4><p>Building query execution plans that guarantee bit-identical results across architectures.</p></div></div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'developer-portfolio', 'project', 'app', 'blog', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(blogContent, 'html', true)}\n    html\n}\n`);

  const aboutContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">About Maya.</h2><p style="font-size:clamp(20px,2.4vw,32px); line-height:1.4; color:var(--fg); margin-bottom:24px;">I'm a systems engineer based in Lisbon, Portugal. I work on compilers, storage runtimes, and developer tooling.</p><p style="color:var(--mut); max-width:60ch; line-height:1.6;">Previously, I led the core query engine refactor at Fathom and built distributed synchronization infrastructure at Basecamp&amp;Co. When not hacking on compilers, I brew espresso and restore vintage acoustic guitars.</p></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'developer-portfolio', 'project', 'app', 'about', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(aboutContent, 'html', true)}\n    html\n}\n`);

  console.log('[OK] developer-portfolio converted');
}

// ----------------------------------------------------------------------------
// 5. SaaS -> modern-saas
// ----------------------------------------------------------------------------
function convertSaas() {
  const headNav = `<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Lumen: revenue analytics for SaaS teams</title><style>
:root{--bg:#f6f5f0;--fg:#14151a;--mut:#6b6b64;--line:#dcdbd2;--acc:#2f4bff;--font:-apple-system,"Segoe UI",Helvetica,Arial,sans-serif}*{box-sizing:border-box}html{background:var(--bg)}body{margin:0;background:var(--bg);color:var(--fg);font:17px/1.5 var(--font);overflow-x:hidden;-webkit-font-smoothing:antialiased}a{color:inherit;text-decoration:none}:focus-visible{outline:2px solid var(--acc);outline-offset:3px}
.pad{padding-left:clamp(20px,4vw,56px);padding-right:clamp(20px,4vw,56px)}
nav{position:fixed;top:0;left:0;right:0;z-index:20;display:flex;justify-content:space-between;align-items:center;gap:20px;padding-top:16px;padding-bottom:16px}nav b{font-size:20px;letter-spacing:-.04em}nav ul{display:flex;gap:26px;list-style:none;margin:0;padding:0;font-size:15px}
.btn{display:inline-block;background:var(--fg);color:var(--bg);padding:12px 22px;border-radius:99px;font:600 15px var(--font);border:0;cursor:pointer}
.big{font-family:var(--disp,var(--font));font-weight:800;line-height:.86;letter-spacing:-.06em}
.hero{min-height:100vh;min-height:100svh;display:flex;flex-direction:column;justify-content:flex-end;padding-top:110px;padding-bottom:36px}.hero h1{font-size:clamp(58px,14vw,260px);margin:0}.m{display:block;overflow:hidden;padding-bottom:.06em}.m>span{display:block}
.hero .r{display:flex;justify-content:space-between;align-items:flex-end;gap:24px;flex-wrap:wrap;margin-top:34px}.hero p{margin:0;max-width:34ch;font-size:clamp(18px,2vw,24px);line-height:1.3}
.mq{overflow:hidden;white-space:nowrap;border-block:1px solid var(--line);padding:14px 0}.mq div{display:inline-block;font-size:clamp(44px,8vw,130px)}.mq span{margin-right:.45em}
.hs{overflow:hidden}.track{display:flex;width:max-content;max-width:100%;height:100vh;height:100svh;overflow-x:auto}.js .track{overflow:visible;max-width:none}
.p{flex:none;width:min(82vw,920px);height:100%;display:flex;flex-direction:column;justify-content:flex-end;padding:100px clamp(24px,4vw,56px) clamp(40px,8vh,90px);border-left:1px solid var(--line)}.p h3{font-size:clamp(50px,9vw,160px);margin:0 0 18px}.p p{margin:0;max-width:40ch;color:var(--mut);font-size:clamp(16px,1.6vw,21px)}.p svg{width:min(100%,520px);max-height:34vh;margin-bottom:auto}
.sec{padding-top:clamp(64px,12vh,130px);padding-bottom:clamp(64px,12vh,130px);border-top:1px solid var(--line)}.sec h2{font-size:clamp(44px,8.5vw,150px);margin:0 0 clamp(28px,5vh,60px)}
footer{border-top:1px solid var(--line);padding-top:60px;padding-bottom:28px;overflow:hidden}.fw{font-size:clamp(70px,24vw,440px);margin:0 0 36px;white-space:nowrap}.fl{display:flex;justify-content:space-between;flex-wrap:wrap;gap:12px;color:var(--mut);font-size:14px}
details{border-top:1px solid var(--line);padding:18px 0}summary{cursor:pointer;font-size:22px;letter-spacing:-.02em;font-weight:600}details p{color:var(--mut);max-width:60ch}
.g{display:grid;gap:16px}.c3{grid-template-columns:repeat(3,1fr)}.c4{grid-template-columns:repeat(4,1fr)}.card{border:1px solid var(--line);border-radius:14px;padding:26px}
@media(max-width:820px){nav ul{display:none}.p{width:86vw}.c3,.c4{grid-template-columns:1fr 1fr}}@media(max-width:560px){.c3,.c4{grid-template-columns:1fr}}

nav{background:rgba(246,245,240,.9)}.big{font-weight:700}.mq div{color:transparent;-webkit-text-stroke:1px #b9b8ae}.p svg{stroke:var(--acc);fill:none;stroke-width:3}.pr b{font-size:56px;letter-spacing:-.05em;display:block;margin:6px 0}.pr .h{background:var(--fg);color:var(--bg)}.pr .h .btn{background:var(--bg);color:var(--fg)}.pr li{list-style:none;padding:8px 0;border-top:1px solid #8884;font-size:15px}.pr ul{padding:0;margin:14px 0 20px}
</style></head><body><nav class="pad"><b><a href="/">Lumen</a></b><ul><li><a href="/features">Product</a></li><li><a href="/#price">Customers</a></li><li><a href="/pricing">Pricing</a></li><li><a href="/changelog">Changelog</a></li></ul><a class="btn" href="/pricing">Start free</a></nav>
<main id="main">`;

  const footScripts = `</main>
<footer class="pad"><div class="fw big">Lumen</div><div class="fl"><span>&copy; 2026 Lumen Analytics, Inc.</span><span>Privacy &middot; Terms &middot; Status</span></div></footer><script src="https://cdnjs.cloudflare.com/ajax/libs/gsap/3.12.5/gsap.min.js"></script><script src="https://cdnjs.cloudflare.com/ajax/libs/gsap/3.12.5/ScrollTrigger.min.js"></script><script src="https://cdn.jsdelivr.net/npm/lenis@1.1.13/dist/lenis.min.js"></script><script>(function(){var rm=matchMedia("(prefers-reduced-motion:reduce)").matches;if(!window.gsap||!window.ScrollTrigger||rm)return;document.documentElement.classList.add("js");gsap.registerPlugin(ScrollTrigger);
try{var l=new Lenis({lerp:.09});l.on("scroll",ScrollTrigger.update);gsap.ticker.add(function(t){l.raf(t*1000)});gsap.ticker.lagSmoothing(0)}catch(e){}
gsap.from(".hero h1 .m>span",{yPercent:110,duration:1.3,ease:"expo.out",stagger:.1,delay:.1});
var t=document.querySelector(".track");if(t){var d=function(){return Math.max(0,t.scrollWidth-innerWidth)};gsap.to(t,{x:function(){return -d()},ease:"none",scrollTrigger:{trigger:".hs",start:"top top",end:function(){return"+="+d()},pin:true,scrub:.8,invalidateOnRefresh:true,anticipatePin:1}})}
gsap.to(".mq div",{xPercent:-40,ease:"none",scrollTrigger:{trigger:".mq",start:"top bottom",end:"bottom top",scrub:true}});
gsap.utils.toArray(".cnt").forEach(function(c){var to=+c.dataset.to,d=+(c.dataset.d||0),o={v:0};gsap.to(o,{v:to,duration:1.8,ease:"power2.out",delay:.3,scrollTrigger:{trigger:c,start:"top 95%",once:true},onUpdate:function(){c.textContent=(c.dataset.pre||"")+o.v.toLocaleString(undefined,{minimumFractionDigits:d,maximumFractionDigits:d})+(c.dataset.suf||"")}})});
gsap.from(".fw",{yPercent:35,opacity:0,ease:"none",scrollTrigger:{trigger:"footer",start:"top 92%",end:"top 35%",scrub:true}});
var r=function(){ScrollTrigger.refresh()};document.fonts&&document.fonts.ready.then(r);addEventListener("load",r)})();
</script></body></html>`;

  fs.writeFileSync(path.join(TEMPLATES_DIR, 'modern-saas', 'project', 'app', 'layout.va'), `import std::string::String;\n\npub fn render_layout(child_content: &str) -> String {\n${toVentaLines(headNav, 'html', true)}\n    html.push_str(child_content);\n${toVentaLines(footScripts, 'html', false)}\n    html\n}\n`);

  const pageContent = `<header class="hero pad"><h1 class="big"><span class="m"><span>Know why</span></span><span class="m"><span>revenue moves.</span></span></h1><div class="r"><p>Lumen connects billing, product usage, and support data so finance and growth see the same numbers.</p><a class="btn" href="#price">Start free trial</a></div></header>
<div class="mq big" aria-hidden="true"><div><span>MRR</span><span>Churn</span><span>Retention</span><span>NRR</span><span>Cohorts</span><span>Forecasts</span><span>MRR</span><span>Churn</span><span>Retention</span><span>NRR</span></div></div>
<section class="hs"><div class="track">
<div class="p"><svg viewBox="0 0 400 160"><path d="M0 130C60 120 90 70 150 80S250 40 310 50 380 20 400 10"/></svg><h3 class="big">Live MRR</h3><p>New, expansion, contraction, and churn broken out by plan, cohort, and region.</p></div>
<div class="p"><svg viewBox="0 0 400 160"><path d="M30 150V90M110 150V60M190 150V100M270 150V40M350 150V20" stroke-width="22"/></svg><h3 class="big">Cohorts</h3><p>See which signup months stick around, and what they did in the first week.</p></div>
<div class="p"><svg viewBox="0 0 400 160"><rect x="40" y="20" width="320" height="120" rx="10"/><path d="M70 60h160M70 90h220M70 115h100"/></svg><h3 class="big">Reports</h3><p>Board-ready summaries to Slack or email every Monday at 8 a.m.</p></div>
<div class="p"><svg viewBox="0 0 400 160"><circle cx="200" cy="80" r="56"/><path d="M200 40v44l28 16"/></svg><h3 class="big">Alerts</h3><p>Get pinged when churn, failed payments, or trial conversion drift off trend.</p></div></div></section>
<section class="sec pad" id="price"><h2 class="big">Simple pricing.</h2><div class="g c3 pr">
<div class="card"><h4>Starter</h4><b>$0</b><ul><li>1 billing source</li><li>30 days of history</li></ul><a class="btn" href="#">Get started</a></div>
<div class="card h"><h4>Growth</h4><b>$149</b><ul><li>5 billing sources</li><li>Unlimited history</li><li>Cohort reports</li></ul><a class="btn" href="#">Start free trial</a></div>
<div class="card"><h4>Scale</h4><b>$499</b><ul><li>SSO and audit log</li><li>Warehouse sync</li></ul><a class="btn" href="#">Talk to sales</a></div></div></section>
<section class="sec pad"><h2 class="big">Questions.</h2><details><summary>Which billing tools work?</summary><p>Stripe, Paddle, Chargebee, and Recurly, plus CSV import.</p></details><details><summary>How long is setup?</summary><p>Most teams see real numbers within 20 minutes.</p></details><details><summary>Is my data secure?</summary><p>Encrypted at rest and in transit. SOC 2 Type II audited.</p></details></section>`;

  fs.writeFileSync(path.join(TEMPLATES_DIR, 'modern-saas', 'project', 'app', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(pageContent, 'html', true)}\n    html\n}\n`);

  // Subpages
  const featContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">Platform Capabilities.</h2><div class="g c3">
<div class="card"><h4>Zero-Config Ingestion</h4><p style="color:var(--mut);">Direct webhook bindings for Stripe, Paddle, Chargebee, and App Store Connect with automated idempotency.</p></div>
<div class="card"><h4>Cohort Matrix Engine</h4><p style="color:var(--mut);">Track NRR, gross churn, and expansion velocity broken down by acquisition source and industry segment.</p></div>
<div class="card"><h4>Predictive Churn Warnings</h4><p style="color:var(--mut);">Early indicator heuristics trigger alerts before renewals lapse, based on product feature engagement drop-offs.</p></div>
</div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'modern-saas', 'project', 'app', 'features', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(featContent, 'html', true)}\n    html\n}\n`);

  const priceContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">Transparent Pricing.</h2><div class="g c3 pr">
<div class="card"><h4>Starter</h4><b>$0</b><ul><li>1 billing source</li><li>30 days history</li><li>Email notifications</li></ul><a class="btn" href="#">Get started</a></div>
<div class="card h"><h4>Growth</h4><b>$149</b><ul><li>5 billing sources</li><li>Unlimited retention</li><li>Cohort retention matrices</li><li>Slack &amp; Webhook alerts</li></ul><a class="btn" href="#">Start 14-day trial</a></div>
<div class="card"><h4>Scale</h4><b>$499</b><ul><li>Unlimited sources</li><li>Snowflake / BigQuery sync</li><li>Custom SSO &amp; SAML</li><li>Dedicated account engineer</li></ul><a class="btn" href="#">Contact Sales</a></div>
</div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'modern-saas', 'project', 'app', 'pricing', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(priceContent, 'html', true)}\n    html\n}\n`);

  const changelogContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">Changelog.</h2><div class="job"><span class="m2" style="font-size:16px;">v2.4.0 &middot; Nov 2026</span><div><h4>Sub-Millisecond Multi-Currency Normalization</h4><p style="color:var(--mut);">All foreign subscription conversions now calculate live through our zero-GC FX exchange cache.</p></div></div><div class="job"><span class="m2" style="font-size:16px;">v2.3.0 &middot; Sep 2026</span><div><h4>Direct Snowflake Data Warehouse Mirroring</h4><p style="color:var(--mut);">Automatic CDC pipe into your cloud warehouse every 15 minutes.</p></div></div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'modern-saas', 'project', 'app', 'changelog', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(changelogContent, 'html', true)}\n    html\n}\n`);

  console.log('[OK] modern-saas converted');
}

// ----------------------------------------------------------------------------
// 6. Agency -> agency-business
// ----------------------------------------------------------------------------
function convertAgency() {
  const headNav = `<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Halftone Studio: brand and digital agency</title><style>
:root{--bg:#0e0e0e;--fg:#f3f0e8;--mut:#9d978b;--line:#2a2a2a;--acc:#ff5a1f;--font:"Helvetica Neue",Helvetica,Arial,sans-serif}*{box-sizing:border-box}html{background:var(--bg)}body{margin:0;background:var(--bg);color:var(--fg);font:17px/1.5 var(--font);overflow-x:hidden;-webkit-font-smoothing:antialiased}a{color:inherit;text-decoration:none}:focus-visible{outline:2px solid var(--acc);outline-offset:3px}
.pad{padding-left:clamp(20px,4vw,56px);padding-right:clamp(20px,4vw,56px)}
nav{position:fixed;top:0;left:0;right:0;z-index:20;display:flex;justify-content:space-between;align-items:center;gap:20px;padding-top:16px;padding-bottom:16px}nav b{font-size:20px;letter-spacing:-.04em}nav ul{display:flex;gap:26px;list-style:none;margin:0;padding:0;font-size:15px}
.btn{display:inline-block;background:var(--fg);color:var(--bg);padding:12px 22px;border-radius:99px;font:600 15px var(--font);border:0;cursor:pointer}
.big{font-family:var(--disp,var(--font));font-weight:800;line-height:.86;letter-spacing:-.06em}
.hero{min-height:100vh;min-height:100svh;display:flex;flex-direction:column;justify-content:flex-end;padding-top:110px;padding-bottom:36px}.hero h1{font-size:clamp(58px,14vw,260px);margin:0}.m{display:block;overflow:hidden;padding-bottom:.06em}.m>span{display:block}
.hero .r{display:flex;justify-content:space-between;align-items:flex-end;gap:24px;flex-wrap:wrap;margin-top:34px}.hero p{margin:0;max-width:34ch;font-size:clamp(18px,2vw,24px);line-height:1.3}
.mq{overflow:hidden;white-space:nowrap;border-block:1px solid var(--line);padding:14px 0}.mq div{display:inline-block;font-size:clamp(44px,8vw,130px)}.mq span{margin-right:.45em}
.hs{overflow:hidden}.track{display:flex;width:max-content;max-width:100%;height:100vh;height:100svh;overflow-x:auto}.js .track{overflow:visible;max-width:none}
.p{flex:none;width:min(82vw,920px);height:100%;display:flex;flex-direction:column;justify-content:flex-end;padding:100px clamp(24px,4vw,56px) clamp(40px,8vh,90px);border-left:1px solid var(--line)}.p h3{font-size:clamp(50px,9vw,160px);margin:0 0 18px}.p p{margin:0;max-width:40ch;color:var(--mut);font-size:clamp(16px,1.6vw,21px)}.p svg{width:min(100%,520px);max-height:34vh;margin-bottom:auto}
.sec{padding-top:clamp(64px,12vh,130px);padding-bottom:clamp(64px,12vh,130px);border-top:1px solid var(--line)}.sec h2{font-size:clamp(44px,8.5vw,150px);margin:0 0 clamp(28px,5vh,60px)}
footer{border-top:1px solid var(--line);padding-top:60px;padding-bottom:28px;overflow:hidden}.fw{font-size:clamp(70px,24vw,440px);margin:0 0 36px;white-space:nowrap}.fl{display:flex;justify-content:space-between;flex-wrap:wrap;gap:12px;color:var(--mut);font-size:14px}
details{border-top:1px solid var(--line);padding:18px 0}summary{cursor:pointer;font-size:22px;letter-spacing:-.02em;font-weight:600}details p{color:var(--mut);max-width:60ch}
.g{display:grid;gap:16px}.c3{grid-template-columns:repeat(3,1fr)}.c4{grid-template-columns:repeat(4,1fr)}.card{border:1px solid var(--line);border-radius:14px;padding:26px}
@media(max-width:820px){nav ul{display:none}.p{width:86vw}.c3,.c4{grid-template-columns:1fr 1fr}}@media(max-width:560px){.c3,.c4{grid-template-columns:1fr}}

nav{mix-blend-mode:difference;color:#fff}h1 em{font-style:normal;color:var(--acc)}.btn{background:var(--acc);color:#0e0e0e}.mq div{color:transparent;-webkit-text-stroke:1px #555}.p svg{border-radius:6px;width:100%;max-width:none;height:34vh;max-height:none}.case{background:var(--acc);color:#0e0e0e;padding:clamp(28px,5vw,72px);display:grid;grid-template-columns:1.2fr 1fr;gap:40px;align-items:end}.case b{font-size:clamp(80px,14vw,220px);display:block}.case h2{margin:0}.team i{display:grid;place-items:center;width:84px;height:84px;border-radius:50%;background:var(--fg);color:#0e0e0e;font:800 26px var(--font);font-style:normal;margin-bottom:16px}.team span{color:var(--mut);font-size:14px;display:block}blockquote{margin:0;font-size:clamp(34px,6vw,96px);line-height:.98;letter-spacing:-.05em;font-weight:800;max-width:18ch}cite{display:block;margin-top:22px;font:400 16px var(--font);color:var(--mut);letter-spacing:0}@media(max-width:820px){.case{grid-template-columns:1fr}}
</style></head><body><nav class="pad"><b><a href="/">Halftone</a></b><ul><li><a href="/work">Work</a></li><li><a href="/services">Services</a></li><li><a href="/#team">Team</a></li><li><a href="/contact">Contact</a></li></ul></nav>
<main id="main">`;

  const footScripts = `</main>
<footer class="pad" id="contact"><div class="fw big" style="color:var(--acc)">Let’s talk.</div><div class="fl"><a href="mailto:hello@halftone.studio">hello@halftone.studio</a><span>Berlin &middot; Lagos</span></div></footer><script src="https://cdnjs.cloudflare.com/ajax/libs/gsap/3.12.5/gsap.min.js"></script><script src="https://cdnjs.cloudflare.com/ajax/libs/gsap/3.12.5/ScrollTrigger.min.js"></script><script src="https://cdn.jsdelivr.net/npm/lenis@1.1.13/dist/lenis.min.js"></script><script>(function(){var rm=matchMedia("(prefers-reduced-motion:reduce)").matches;if(!window.gsap||!window.ScrollTrigger||rm)return;document.documentElement.classList.add("js");gsap.registerPlugin(ScrollTrigger);
try{var l=new Lenis({lerp:.09});l.on("scroll",ScrollTrigger.update);gsap.ticker.add(function(t){l.raf(t*1000)});gsap.ticker.lagSmoothing(0)}catch(e){}
gsap.from(".hero h1 .m>span",{yPercent:110,duration:1.3,ease:"expo.out",stagger:.1,delay:.1});
var t=document.querySelector(".track");if(t){var d=function(){return Math.max(0,t.scrollWidth-innerWidth)};gsap.to(t,{x:function(){return -d()},ease:"none",scrollTrigger:{trigger:".hs",start:"top top",end:function(){return"+="+d()},pin:true,scrub:.8,invalidateOnRefresh:true,anticipatePin:1}})}
gsap.to(".mq div",{xPercent:-40,ease:"none",scrollTrigger:{trigger:".mq",start:"top bottom",end:"bottom top",scrub:true}});
gsap.utils.toArray(".cnt").forEach(function(c){var to=+c.dataset.to,d=+(c.dataset.d||0),o={v:0};gsap.to(o,{v:to,duration:1.8,ease:"power2.out",delay:.3,scrollTrigger:{trigger:c,start:"top 95%",once:true},onUpdate:function(){c.textContent=(c.dataset.pre||"")+o.v.toLocaleString(undefined,{minimumFractionDigits:d,maximumFractionDigits:d})+(c.dataset.suf||"")}})});
gsap.from(".fw",{yPercent:35,opacity:0,ease:"none",scrollTrigger:{trigger:"footer",start:"top 92%",end:"top 35%",scrub:true}});
var r=function(){ScrollTrigger.refresh()};document.fonts&&document.fonts.ready.then(r);addEventListener("load",r)})();
</script></body></html>`;

  fs.writeFileSync(path.join(TEMPLATES_DIR, 'agency-business', 'project', 'app', 'layout.va'), `import std::string::String;\n\npub fn render_layout(child_content: &str) -> String {\n${toVentaLines(headNav, 'html', true)}\n    html.push_str(child_content);\n${toVentaLines(footScripts, 'html', false)}\n    html\n}\n`);

  const pageContent = `<header class="hero pad"><h1 class="big"><span class="m"><span>Brands</span></span><span class="m"><span><em>people</em></span></span><span class="m"><span>remember.</span></span></h1><div class="r"><p>An independent studio in Berlin and Lagos. Identities, websites, and launch campaigns for companies that would rather be known than loud.</p><a class="btn" href="#contact">Start a project</a></div></header>
<div class="mq big" aria-hidden="true"><div><span>Identity</span><span>Web</span><span>Motion</span><span>Packaging</span><span>Campaigns</span><span>Identity</span><span>Web</span><span>Motion</span></div></div>
<section class="hs"><div class="track">
<div class="p"><svg viewBox="0 0 400 200" preserveAspectRatio="xMidYMid slice"><rect width="400" height="200" fill="#f3f0e8"/><circle cx="200" cy="100" r="70" fill="#ff5a1f"/></svg><h3 class="big">Marrow Coffee</h3><p>Identity and packaging for a roaster with 40 cafés.</p></div>
<div class="p"><svg viewBox="0 0 400 200" preserveAspectRatio="xMidYMid slice"><rect width="400" height="200" fill="#1f3d2b"/><path d="M0 200L200 30L400 200Z" fill="#f3f0e8"/></svg><h3 class="big">Alpine Rail</h3><p>Website and wayfinding for a regional railway.</p></div>
<div class="p"><svg viewBox="0 0 400 200" preserveAspectRatio="xMidYMid slice"><rect width="400" height="200" fill="#c9c3ff"/><rect x="70" y="30" width="100" height="150" fill="#0e0e0e"/><rect x="220" y="70" width="100" height="110" fill="#ff5a1f"/></svg><h3 class="big">Plume Bank</h3><p>Brand system and app design for a challenger bank.</p></div>
<div class="p"><svg viewBox="0 0 400 200" preserveAspectRatio="xMidYMid slice"><rect width="400" height="200" fill="#ffd9c7"/><g fill="#0e0e0e"><circle cx="110" cy="100" r="40"/><rect x="190" y="60" width="80" height="80"/><path d="M300 140L340 60L380 140Z"/></g></svg><h3 class="big">Kite Foods</h3><p>Launch campaign and motion kit for a snack brand.</p></div></div></section>
<section class="sec pad"><div class="case"><div><h2 class="big">Marrow Coffee</h2><p style="max-width:34ch;font-size:19px;margin:16px 0 0">New identity and an e-commerce site rebuilt around subscriptions.</p></div><div><b class="big"><span class="cnt" data-to="212" data-pre="+" data-suf="%">+212%</span></b><p>online subscription revenue in six months.</p></div></div></section>
<section class="sec pad" id="team"><h2 class="big">The team.</h2><div class="g c4 team"><div><i>AK</i><b>Amara Kone</b><span>Creative director</span></div><div><i>LS</i><b>Lukas Stein</b><span>Head of design</span></div><div><i>NO</i><b>Ngozi Obi</b><span>Engineering lead</span></div><div><i>JM</i><b>Jonas Miller</b><span>Strategy</span></div></div></section>
<section class="sec pad"><blockquote>They gave us a brand we were proud of in six weeks.<cite>Sana Idris, CEO of Plume Bank</cite></blockquote></section>`;

  fs.writeFileSync(path.join(TEMPLATES_DIR, 'agency-business', 'project', 'app', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(pageContent, 'html', true)}\n    html\n}\n`);

  // Subpages
  const servContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">Capabilities.</h2><div class="g c4 team">
<div><b>Brand Architecture</b><span>Positioning, verbal identity, design systems, and trademark assets.</span></div>
<div><b>Digital Flagships</b><span>Zero-bloat web engineering, mobile applications, and commerce systems.</span></div>
<div><b>Motion &amp; 3D</b><span>Brand launch films, product animations, and interactive spatial graphics.</span></div>
<div><b>Packaging &amp; Print</b><span>Custom structural packaging, print production, and signage systems.</span></div>
</div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'agency-business', 'project', 'app', 'services', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(servContent, 'html', true)}\n    html\n}\n`);

  const workContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">Selected Cases.</h2><div class="g" style="grid-template-columns:1fr; gap:32px;">
<div class="case"><div><h2 class="big">Alpine Rail</h2><p style="max-width:34ch;font-size:19px;margin:16px 0 0">Unified bilingual digital ticketing and live Alpine transit wayfinding across 120 mountain stations.</p></div><div><b class="big">+41%</b><p>digital ticket volume in first season.</p></div></div>
<div class="case" style="background:#c9c3ff; color:#0e0e0e;"><div><h2 class="big">Plume Bank</h2><p style="max-width:34ch;font-size:19px;margin:16px 0 0">Identity and iOS/Android interface design for Europe's fastest growing fintech for creative freelancers.</p></div><div><b class="big">4.9★</b><p>App Store rating across 80,000 reviews.</p></div></div>
</div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'agency-business', 'project', 'app', 'work', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(workContent, 'html', true)}\n    html\n}\n`);

  const contactContent = `<section class="sec pad" style="padding-top:120px;"><h2 class="big">Initiate Collaboration.</h2><p style="font-size:clamp(20px,2.4vw,32px); line-height:1.4; margin-bottom:28px;">We accept a maximum of four client engagements per quarter to ensure partner-level dedication.</p><div class="g" style="grid-template-columns:1fr 1fr; gap:24px;"><div class="card"><h4>Berlin Studio</h4><p style="color:var(--mut);">Torstraße 140, 10119 Berlin<br>berlin@halftone.studio</p></div><div class="card"><h4>Lagos Studio</h4><p style="color:var(--mut);">14 Victoria Island, Lagos<br>lagos@halftone.studio</p></div></div></section>`;
  fs.writeFileSync(path.join(TEMPLATES_DIR, 'agency-business', 'project', 'app', 'contact', 'page.va'), `import std::string::String;\n\npub fn render_page() -> String {\n${toVentaLines(contactContent, 'html', true)}\n    html\n}\n`);

  console.log('[OK] agency-business converted');
}

convertDashboard();
convertDocumentation();
convertEcommerce();
convertPortfolio();
convertSaas();
convertAgency();
console.log('All 6 templates converted successfully into pure Venta!');
