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
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let padded_len = ((data.len() + 8) / 64 + 1) * 64;
    let mut msg = Vec::with_capacity(padded_len);
    msg.extend_from_slice(data);
    msg.push(0x80);
    let zeros = (56 + 64 - (msg.len() % 64)) % 64;
    msg.resize(msg.len() + zeros, 0);
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

        for (i, &w_val) in w.iter().enumerate() {
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
                .wrapping_add(w_val);
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
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 {
            chunk[1] as usize
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            chunk[2] as usize
        } else {
            0
        };

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

// ----------------------------------------------------------------------------
// RFC 6455 WebSocket Frames: Encoding, Decoding, and Masking
// ----------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WebSocketOpcode {
    Continuation = 0x0,
    Text = 0x1,
    Binary = 0x2,
    Close = 0x8,
    Ping = 0x9,
    Pong = 0xA,
}

impl WebSocketOpcode {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x0 => Some(WebSocketOpcode::Continuation),
            0x1 => Some(WebSocketOpcode::Text),
            0x2 => Some(WebSocketOpcode::Binary),
            0x8 => Some(WebSocketOpcode::Close),
            0x9 => Some(WebSocketOpcode::Ping),
            0xA => Some(WebSocketOpcode::Pong),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct WebSocketFrame {
    pub fin: bool,
    pub opcode: WebSocketOpcode,
    pub payload: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WebSocketError {
    InvalidOpcode(u8),
    ProtocolViolation(String),
}

impl std::fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebSocketError::InvalidOpcode(op) => write!(f, "Invalid WebSocket opcode: 0x{:X}", op),
            WebSocketError::ProtocolViolation(msg) => {
                write!(f, "WebSocket protocol violation: {}", msg)
            }
        }
    }
}

impl std::error::Error for WebSocketError {}

/// Encode an RFC 6455 WebSocket frame with optional masking
pub fn encode_websocket_frame(
    fin: bool,
    opcode: WebSocketOpcode,
    mask: Option<[u8; 4]>,
    payload: &[u8],
) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 14);

    let b0 = (if fin { 0x80 } else { 0x00 }) | (opcode as u8);
    frame.push(b0);

    let mask_bit = if mask.is_some() { 0x80 } else { 0x00 };
    let len = payload.len();

    if len <= 125 {
        frame.push(mask_bit | (len as u8));
    } else if len <= 65535 {
        frame.push(mask_bit | 126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(mask_bit | 127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }

    if let Some(mask_key) = mask {
        frame.extend_from_slice(&mask_key);
        for (i, &b) in payload.iter().enumerate() {
            frame.push(b ^ mask_key[i % 4]);
        }
    } else {
        frame.extend_from_slice(payload);
    }

    frame
}

/// Decode a raw RFC 6455 WebSocket frame from a byte buffer.
/// Returns Ok(Some((frame, bytes_consumed))) on full frame,
/// Ok(None) if buffer is incomplete, or Err(WebSocketError).
pub fn decode_websocket_frame(
    data: &[u8],
) -> Result<Option<(WebSocketFrame, usize)>, WebSocketError> {
    if data.len() < 2 {
        return Ok(None);
    }

    let fin = (data[0] & 0x80) != 0;
    let rsv = data[0] & 0x70;
    if rsv != 0 {
        return Err(WebSocketError::ProtocolViolation(
            "RSV bits must be 0 when no extensions negotiated".to_string(),
        ));
    }

    let opcode_raw = data[0] & 0x0F;
    let opcode =
        WebSocketOpcode::from_u8(opcode_raw).ok_or(WebSocketError::InvalidOpcode(opcode_raw))?;

    let masked = (data[1] & 0x80) != 0;
    let len_indicator = data[1] & 0x7F;

    let mut offset = 2;
    let payload_len: usize = match len_indicator {
        0..=125 => len_indicator as usize,
        126 => {
            if data.len() < offset + 2 {
                return Ok(None);
            }
            let l = u16::from_be_bytes([data[offset], data[offset + 1]]) as usize;
            offset += 2;
            l
        }
        127 => {
            if data.len() < offset + 8 {
                return Ok(None);
            }
            let l = u64::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]) as usize;
            offset += 8;
            l
        }
        _ => unreachable!(),
    };

    if matches!(
        opcode,
        WebSocketOpcode::Close | WebSocketOpcode::Ping | WebSocketOpcode::Pong
    ) && (payload_len > 125 || !fin)
    {
        return Err(WebSocketError::ProtocolViolation(
            "Control frame cannot exceed 125 bytes or be fragmented".to_string(),
        ));
    }

    let mask = if masked {
        if data.len() < offset + 4 {
            return Ok(None);
        }
        let m = [
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ];
        offset += 4;
        Some(m)
    } else {
        None
    };

    if data.len() < offset + payload_len {
        return Ok(None);
    }

    let mut payload = data[offset..offset + payload_len].to_vec();
    if let Some(mask_key) = mask {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask_key[i % 4];
        }
    }

    let consumed = offset + payload_len;
    Ok(Some((
        WebSocketFrame {
            fin,
            opcode,
            payload,
        },
        consumed,
    )))
}

/// Encode RFC 6455 unmasked text frame
pub fn encode_websocket_text_frame(payload: &str) -> Vec<u8> {
    encode_websocket_frame(true, WebSocketOpcode::Text, None, payload.as_bytes())
}

/// Encode RFC 6455 unmasked binary frame
pub fn encode_websocket_binary_frame(payload: &[u8]) -> Vec<u8> {
    encode_websocket_frame(true, WebSocketOpcode::Binary, None, payload)
}

/// Encode RFC 6455 unmasked ping frame
pub fn encode_websocket_ping_frame(payload: &[u8]) -> Vec<u8> {
    encode_websocket_frame(true, WebSocketOpcode::Ping, None, payload)
}

/// Encode RFC 6455 unmasked pong frame
pub fn encode_websocket_pong_frame(payload: &[u8]) -> Vec<u8> {
    encode_websocket_frame(true, WebSocketOpcode::Pong, None, payload)
}

/// Encode RFC 6455 unmasked close frame
pub fn encode_websocket_close_frame(code: u16, reason: &str) -> Vec<u8> {
    let mut payload = Vec::with_capacity(2 + reason.len());
    payload.extend_from_slice(&code.to_be_bytes());
    payload.extend_from_slice(reason.as_bytes());
    encode_websocket_frame(true, WebSocketOpcode::Close, None, &payload)
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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ClientKind {
    WebSocket,
    ServerSentEvents,
}

pub struct ActiveClient {
    pub stream: TcpStream,
    pub kind: ClientKind,
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
        let _ = stream.set_nodelay(true);
        let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));

        let conn_evt = ReloadEvent::connected(self.port);
        let frame = encode_websocket_text_frame(&conn_evt.to_json());
        let _ = stream.write_all(&frame);
        let _ = stream.flush();

        let read_stream = match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        };

        if let Ok(mut list) = self.clients.lock() {
            list.push(ActiveClient {
                stream,
                kind: ClientKind::WebSocket,
            });
        }

        // Spawn client frame listener (handles Ping/Pong, Close, and client messages)
        thread::spawn(move || {
            let mut reader = read_stream;
            let mut buf = [0u8; 4096];
            let mut pending = Vec::new();

            loop {
                match std::io::Read::read(&mut reader, &mut buf) {
                    Ok(0) => break, // EOF, client disconnected
                    Ok(n) => {
                        pending.extend_from_slice(&buf[..n]);
                        while !pending.is_empty() {
                            match decode_websocket_frame(&pending) {
                                Ok(Some((frame, consumed))) => {
                                    pending.drain(..consumed);
                                    match frame.opcode {
                                        WebSocketOpcode::Ping => {
                                            let pong = encode_websocket_pong_frame(&frame.payload);
                                            let _ = reader.write_all(&pong);
                                            let _ = reader.flush();
                                        }
                                        WebSocketOpcode::Close => {
                                            let close_resp = encode_websocket_close_frame(
                                                1000,
                                                "Normal Closure",
                                            );
                                            let _ = reader.write_all(&close_resp);
                                            let _ = reader.flush();
                                            return;
                                        }
                                        _ => {}
                                    }
                                }
                                Ok(None) => break, // Incomplete frame
                                Err(_) => return,  // Framing / protocol violation
                            }
                        }
                    }
                    Err(_) => break, // Socket error or closed
                }
            }
        });
    }

    pub fn add_sse_client(&self, mut stream: TcpStream) {
        let _ = stream.set_nodelay(true);
        let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));

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

        let clients = if let Ok(mut list) = self.clients.lock() {
            std::mem::take(&mut *list)
        } else {
            Vec::new()
        };

        let mut retained = Vec::with_capacity(clients.len());
        for mut client in clients {
            let res = match client.kind {
                ClientKind::WebSocket => client.stream.write_all(&ws_frame),
                ClientKind::ServerSentEvents => client.stream.write_all(&sse_bytes),
            };
            if res.is_ok() && client.stream.flush().is_ok() {
                retained.push(client);
            }
        }

        if let Ok(mut list) = self.clients.lock() {
            retained.extend(list.drain(..));
            *list = retained;
        }
    }

    pub fn client_count(&self) -> usize {
        self.clients.lock().map(|l| l.len()).unwrap_or(0)
    }

    pub fn last_rebuild_ms(&self) -> u64 {
        self.last_rebuild_ms.load(Ordering::SeqCst)
    }

    pub fn has_clients(&self) -> bool {
        self.client_count() > 0
    }

    pub fn clear_clients(&self) {
        if let Ok(mut list) = self.clients.lock() {
            list.clear();
        }
    }
}

// ----------------------------------------------------------------------------
// 4. File Watcher & Change Detector
// ----------------------------------------------------------------------------

const IGNORED_DIRS: &[&str] = &[
    "target",
    "out",
    "dist",
    "node_modules",
    ".git",
    ".cargo",
    ".ventanext",
];

pub fn scan_directory_mtimes(dir: &Path, mtimes: &mut HashMap<PathBuf, SystemTime>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with('.') && !IGNORED_DIRS.contains(&name) {
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
        let mut current: HashMap<PathBuf, SystemTime> = HashMap::new();
        scan_directory_mtimes(&watch_root, &mut snapshot);

        while !stop_signal.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(debounce_ms));

            current.clear();
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
                std::mem::swap(&mut snapshot, &mut current);

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
    let clone = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let _ = clone.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(clone);
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
        if reader.read_line(&mut line).is_err() || line == "\r\n" || line == "\n" || line.is_empty()
        {
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
                let _ = stream.set_read_timeout(None);
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
        let _ = stream.set_read_timeout(None);
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

    println!("================================================================================");
    println!("   ⚡ VentaNext Framework — Development Server & Live Reload Bridge");
    println!("================================================================================");
    println!("• Local Viewport:       http://{}/", addr);
    println!(
        "• Live Reload Bridge:   Active (ws://{}/_ventanext/reload + SSE)",
        addr
    );
    println!("• Status Endpoint:      http://{}/_ventanext/status", addr);
    println!("• Watching Root:        {:?}", config.project_dir);
    println!("================================================================================");

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

    for stream in listener.incoming().flatten() {
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

    stop_signal.store(true, Ordering::Relaxed);
    let _ = watcher_handle.join();
    Ok(())
}

/// Start dev server listening on a bound `TcpListener` in a background thread.
/// Returns the LiveReloadBridge and a JoinHandle to control the thread lifecycle.
pub fn start_dev_server_background(
    listener: TcpListener,
    config: DevServerConfig,
    stop_signal: Arc<AtomicBool>,
) -> (LiveReloadBridge, thread::JoinHandle<()>) {
    let port = listener
        .local_addr()
        .map(|a| a.port())
        .unwrap_or(config.port);
    let bridge = LiveReloadBridge::new(port);
    let b_clone = bridge.clone();
    let stop_clone = stop_signal.clone();

    let handle = thread::spawn(move || {
        let _ = listener.set_nonblocking(true);
        while !stop_clone.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let b = b_clone.clone();
                    let c = DevServerConfig {
                        host: config.host.clone(),
                        port,
                        project_dir: config.project_dir.clone(),
                        debounce_ms: config.debounce_ms,
                    };
                    thread::spawn(move || {
                        handle_client_connection(stream, &b, &c);
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    (bridge, handle)
}

// ----------------------------------------------------------------------------
// 8. Unit Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

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
        assert_eq!(frame[1], 5); // Payload len
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

    #[test]
    fn test_rfc6455_decode_unmasked_text_frame() {
        // Canonical unmasked "Hello" frame from RFC 6455 Section 5.7
        let raw = [0x81, 0x05, 0x48, 0x65, 0x6c, 0x6c, 0x6f];
        let res = decode_websocket_frame(&raw).expect("decode failed");
        assert!(res.is_some());
        let (frame, consumed) = res.unwrap();
        assert_eq!(consumed, 7);
        assert!(frame.fin);
        assert_eq!(frame.opcode, WebSocketOpcode::Text);
        assert_eq!(frame.payload, b"Hello");
    }

    #[test]
    fn test_rfc6455_decode_masked_text_frame() {
        // Canonical masked "Hello" frame from RFC 6455 Section 5.7
        // Mask key: 0x37 0xfa 0x21 0x3d, masked payload: 0x7f 0x9f 0x4d 0x51 0x58
        let raw = [
            0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58,
        ];
        let res = decode_websocket_frame(&raw).expect("decode failed");
        assert!(res.is_some());
        let (frame, consumed) = res.unwrap();
        assert_eq!(consumed, 11);
        assert!(frame.fin);
        assert_eq!(frame.opcode, WebSocketOpcode::Text);
        assert_eq!(frame.payload, b"Hello");
    }

    #[test]
    fn test_rfc6455_control_frames_ping_pong_close() {
        // Ping frame with 4-byte payload
        let ping_frame = encode_websocket_ping_frame(b"ping");
        let (decoded_ping, _) = decode_websocket_frame(&ping_frame)
            .unwrap()
            .expect("ping decode");
        assert_eq!(decoded_ping.opcode, WebSocketOpcode::Ping);
        assert_eq!(decoded_ping.payload, b"ping");

        // Pong frame response
        let pong_frame = encode_websocket_pong_frame(&decoded_ping.payload);
        let (decoded_pong, _) = decode_websocket_frame(&pong_frame)
            .unwrap()
            .expect("pong decode");
        assert_eq!(decoded_pong.opcode, WebSocketOpcode::Pong);
        assert_eq!(decoded_pong.payload, b"ping");

        // Close frame
        let close_frame = encode_websocket_close_frame(1000, "Normal Closure");
        let (decoded_close, _) = decode_websocket_frame(&close_frame)
            .unwrap()
            .expect("close decode");
        assert_eq!(decoded_close.opcode, WebSocketOpcode::Close);
        assert_eq!(
            u16::from_be_bytes([decoded_close.payload[0], decoded_close.payload[1]]),
            1000
        );
        assert_eq!(&decoded_close.payload[2..], b"Normal Closure");
    }

    #[test]
    fn test_rfc6455_partial_frame_streaming() {
        let full = encode_websocket_text_frame("streaming partial frame test");
        // Only 3 bytes provided
        assert!(decode_websocket_frame(&full[..3]).unwrap().is_none());
        // Half provided
        assert!(decode_websocket_frame(&full[..full.len() / 2])
            .unwrap()
            .is_none());
        // All provided
        let (frame, consumed) = decode_websocket_frame(&full).unwrap().unwrap();
        assert_eq!(consumed, full.len());
        assert_eq!(frame.payload, b"streaming partial frame test");
    }

    #[test]
    fn test_rfc6455_16bit_extended_length() {
        let large_payload = vec![0x42; 600];
        let mask = Some([0x12, 0x34, 0x56, 0x78]);
        let encoded = encode_websocket_frame(true, WebSocketOpcode::Binary, mask, &large_payload);

        let (frame, consumed) = decode_websocket_frame(&encoded).unwrap().unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(frame.opcode, WebSocketOpcode::Binary);
        assert_eq!(frame.payload, large_payload);
    }

    #[test]
    fn test_live_reload_bridge_lifecycle_and_pruning() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let bridge = LiveReloadBridge::new(port);

        // Connect client 1 (WebSocket)
        let c1_stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let (s1, _) = listener.accept().unwrap();
        bridge.add_websocket_client(s1);

        // Connect client 2 (SSE)
        let c2_stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let (s2, _) = listener.accept().unwrap();
        bridge.add_sse_client(s2);

        assert_eq!(bridge.client_count(), 2);
        assert!(bridge.has_clients());

        // Broadcast event
        let evt = ReloadEvent::reload("test.va", 12);
        bridge.broadcast(&evt);
        assert_eq!(bridge.last_rebuild_ms(), 12);
        assert_eq!(bridge.client_count(), 2);

        // Drop client 1 connection
        drop(c1_stream);

        // Next broadcast prunes dead client 1
        let evt2 = ReloadEvent::reload("test2.va", 8);
        bridge.broadcast(&evt2);
        assert_eq!(bridge.client_count(), 1);

        drop(c2_stream);
        bridge.clear_clients();
        assert_eq!(bridge.client_count(), 0);
        assert!(!bridge.has_clients());
    }

    #[test]
    fn test_dev_server_background_http_endpoints() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop_signal = Arc::new(AtomicBool::new(false));

        let config = DevServerConfig {
            host: "127.0.0.1".to_string(),
            port,
            project_dir: PathBuf::from("."),
            debounce_ms: 100,
        };

        let (_bridge, handle) = start_dev_server_background(listener, config, stop_signal.clone());

        // Test GET /_ventanext/status
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream
            .write_all(b"GET /_ventanext/status HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader.read_line(&mut status_line).unwrap();
        assert!(status_line.contains("200 OK"));

        // Read headers
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        let mut body = String::new();
        reader.read_line(&mut body).unwrap();
        assert!(body.contains("\"status\":\"active\""));
        assert!(body.contains(&format!("\"port\":{}", port)));

        // Test POST /_ventanext/trigger-reload
        let mut stream_trigger = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream_trigger
            .write_all(b"POST /_ventanext/trigger-reload HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        let mut reader_trigger = BufReader::new(stream_trigger);
        let mut trig_status = String::new();
        reader_trigger.read_line(&mut trig_status).unwrap();
        assert!(trig_status.contains("200 OK"));

        // Test GET non-existent route 404
        let mut stream_404 = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream_404
            .write_all(b"GET /non-existent-xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .unwrap();
        let mut reader_404 = BufReader::new(stream_404);
        let mut not_found_status = String::new();
        reader_404.read_line(&mut not_found_status).unwrap();
        assert!(not_found_status.contains("404 Not Found"));

        // Shut down server
        stop_signal.store(true, Ordering::Relaxed);
        let _ = handle.join();
    }

    #[test]
    fn test_dev_server_websocket_upgrade_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop_signal = Arc::new(AtomicBool::new(false));

        let config = DevServerConfig {
            host: "127.0.0.1".to_string(),
            port,
            project_dir: PathBuf::from("."),
            debounce_ms: 100,
        };

        let (bridge, handle) = start_dev_server_background(listener, config, stop_signal.clone());

        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let req = format!(
            "GET /_ventanext/reload HTTP/1.1\r\n\
            Host: 127.0.0.1:{}\r\n\
            Upgrade: websocket\r\n\
            Connection: Upgrade\r\n\
            Sec-WebSocket-Key: {}\r\n\
            Sec-WebSocket-Version: 13\r\n\r\n",
            port, key
        );
        stream.write_all(req.as_bytes()).unwrap();

        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut status_line = String::new();
        reader.read_line(&mut status_line).unwrap();
        assert!(status_line.contains("101 Switching Protocols"));

        let mut accept_found = false;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.to_lowercase().starts_with("sec-websocket-accept:") {
                assert!(line.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
                accept_found = true;
            }
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        assert!(accept_found);

        // Give the background thread time to register the client
        thread::sleep(Duration::from_millis(50));
        assert!(bridge.client_count() >= 1);

        // Read the initial 'connected' frame sent on upgrade
        let mut initial_buf = [0u8; 512];
        let n = reader.get_mut().read(&mut initial_buf).unwrap();
        let (conn_frame, _) = decode_websocket_frame(&initial_buf[..n])
            .unwrap()
            .expect("initial connected frame");
        let conn_json: serde_json::Value = serde_json::from_slice(&conn_frame.payload).unwrap();
        assert_eq!(conn_json["type"], "connected");

        // Broadcast a reload event and verify client receives it over WebSocket
        let evt = ReloadEvent::reload("components/header.va", 42);
        bridge.broadcast(&evt);

        let mut reload_buf = [0u8; 512];
        let n_reload = reader.get_mut().read(&mut reload_buf).unwrap();
        let (reload_frame, _) = decode_websocket_frame(&reload_buf[..n_reload])
            .unwrap()
            .expect("broadcast reload frame");
        let reload_json: serde_json::Value = serde_json::from_slice(&reload_frame.payload).unwrap();
        assert_eq!(reload_json["type"], "reload");
        assert_eq!(reload_json["file"], "components/header.va");
        assert_eq!(reload_json["rebuild_ms"], 42);

        // Send a masked client Ping frame to server and receive Pong
        let ping_payload = b"ping_test_data";
        let mask = Some([0xAA, 0xBB, 0xCC, 0xDD]);
        let ping_frame = encode_websocket_frame(true, WebSocketOpcode::Ping, mask, ping_payload);
        stream.write_all(&ping_frame).unwrap();

        let mut pong_buf = [0u8; 256];
        let n_pong = reader.get_mut().read(&mut pong_buf).unwrap();
        let (pong_frame, _) = decode_websocket_frame(&pong_buf[..n_pong])
            .unwrap()
            .expect("pong frame response");
        assert_eq!(pong_frame.opcode, WebSocketOpcode::Pong);
        assert_eq!(pong_frame.payload, ping_payload);

        stop_signal.store(true, Ordering::Relaxed);
        let _ = handle.join();
    }
}
