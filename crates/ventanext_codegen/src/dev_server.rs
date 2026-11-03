// ============================================================================
// ⚡ VentaNext Framework — Phase 5 Development Server & Hot-Reload Bridge
// Multi-Threaded HTTP/1.1 Engine, RFC 6455 WebSocket Handshake & Framing,
// Server-Sent Events (SSE), File System Watcher, and Live Browser Refresh
// ============================================================================

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ----------------------------------------------------------------------------
// 1. Pure Rust FIPS 180-1 SHA-1 Engine & RFC 4648 Base64 Encoder
// ----------------------------------------------------------------------------

pub fn compute_sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x67452301,
        0xEFCDAB89,
        0x98BADCFE,
        0x10325476,
        0xC3D2E1F0,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, val) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&val.to_be_bytes());
    }
    out
}

pub fn encode_base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };

        let n = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[(n >> 18) & 63] as char);
        out.push(TABLE[(n >> 12) & 63] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(n >> 6) & 63] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[n & 63] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Compute RFC 6455 Sec-WebSocket-Accept key
pub fn compute_websocket_accept(sec_websocket_key: &str) -> String {
    const RFC6455_MAGIC_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let concatenated = format!("{}{}", sec_websocket_key.trim(), RFC6455_MAGIC_GUID);
    let sha1 = compute_sha1(concatenated.as_bytes());
    encode_base64(&sha1)
}

/// Encode RFC 6455 unmasked text frame
pub fn encode_websocket_text_frame(payload: &str) -> Vec<u8> {
    let bytes = payload.as_bytes();
    let len = bytes.len();
    let mut frame = Vec::with_capacity(len + 10);

    // FIN = 1, Opcode = 0x1 (Text)
    frame.push(0x81);

    if len <= 125 {
        frame.push(len as u8);
    } else if len <= 65535 {
        frame.push(126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }

    frame.extend_from_slice(bytes);
    frame
}

// ----------------------------------------------------------------------------
// 2. Live Reload Event Models
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadEvent {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rebuild_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub timestamp: u64,
}

impl ReloadEvent {
    pub fn reload(file: &str, rebuild_ms: u64) -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            r#type: "reload".to_string(),
            file: Some(file.to_string()),
            rebuild_ms: Some(rebuild_ms),
            message: None,
            timestamp: ts,
        }
    }

    pub fn connected(port: u16) -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            r#type: "connected".to_string(),
            file: None,
            rebuild_ms: None,
            message: Some(format!("VentaNext DevServer connected on port {}", port)),
            timestamp: ts,
        }
    }

    pub fn error(msg: &str, file: &str) -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            r#type: "error".to_string(),
            file: Some(file.to_string()),
            rebuild_ms: None,
            message: Some(msg.to_string()),
            timestamp: ts,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{\"type\":\"reload\"}".to_string())
    }

    pub fn to_sse_bytes(&self) -> Vec<u8> {
        let json = self.to_json();
        format!("data: {}\n\n", json).into_bytes()
    }
}

// ----------------------------------------------------------------------------
// 3. Client Connections & Live Reload Bridge
// ----------------------------------------------------------------------------

enum ClientKind {
    WebSocket,
    ServerSentEvents,
}

struct ActiveClient {
    stream: TcpStream,
    kind: ClientKind,
}

#[derive(Clone)]
pub struct LiveReloadBridge {
    clients: Arc<Mutex<Vec<ActiveClient>>>,
    last_rebuild_ms: Arc<AtomicU64>,
    port: u16,
}

impl LiveReloadBridge {
    pub fn new(port: u16) -> Self {
        Self {
            clients: Arc::new(Mutex::new(Vec::new())),
            last_rebuild_ms: Arc::new(AtomicU64::new(0)),
            port,
        }
    }

    pub fn add_websocket_client(&self, mut stream: TcpStream) {
        let conn_evt = ReloadEvent::connected(self.port);
        let frame = encode_websocket_text_frame(&conn_evt.to_json());
        let _ = stream.write_all(&frame);
        let _ = stream.flush();

        if let Ok(mut list) = self.clients.lock() {
            list.push(ActiveClient {
                stream,
                kind: ClientKind::WebSocket,
            });
        }
    }

    pub fn add_sse_client(&self, mut stream: TcpStream) {
        let conn_evt = ReloadEvent::connected(self.port);
        let bytes = conn_evt.to_sse_bytes();
        let _ = stream.write_all(&bytes);
        let _ = stream.flush();

        if let Ok(mut list) = self.clients.lock() {
            list.push(ActiveClient {
                stream,
                kind: ClientKind::ServerSentEvents,
            });
        }
    }

    pub fn broadcast(&self, event: &ReloadEvent) {
        if let Some(ms) = event.rebuild_ms {
            self.last_rebuild_ms.store(ms, Ordering::SeqCst);
        }

        let json = event.to_json();
        let ws_frame = encode_websocket_text_frame(&json);
        let sse_bytes = event.to_sse_bytes();

        if let Ok(mut list) = self.clients.lock() {
            let mut retained = Vec::new();
            for mut client in list.drain(..) {
                let res = match client.kind {
                    ClientKind::WebSocket => client.stream.write_all(&ws_frame),
                    ClientKind::ServerSentEvents => client.stream.write_all(&sse_bytes),
                };
                if res.is_ok() && client.stream.flush().is_ok() {
                    retained.push(client);
                }
            }
            *list = retained;
        }
    }

    pub fn client_count(&self) -> usize {
        self.clients.lock().map(|l| l.len()).unwrap_or(0)
    }

    pub fn last_rebuild_ms(&self) -> u64 {
        self.last_rebuild_ms.load(Ordering::SeqCst)
    }
}

// ----------------------------------------------------------------------------
// 4. File Watcher & Change Detector
// ----------------------------------------------------------------------------

pub fn scan_directory_mtimes(dir: &Path, mtimes: &mut HashMap<PathBuf, SystemTime>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with('.') && name != "target" && name != "out" {
                    scan_directory_mtimes(&p, mtimes);
                }
            } else if let Ok(meta) = entry.metadata() {
                if let Ok(mtime) = meta.modified() {
                    mtimes.insert(p, mtime);
                }
            }
        }
    }
}

pub fn start_file_watcher(
    watch_root: PathBuf,
    debounce_ms: u64,
    bridge: LiveReloadBridge,
    stop_signal: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut snapshot: HashMap<PathBuf, SystemTime> = HashMap::new();
        scan_directory_mtimes(&watch_root, &mut snapshot);

        while !stop_signal.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(debounce_ms));

            let mut current: HashMap<PathBuf, SystemTime> = HashMap::new();
            scan_directory_mtimes(&watch_root, &mut current);

            let mut changed_file: Option<String> = None;

            for (p, mtime) in &current {
                match snapshot.get(p) {
                    Some(prev_time) if prev_time != mtime => {
                        changed_file = Some(p.to_string_lossy().to_string());
                        break;
                    }
                    None => {
                        changed_file = Some(p.to_string_lossy().to_string());
                        break;
                    }
                    _ => {}
                }
            }

            if changed_file.is_none() && current.len() != snapshot.len() {
                changed_file = Some("file inventory updated".to_string());
            }

            if let Some(file_desc) = changed_file {
                let start = Instant::now();
                snapshot = current;

                let app_dir = if watch_root.join("app").exists() {
                    watch_root.join("app")
                } else {
                    watch_root.clone()
                };

                let rel_display = Path::new(&file_desc)
                    .strip_prefix(&watch_root)
                    .unwrap_or(Path::new(&file_desc))
                    .to_string_lossy();

                println!("⚡ [ventanext dev] File modified: {}", rel_display);

                let rebuild_res = crate::scan_app_directory(&app_dir);
                let duration_ms = start.elapsed().as_millis() as u64;

                match rebuild_res {
                    Ok(discovered) => {
                        println!(
                            "⚡ [ventanext dev] Rebuilt {} route(s) in {}ms • Triggering hot reload...",
                            discovered.routes.len(),
                            duration_ms
                        );
                        let evt = ReloadEvent::reload(&rel_display, duration_ms);
                        bridge.broadcast(&evt);
                    }
                    Err(e) => {
                        eprintln!("❌ [ventanext dev] Compilation/Scan error: {}", e);
                        let evt = ReloadEvent::error(&e.to_string(), &rel_display);
                        bridge.broadcast(&evt);
                    }
                }
            }
        }
    })
}

// ----------------------------------------------------------------------------
// 5. Client Live Reload Injected Script
// ----------------------------------------------------------------------------

pub const LIVE_RELOAD_CLIENT_SCRIPT: &str = r#"
<!-- VentaNext Live Reload & Hot Refresh Client Script -->
<script id="ventanext-live-reload">
(function() {
  if (window.__VENTANEXT_HOT_RELOAD__) return;
  window.__VENTANEXT_HOT_RELOAD__ = true;
  var proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  var wsUrl = proto + '//' + location.host + '/_ventanext/reload';
  var reconnectTimer = null;

  function connect() {
    try {
      var ws = new WebSocket(wsUrl);
      ws.onopen = function() {
        console.log('[VentaNext] Live Reload WebSocket connected on ' + wsUrl);
        removeErrorOverlay();
      };
      ws.onmessage = function(ev) {
        try {
          var data = JSON.parse(ev.data);
          if (data.type === 'reload') {
            console.log('[VentaNext] Rebuilt ' + (data.file || 'source') + ' in ' + (data.rebuild_ms || 0) + 'ms. Refreshing...');
            location.reload();
          } else if (data.type === 'error') {
            showErrorOverlay(data.message || 'Build Error', data.file);
          }
        } catch(e) {}
      };
      ws.onclose = function() {
        clearTimeout(reconnectTimer);
        reconnectTimer = setTimeout(fallbackSSE, 1000);
      };
      ws.onerror = function() {
        ws.close();
      };
    } catch(err) {
      fallbackSSE();
    }
  }

  function fallbackSSE() {
    try {
      var es = new EventSource('/_ventanext/reload');
      es.onmessage = function(ev) {
        try {
          var data = JSON.parse(ev.data);
          if (data.type === 'reload') {
            location.reload();
          } else if (data.type === 'error') {
            showErrorOverlay(data.message || 'Build Error', data.file);
          }
        } catch(e) {}
      };
      es.onerror = function() {
        es.close();
        clearTimeout(reconnectTimer);
        reconnectTimer = setTimeout(connect, 2000);
      };
    } catch(e) {
      setTimeout(connect, 3000);
    }
  }

  function showErrorOverlay(msg, file) {
    var existing = document.getElementById('ventanext-error-overlay');
    if (!existing) {
      existing = document.createElement('div');
      existing.id = 'ventanext-error-overlay';
      existing.style.cssText = 'position:fixed;top:0;left:0;right:0;bottom:0;background:rgba(10,12,18,0.96);z-index:999999;color:#fff;font-family:monospace;padding:32px;overflow:auto;backdrop-filter:blur(10px);border:2px solid #ef4444;';
      document.body.appendChild(existing);
    }
    existing.innerHTML = '<div style="max-width:800px;margin:0 auto;"><div style="color:#ef4444;font-size:1.4rem;font-weight:bold;margin-bottom:12px;">⚠️ VentaNext Compilation Error</div>' +
      (file ? '<div style="color:#ffd700;font-size:0.95rem;margin-bottom:16px;">File: ' + file + '</div>' : '') +
      '<pre style="background:#05060a;padding:20px;border-radius:8px;border:1px solid #333;color:#fca5a5;font-size:0.95rem;line-height:1.5;white-space:pre-wrap;">' + msg + '</pre>' +
      '<div style="color:#888;font-size:0.85rem;margin-top:20px;">Saving the file with corrections will automatically resolve this error and refresh the preview.</div></div>';
  }

  function removeErrorOverlay() {
    var el = document.getElementById('ventanext-error-overlay');
    if (el) el.remove();
  }

  if (window.WebSocket) connect(); else fallbackSSE();
})();
</script>
"#;

pub fn inject_live_reload_script(html: &str) -> String {
    if html.contains("id=\"ventanext-live-reload\"") {
        return html.to_string();
    }
    if let Some(idx) = html.rfind("</body>") {
        let (head, tail) = html.split_at(idx);
        format!("{}{}{}", head, LIVE_RELOAD_CLIENT_SCRIPT, tail)
    } else {
        format!("{}{}", html, LIVE_RELOAD_CLIENT_SCRIPT)
    }
}

// ----------------------------------------------------------------------------
// 6. Dev Server HTTP Connection Handler
// ----------------------------------------------------------------------------

pub struct DevServerConfig {
    pub host: String,
    pub port: u16,
    pub project_dir: PathBuf,
    pub debounce_ms: u64,
}

impl Default for DevServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3000,
            project_dir: PathBuf::from("."),
            debounce_ms: 80,
        }
    }
}

pub fn handle_client_connection(
    mut stream: TcpStream,
    bridge: &LiveReloadBridge,
    config: &DevServerConfig,
) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
        return;
    }

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }
    let method = parts[0];
    let raw_path = parts[1];
    let path = raw_path.split('?').next().unwrap_or("/");

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if let Some(idx) = line.find(':') {
            let k = line[..idx].trim().to_lowercase();
            let v = line[idx + 1..].trim().to_string();
            headers.insert(k, v);
        }
    }

    // A. WebSocket Upgrade on /_ventanext/reload
    if path == "/_ventanext/reload" || path == "/ventanext-live-reload" {
        let is_upgrade = headers
            .get("upgrade")
            .map(|v| v.to_lowercase() == "websocket")
            .unwrap_or(false);

        if is_upgrade {
            if let Some(sec_key) = headers.get("sec-websocket-key") {
                let accept_key = compute_websocket_accept(sec_key);
                let response = format!(
                    "HTTP/1.1 101 Switching Protocols\r\n\
                    Upgrade: websocket\r\n\
                    Connection: Upgrade\r\n\
                    Sec-WebSocket-Accept: {}\r\n\r\n",
                    accept_key
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
                bridge.add_websocket_client(stream);
                return;
            }
        }

        // B. Fallback Server-Sent Events (SSE) on /_ventanext/reload
        let sse_header = "HTTP/1.1 200 OK\r\n\
            Content-Type: text/event-stream\r\n\
            Cache-Control: no-cache\r\n\
            Connection: keep-alive\r\n\
            Access-Control-Allow-Origin: *\r\n\r\n";
        let _ = stream.write_all(sse_header.as_bytes());
        let _ = stream.flush();
        bridge.add_sse_client(stream);
        return;
    }

    // C. Dev Server Status Endpoint
    if path == "/_ventanext/status" {
        let json = format!(
            "{{\"status\":\"active\",\"bridge\":\"websocket/sse\",\"port\":{},\"clients\":{},\"last_rebuild_ms\":{}}}",
            config.port,
            bridge.client_count(),
            bridge.last_rebuild_ms()
        );
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            json.len(),
            json
        );
        let _ = stream.write_all(resp.as_bytes());
        return;
    }

    // D. Manual Trigger Endpoint
    if path == "/_ventanext/trigger-reload" && method == "POST" {
        let evt = ReloadEvent::reload("manual-trigger", 4);
        bridge.broadcast(&evt);
        let json = "{\"status\":\"ok\",\"reloaded\":true}";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            json.len(),
            json
        );
        let _ = stream.write_all(resp.as_bytes());
        return;
    }

    // E. Default Project Preview & Pages with Live-Reload Script Injection
    let app_dir = if config.project_dir.join("app").exists() {
        config.project_dir.join("app")
    } else {
        config.project_dir.clone()
    };

    let page_file = if path == "/" {
        app_dir.join("page.va")
    } else {
        app_dir.join(path.trim_start_matches('/')).join("page.va")
    };

    if page_file.exists() {
        let raw_src = fs::read_to_string(&page_file).unwrap_or_default();
        let html_content = format!(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>VentaNext Dev Viewport</title></head><body><h1>VentaNext Running on http://{}:{}</h1><pre>{}</pre></body></html>",
            config.host, config.port, raw_src
        );
        let injected = inject_live_reload_script(&html_content);
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            injected.len(),
            injected
        );
        let _ = stream.write_all(resp.as_bytes());
        return;
    }

    // F. Fallback 404
    let not_found = format!(
        "<!DOCTYPE html><html><body><h2>404 — Not Found in Dev Server</h2><p>Route {} not found in {:?}</p></body></html>",
        path, app_dir
    );
    let injected = inject_live_reload_script(&not_found);
    let resp = format!(
        "HTTP/1.1 404 Not Found\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
        injected.len(),
        injected
    );
    let _ = stream.write_all(resp.as_bytes());
}

// ----------------------------------------------------------------------------
// 7. Dev Server Runner
// ----------------------------------------------------------------------------

pub fn run_development_server(config: DevServerConfig, serve_once: bool) -> std::io::Result<()> {
    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr)?;
    let bridge = LiveReloadBridge::new(config.port);
    let stop_signal = Arc::new(AtomicBool::new(false));

    let watch_dir = config.project_dir.clone();
    let watcher_handle = start_file_watcher(
        watch_dir,
        config.debounce_ms,
        bridge.clone(),
        stop_signal.clone(),
    );

    println!(
        "================================================================================"
    );
    println!(
        "   ⚡ VentaNext Framework — Development Server & Live Reload Bridge"
    );
    println!(
        "================================================================================"
    );
    println!("• Local Viewport:       http://{}/", addr);
    println!("• Live Reload Bridge:   Active (ws://{}/_ventanext/reload + SSE)", addr);
    println!("• Status Endpoint:      http://{}/_ventanext/status", addr);
    println!("• Watching Root:        {:?}", config.project_dir);
    println!(
        "================================================================================"
    );

    if serve_once {
        listener.set_nonblocking(true)?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(1500) {
            if let Ok((stream, _)) = listener.accept() {
                handle_client_connection(stream, &bridge, &config);
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        stop_signal.store(true, Ordering::Relaxed);
        let _ = watcher_handle.join();
        return Ok(());
    }

    for stream_res in listener.incoming() {
        if let Ok(stream) = stream_res {
            let b_clone = bridge.clone();
            let c_clone = DevServerConfig {
                host: config.host.clone(),
                port: config.port,
                project_dir: config.project_dir.clone(),
                debounce_ms: config.debounce_ms,
            };
            thread::spawn(move || {
                handle_client_connection(stream, &b_clone, &c_clone);
            });
        }
    }

    stop_signal.store(true, Ordering::Relaxed);
    let _ = watcher_handle.join();
    Ok(())
}

// ----------------------------------------------------------------------------
// 8. Unit Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn to_hex(data: &[u8]) -> String {
        data.iter().map(|b| format!("{:02x}", b)).collect()
    }

    #[test]
    fn test_sha1_known_vectors() {
        let empty_hash = compute_sha1(b"");
        assert_eq!(
            to_hex(&empty_hash),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );

        let fox_hash = compute_sha1(b"The quick brown fox jumps over the lazy dog");
        assert_eq!(
            to_hex(&fox_hash),
            "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"
        );
    }

    #[test]
    fn test_base64_encoding() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"foob"), "Zm9vYg==");
        assert_eq!(encode_base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn test_rfc6455_websocket_accept_canonical_vector() {
        // Canonical test vector from RFC 6455 Section 1.3 / 4.2.2
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let accept = compute_websocket_accept(key);
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn test_websocket_text_frame_encoding() {
        let frame = encode_websocket_text_frame("hello");
        assert_eq!(frame[0], 0x81); // FIN + Text
        assert_eq!(frame[1], 5);    // Payload len
        assert_eq!(&frame[2..], b"hello");

        // Medium frame (> 125 bytes)
        let medium_payload = "a".repeat(200);
        let m_frame = encode_websocket_text_frame(&medium_payload);
        assert_eq!(m_frame[0], 0x81);
        assert_eq!(m_frame[1], 126);
        assert_eq!(u16::from_be_bytes([m_frame[2], m_frame[3]]), 200);
        assert_eq!(&m_frame[4..], medium_payload.as_bytes());
    }

    #[test]
    fn test_sse_event_formatting() {
        let evt = ReloadEvent::reload("app/page.va", 16);
        let sse = String::from_utf8(evt.to_sse_bytes()).unwrap();
        assert!(sse.starts_with("data: "));
        assert!(sse.ends_with("\n\n"));
        assert!(sse.contains("\"type\":\"reload\""));
        assert!(sse.contains("\"file\":\"app/page.va\""));
        assert!(sse.contains("\"rebuild_ms\":16"));
    }

    #[test]
    fn test_live_reload_script_injection() {
        let original = "<!DOCTYPE html><html><body><h1>Hello</h1></body></html>";
        let injected = inject_live_reload_script(original);
        assert!(injected.contains("id=\"ventanext-live-reload\""));
        assert!(injected.contains("window.__VENTANEXT_HOT_RELOAD__"));
        assert!(injected.contains("<h1>Hello</h1>"));
        assert!(injected.ends_with("</body></html>"));

        // Idempotency: second injection does nothing
        let double_injected = inject_live_reload_script(&injected);
        assert_eq!(injected, double_injected);
    }
}
