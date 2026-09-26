//! Overlay GL-hook support: localhost JSON proxy + DLL inject into the game process.
//!
//! Env for the JVM (set before spawn):
//! - `TUFFBOX_OVERLAY_IPC=127.0.0.1:{port}`
//! - `TUFFBOX_OVERLAY_SESSION=<path to overlay-session.json>`

use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::json;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

static IPC_PORT: AtomicU16 = AtomicU16::new(0);
static IPC_RUNNING: AtomicBool = AtomicBool::new(false);
static SESSION_PATH: Lazy<Mutex<Option<PathBuf>>> = Lazy::new(|| Mutex::new(None));

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverlaySession {
    username: String,
    uuid: String,
    api_base: String,
    anon_key: String,
    #[serde(default)]
    write_secret: String,
    #[serde(default)]
    pack_name: String,
}

fn load_session() -> Result<OverlaySession, String> {
    let path = SESSION_PATH
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "overlay session path not set".to_string())?;
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("read session: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse session: {e}"))
}

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

fn edge_url(base: &str, name: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/functions/v1/{name}")
}

fn rest_url(base: &str, path_and_query: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/rest/v1/{path_and_query}")
}

fn handle_session() -> Result<serde_json::Value, String> {
    let s = load_session()?;
    Ok(json!({
        "username": s.username,
        "uuid": s.uuid,
        "packName": s.pack_name,
        "apiBase": s.api_base,
    }))
}

fn handle_youtube_feed() -> Result<serde_json::Value, String> {
    let s = load_session()?;
    let client = http_client()?;
    let url = rest_url(
        &s.api_base,
        "youtube_feed?select=video_id,title,thumbnail_url,channel_name,source,lang,view_count,published_at&order=view_count.desc&limit=40",
    );
    let resp = client
        .get(&url)
        .header("apikey", &s.anon_key)
        .header("Authorization", format!("Bearer {}", s.anon_key))
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("youtube_feed {status}: {body}"));
    }
    // Normalize for the hook UI.
    let items = body
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|row| {
            json!({
                "id": row.get("video_id").cloned().unwrap_or(json!("")),
                "title": row.get("title").cloned().unwrap_or(json!("")),
                "thumbnail_url": row.get("thumbnail_url").cloned().unwrap_or(json!("")),
                "channel": row.get("channel_name").cloned().unwrap_or(json!("")),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "items": items }))
}

fn handle_friends() -> Result<serde_json::Value, String> {
    let s = load_session()?;
    let client = http_client()?;
    let url = edge_url(&s.api_base, "overlay-friends");
    let resp = client
        .post(&url)
        .header("apikey", &s.anon_key)
        .header("Authorization", format!("Bearer {}", s.anon_key))
        .header("Content-Type", "application/json")
        .json(&json!({
            "action": "list",
            "playerKey": s.uuid,
            "username": s.username,
            "writeSecret": s.write_secret,
        }))
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().unwrap_or_else(|_| json!({ "error": "bad json" }));
    if !status.is_success() {
        return Err(format!("friends {status}: {body}"));
    }
    Ok(body)
}

/// Proxy friends mutations (add / accept / remove) to the edge function.
fn handle_friends_action(body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let s = load_session()?;
    let client = http_client()?;
    let url = edge_url(&s.api_base, "overlay-friends");
    let action = body
        .get("action")
        .and_then(|a| a.as_str())
        .unwrap_or("")
        .to_string();
    if action.is_empty() {
        return Err("action required".into());
    }
    let mut payload = json!({
        "action": action,
        "playerKey": s.uuid,
        "username": s.username,
        "writeSecret": s.write_secret,
    });
    if let Some(name) = body.get("friendUsername").and_then(|v| v.as_str()) {
        payload["friendUsername"] = json!(name);
    }
    if let Some(id) = body.get("friendshipId").and_then(|v| v.as_i64()) {
        payload["friendshipId"] = json!(id);
    }
    let resp = client
        .post(&url)
        .header("apikey", &s.anon_key)
        .header("Authorization", format!("Bearer {}", s.anon_key))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let out: serde_json::Value = resp.json().unwrap_or_else(|_| json!({ "error": "bad json" }));
    if !status.is_success() {
        // Surface edge error body to the hook (404 player not found, etc.).
        return Ok(out);
    }
    Ok(out)
}

fn handle_chat_poll(since_id: i64) -> Result<serde_json::Value, String> {
    let s = load_session()?;
    let client = http_client()?;
    let url = edge_url(&s.api_base, "overlay-chat-poll");
    let payload = json!({
        "playerKey": s.uuid,
        "username": s.username,
        "writeSecret": s.write_secret,
        "sinceId": since_id,
    });
    let resp = client
        .post(&url)
        .header("apikey", &s.anon_key)
        .header("Authorization", format!("Bearer {}", s.anon_key))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().unwrap_or_else(|_| json!({ "error": "bad json" }));
    if !status.is_success() {
        return Err(format!("chat-poll {status}: {body}"));
    }
    Ok(body)
}

fn handle_chat_send(body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let s = load_session()?;
    let client = http_client()?;
    let url = edge_url(&s.api_base, "overlay-chat-send");
    let to_key = body
        .get("toKey")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let text = body
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Proxy-side validation (hook already sanitises; this is the trust boundary
    // before we attach writeSecret and hit Supabase).
    if to_key.len() < 8
        || to_key.len() > 64
        || !to_key
            .bytes()
            .all(|b| b.is_ascii_hexdigit() || b == b'-')
    {
        return Err("invalid toKey".into());
    }
    let text = sanitize_chat_body(&text)?;
    let payload = json!({
        "playerKey": s.uuid,
        "username": s.username,
        "writeSecret": s.write_secret,
        "toKey": to_key,
        "body": text,
    });
    let resp = client
        .post(&url)
        .header("apikey", &s.anon_key)
        .header("Authorization", format!("Bearer {}", s.anon_key))
        // Explicit UTF-8 charset so intermediate proxies don't mangle emoji.
        .header("Content-Type", "application/json; charset=utf-8")
        .json(&payload)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let out: serde_json::Value = resp.json().unwrap_or_else(|_| json!({ "error": "bad json" }));
    if !status.is_success() {
        return Ok(out);
    }
    Ok(out)
}

/// Mirror of the hook's chat sanitiser — keep the two in sync.
fn sanitize_chat_body(input: &str) -> Result<String, String> {
    const MAX_CHARS: usize = 500;
    const MAX_BYTES: usize = 2000;
    let mut out = String::with_capacity(input.len().min(MAX_BYTES));
    let mut bytes = 0usize;
    let mut chars = 0usize;
    for ch in input.chars() {
        if chars >= MAX_CHARS || bytes >= MAX_BYTES {
            break;
        }
        let ch = if matches!(ch, '\n' | '\r' | '\t') {
            ' '
        } else {
            ch
        };
        let u = ch as u32;
        // C0/C1 controls, bidi overrides, deprecated format, tags, soft hyphen, BOM.
        let bad = matches!(
            ch,
            '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
                | '\u{200E}'
                | '\u{200F}'
                | '\u{061C}'
                | '\u{200B}'
                | '\u{200C}'
                | '\u{2060}'
                | '\u{FEFF}'
                | '\u{00AD}'
                | '\u{180E}'
                | '\u{206A}'..='\u{206F}'
        ) || u < 0x20
            || (0x7F..=0x9F).contains(&u)
            || (0xE0000..=0xE007F).contains(&u)
            || (0xE000..=0xF8FF).contains(&u);
        if bad {
            continue;
        }
        let mut buf = [0u8; 4];
        let enc = ch.encode_utf8(&mut buf);
        if bytes + enc.len() > MAX_BYTES {
            break;
        }
        out.push_str(enc);
        bytes += enc.len();
        chars += 1;
    }
    // Collapse whitespace.
    let cleaned = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        return Err("body empty after sanitise".into());
    }
    Ok(cleaned)
}

/// Presence heartbeat — marks us online and returns friends' live presence.
fn handle_presence() -> Result<serde_json::Value, String> {
    let s = load_session()?;
    let client = http_client()?;
    let url = edge_url(&s.api_base, "overlay-presence");
    let payload = json!({
        "playerKey": s.uuid,
        "username": s.username,
        "writeSecret": s.write_secret,
        "packName": s.pack_name,
        "server": "",
        "offline": false,
    });
    let resp = client
        .post(&url)
        .header("apikey", &s.anon_key)
        .header("Authorization", format!("Bearer {}", s.anon_key))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().unwrap_or_else(|_| json!({ "error": "bad json" }));
    if !status.is_success() {
        return Err(format!("presence {status}: {body}"));
    }
    Ok(body)
}

fn handle_youtube_resolve(id: &str) -> Result<serde_json::Value, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("id required".into());
    }
    // Hook plays via libmpv with a YouTube URL; ytdl inside mpv resolves streams.
    Ok(json!({
        "id": id,
        "url": format!("https://www.youtube.com/watch?v={id}"),
    }))
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}=");
    query.split('&').find_map(|p| p.strip_prefix(&prefix))
}

fn route(
    method: &str,
    path: &str,
    query: &str,
    body: Option<&serde_json::Value>,
) -> (u16, serde_json::Value) {
    if method != "GET" && method != "POST" {
        return (405, json!({ "error": "method not allowed" }));
    }
    let path = path.split('?').next().unwrap_or(path);
    let empty = json!({});
    let body = body.unwrap_or(&empty);
    let result = match (method, path) {
        (_, "/health") => Ok(json!({ "ok": true })),
        (_, "/session") => handle_session(),
        (_, "/youtube-feed") => handle_youtube_feed(),
        ("GET", "/friends") | ("POST", "/friends") => handle_friends(),
        ("POST", "/friends/action") => handle_friends_action(body),
        (_, "/presence") => handle_presence(),
        ("GET", "/chat") | ("GET", "/chat/poll") | ("POST", "/chat") | ("POST", "/chat/poll") => {
            let since = query_param(query, "sinceId")
                .and_then(|s| s.parse::<i64>().ok())
                .or_else(|| body.get("sinceId").and_then(|v| v.as_i64()))
                .unwrap_or(0);
            handle_chat_poll(since)
        }
        ("POST", "/chat/send") => handle_chat_send(body),
        (_, "/youtube-resolve") => {
            let id = query_param(query, "id").unwrap_or("");
            handle_youtube_resolve(id)
        }
        _ => Err(format!("not found: {path}")),
    };
    match result {
        Ok(v) => (200, v),
        Err(e) => (500, json!({ "error": e })),
    }
}

fn parse_http_request(raw: &str) -> (String, String, String, Option<serde_json::Value>) {
    let mut lines = raw.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };

    // Split headers / body on blank line.
    let mut content_length = 0usize;
    let mut in_headers = true;
    let mut body_start = String::new();
    for line in lines {
        if in_headers {
            if line.is_empty() {
                in_headers = false;
                continue;
            }
            if let Some(v) = line
                .split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .map(|(_, v)| v.trim())
            {
                content_length = v.parse().unwrap_or(0);
            }
        } else {
            if !body_start.is_empty() {
                body_start.push_str("\r\n");
            }
            body_start.push_str(line);
        }
    }
    let body = if method == "POST" && content_length > 0 {
        // Body may be truncated if it spanned the initial read; best-effort parse.
        let sliced = if body_start.len() >= content_length {
            &body_start[..content_length]
        } else {
            body_start.as_str()
        };
        serde_json::from_str(sliced).ok()
    } else if method == "POST" && !body_start.trim().is_empty() {
        serde_json::from_str(body_start.trim()).ok()
    } else {
        None
    };
    (method, path, query, body)
}

fn serve_connection(mut stream: std::net::TcpStream) {
    let mut buf = [0u8; 16384];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };
    let req = String::from_utf8_lossy(&buf[..n]);
    let (method, path, query, body) = parse_http_request(&req);
    let (status, resp_body) = route(&method, &path, &query, body.as_ref());
    let body_bytes = serde_json::to_vec(&resp_body).unwrap_or_else(|_| b"{}".to_vec());
    let reason = if status == 200 { "OK" } else { "Error" };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
        body_bytes.len()
    );
    let _ = stream.write_all(&body_bytes);
}

/// Start (or reuse) the overlay IPC server. Returns `127.0.0.1:port`.
pub fn ensure_ipc_server(session_path: &Path) -> Result<String, String> {
    {
        let mut g = SESSION_PATH.lock().map_err(|e| e.to_string())?;
        *g = Some(session_path.to_path_buf());
    }

    let existing = IPC_PORT.load(Ordering::SeqCst);
    if existing != 0 && IPC_RUNNING.load(Ordering::SeqCst) {
        return Ok(format!("127.0.0.1:{existing}"));
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind overlay ipc: {e}"))?;
    listener
        .set_nonblocking(false)
        .map_err(|e| e.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .port();
    IPC_PORT.store(port, Ordering::SeqCst);
    IPC_RUNNING.store(true, Ordering::SeqCst);

    thread::Builder::new()
        .name("tuffbox-overlay-ipc".into())
        .spawn(move || {
            for conn in listener.incoming() {
                if !IPC_RUNNING.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(stream) = conn {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                    // One request per connection; keep it simple for the hook.
                    serve_connection(stream);
                }
            }
            IPC_RUNNING.store(false, Ordering::SeqCst);
        })
        .map_err(|e| format!("spawn ipc thread: {e}"))?;

    Ok(format!("127.0.0.1:{port}"))
}

pub fn stop_ipc_server() {
    let port = IPC_PORT.load(Ordering::SeqCst);
    IPC_RUNNING.store(false, Ordering::SeqCst);
    if port != 0 {
        let _ = std::net::TcpStream::connect_timeout(
            &SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_millis(200),
        );
    }
    IPC_PORT.store(0, Ordering::SeqCst);
}

pub fn ipc_endpoint() -> Option<String> {
    let port = IPC_PORT.load(Ordering::SeqCst);
    if port == 0 {
        None
    } else {
        Some(format!("127.0.0.1:{port}"))
    }
}

/// Locate `tuffbox_overlay_hook.dll` next to the exe / resources / env.
pub fn find_hook_dll() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TUFFBOX_OVERLAY_HOOK_DLL") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
            roots.push(dir.join("resources"));
            roots.push(dir.join("overlay-hook"));
            roots.push(dir.join("binaries"));
        }
    }
    // Dev: workspace bridges/overlay-hook target
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    roots.push(manifest_dir.join("binaries"));
    roots.push(
        manifest_dir
            .join("../../../target/release")
            .canonicalize()
            .unwrap_or_else(|_| manifest_dir.join("../../../target/release")),
    );
    roots.push(
        manifest_dir
            .join("../../../target/debug")
            .canonicalize()
            .unwrap_or_else(|_| manifest_dir.join("../../../target/debug")),
    );
    roots.push(manifest_dir.join("../../../bridges/overlay-hook/target/release"));
    roots.push(manifest_dir.join("../../../bridges/overlay-hook/target/debug"));

    for root in roots {
        for name in [
            "tuffbox_overlay_hook.dll",
            "tuffbox_overlay_hook.dll.dll",
            "overlay_hook.dll",
        ] {
            let p = root.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(windows)]
pub fn inject_hook_dll(pid: u32) -> Result<String, String> {
    let dll = find_hook_dll().ok_or_else(|| {
        "tuffbox_overlay_hook.dll not found — build bridges/overlay-hook (cdylib)".to_string()
    })?;
    inject_dll_loadlibrary(pid, &dll)?;
    Ok(format!("injected {}", dll.display()))
}

#[cfg(not(windows))]
pub fn inject_hook_dll(_pid: u32) -> Result<String, String> {
    Err("overlay GL hook inject is Windows-only".into())
}

#[cfg(windows)]
fn inject_dll_loadlibrary(pid: u32, dll: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::Win32::System::Memory::{
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION,
        PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE, INFINITE,
    };

    type CreateRemoteThreadFn = unsafe extern "system" fn(
        windows::Win32::Foundation::HANDLE,
        *const core::ffi::c_void,
        usize,
        Option<unsafe extern "system" fn(*mut core::ffi::c_void) -> u32>,
        *mut core::ffi::c_void,
        u32,
        *mut u32,
    ) -> windows::Win32::Foundation::HANDLE;

    let wide: Vec<u16> = dll
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let bytes = wide.len() * 2;

    unsafe {
        let process = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_VM_READ,
            false,
            pid,
        )
        .map_err(|e| format!("OpenProcess: {e}"))?;

        let remote = VirtualAllocEx(
            process,
            None,
            bytes,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if remote.is_null() {
            let _ = CloseHandle(process);
            return Err("VirtualAllocEx failed".into());
        }

        let mut written = 0usize;
        if WriteProcessMemory(
            process,
            remote,
            wide.as_ptr() as *const _,
            bytes,
            Some(&mut written),
        )
        .is_err()
            || written != bytes
        {
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            let _ = CloseHandle(process);
            return Err("WriteProcessMemory failed".into());
        }

        let k32 = GetModuleHandleW(PCWSTR(windows::core::w!("kernel32.dll").as_ptr()))
            .map_err(|e| format!("GetModuleHandleW: {e}"))?;
        let load_library = GetProcAddress(k32, PCSTR(b"LoadLibraryW\0".as_ptr()))
            .ok_or_else(|| "LoadLibraryW not found".to_string())?;
        let create_remote = GetProcAddress(k32, PCSTR(b"CreateRemoteThread\0".as_ptr()))
            .ok_or_else(|| "CreateRemoteThread not found".to_string())?;
        let create_remote: CreateRemoteThreadFn = std::mem::transmute(create_remote);

        let thread = create_remote(
            process,
            std::ptr::null(),
            0,
            Some(std::mem::transmute(load_library)),
            remote,
            0,
            std::ptr::null_mut(),
        );
        if thread.is_invalid() {
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            let _ = CloseHandle(process);
            return Err("CreateRemoteThread failed".into());
        }

        let _ = WaitForSingleObject(thread, INFINITE);
        let _ = CloseHandle(thread);
        let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
        let _ = CloseHandle(process);
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayHookLaunchInfo {
    pub ipc: String,
    pub session_path: String,
    pub hook_dll: Option<String>,
}
