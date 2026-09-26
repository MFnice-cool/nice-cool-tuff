//! Optional libmpv integration (dynamic LoadLibrary).
//!
//! Playback path:
//! 1. `vo=libmpv` + OpenGL render context
//! 2. Each `tick()` renders into an offscreen FBO/texture
//! 3. UI / PiP blits that texture into the game framebuffer
//!
//! Fail-soft if `mpv-2.dll` (or friends) is missing — audio-only path is
//! not attempted; callers just get a clear error string.

use crate::gl;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;

type MpvHandle = *mut c_void;
type MpvRenderContext = *mut c_void;

// ── mpv render param types (libmpv/render.h) ──────────────────────────
const MPV_RENDER_PARAM_INVALID: c_int = 0;
const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 3;
const MPV_RENDER_PARAM_OPENGL_FBO: c_int = 4;
const MPV_RENDER_PARAM_FLIP_Y: c_int = 5;
const MPV_RENDER_PARAM_ADVANCED_CONTROL: c_int = 10;

const MPV_FORMAT_FLAG: c_int = 3;
const MPV_FORMAT_INT64: c_int = 4;
const MPV_FORMAT_DOUBLE: c_int = 5;
const MPV_FORMAT_STRING: c_int = 1;

#[repr(C)]
struct MpvRenderParam {
    type_: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct MpvOpenGlInitParams {
    get_proc_address: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void>,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct MpvOpenGlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

struct MpvApi {
    create: unsafe extern "C" fn() -> MpvHandle,
    initialize: unsafe extern "C" fn(MpvHandle) -> c_int,
    terminate: unsafe extern "C" fn(MpvHandle),
    set_option_string: unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int,
    command_string: unsafe extern "C" fn(MpvHandle, *const c_char) -> c_int,
    set_property_string: unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int,
    get_property: unsafe extern "C" fn(
        MpvHandle,
        *const c_char,
        c_int,
        *mut c_void,
    ) -> c_int,
    free: unsafe extern "C" fn(*mut c_void),
    render_context_create: unsafe extern "C" fn(
        *mut MpvRenderContext,
        MpvHandle,
        *mut MpvRenderParam,
    ) -> c_int,
    render_context_free: unsafe extern "C" fn(MpvRenderContext),
    render_context_render: unsafe extern "C" fn(MpvRenderContext, *mut MpvRenderParam) -> c_int,
    render_context_update: unsafe extern "C" fn(MpvRenderContext) -> u64,
    _lib: libloading::Library,
}

static API: OnceCell<Option<MpvApi>> = OnceCell::new();
static PLAYER: Mutex<Option<Player>> = Mutex::new(None);

/// Default offscreen render size (16:9). Scaled by mpv into the FBO.
const RENDER_W: i32 = 640;
const RENDER_H: i32 = 360;

struct Player {
    handle: usize,
    render: usize,
    tex: u32,
    fbo: u32,
    width: i32,
    height: i32,
    title: String,
    paused: bool,
    /// Last known time / duration in seconds.
    time: f64,
    duration: f64,
    volume: i32,
    has_frame: bool,
}

fn load_api() -> Option<&'static MpvApi> {
    API.get_or_init(|| unsafe { try_load() }).as_ref()
}

unsafe fn try_load() -> Option<MpvApi> {
    for name in ["mpv-2.dll", "mpv-1.dll", "libmpv-2.dll", "mpv.dll"] {
        let Ok(lib) = libloading::Library::new(name) else {
            continue;
        };
        macro_rules! sym {
            ($n:expr) => {
                match lib.get($n) {
                    Ok(s) => *s,
                    Err(_) => continue,
                }
            };
        }
        let create = sym!(b"mpv_create\0");
        let initialize = sym!(b"mpv_initialize\0");
        let terminate = sym!(b"mpv_terminate_destroy\0");
        let set_option_string = sym!(b"mpv_set_option_string\0");
        let command_string = sym!(b"mpv_command_string\0");
        let set_property_string = sym!(b"mpv_set_property_string\0");
        let get_property = sym!(b"mpv_get_property\0");
        let free = sym!(b"mpv_free\0");
        let render_context_create = sym!(b"mpv_render_context_create\0");
        let render_context_free = sym!(b"mpv_render_context_free\0");
        let render_context_render = sym!(b"mpv_render_context_render\0");
        let render_context_update = sym!(b"mpv_render_context_update\0");
        return Some(MpvApi {
            create,
            initialize,
            terminate,
            set_option_string,
            command_string,
            set_property_string,
            get_property,
            free,
            render_context_create,
            render_context_free,
            render_context_render,
            render_context_update,
            _lib: lib,
        });
    }
    None
}

/// wglGetProcAddress / opengl32 trampoline for mpv's GL loader.
unsafe extern "C" fn mpv_get_proc_address(
    _ctx: *mut c_void,
    name: *const c_char,
) -> *mut c_void {
    if name.is_null() {
        return ptr::null_mut();
    }
    let cstr = CStr::from_ptr(name);
    let bytes = cstr.to_bytes_with_nul();
    gl::get_proc_address(bytes)
}

fn set_opt(api: &MpvApi, handle: MpvHandle, key: &str, val: &str) {
    unsafe {
        let k = CString::new(key).unwrap();
        let v = CString::new(val).unwrap();
        let _ = (api.set_option_string)(handle, k.as_ptr(), v.as_ptr());
    }
}

pub fn play_url(url: &str) -> Result<(), String> {
    play_url_titled(url, "")
}

pub fn play_url_titled(url: &str, title: &str) -> Result<(), String> {
    let api = load_api().ok_or_else(|| {
        "libmpv not found (place mpv-2.dll next to the hook)".to_string()
    })?;
    // Ensure GL entry points are available before creating the render ctx.
    if gl::ensure_procs().is_none() {
        return Err("OpenGL procs not ready".into());
    }
    shutdown();

    unsafe {
        let handle = (api.create)();
        if handle.is_null() {
            return Err("mpv_create failed".into());
        }

        // libmpv owns the GL path — we pull frames via render API.
        set_opt(api, handle, "vo", "libmpv");
        set_opt(api, handle, "gpu-context", "auto");
        set_opt(api, handle, "ytdl", "yes");
        set_opt(api, handle, "keep-open", "yes");
        set_opt(api, handle, "idle", "yes");
        set_opt(api, handle, "osc", "no");
        set_opt(api, handle, "osd-level", "0");
        set_opt(api, handle, "video-timing-offset", "0");
        // Quiet logs into the game console.
        set_opt(api, handle, "msg-level", "all=error");

        if (api.initialize)(handle) < 0 {
            (api.terminate)(handle);
            return Err("mpv_initialize failed".into());
        }

        // Build OpenGL render context.
        let mut gl_init = MpvOpenGlInitParams {
            get_proc_address: Some(mpv_get_proc_address),
            get_proc_address_ctx: ptr::null_mut(),
        };
        let api_type = CString::new("opengl").unwrap();
        let mut advanced: c_int = 0;
        let mut params = [
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_API_TYPE,
                data: api_type.as_ptr() as *mut _,
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &mut gl_init as *mut _ as *mut _,
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_ADVANCED_CONTROL,
                data: &mut advanced as *mut _ as *mut _,
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];
        let mut render: MpvRenderContext = ptr::null_mut();
        let rc = (api.render_context_create)(&mut render, handle, params.as_mut_ptr());
        if rc < 0 || render.is_null() {
            (api.terminate)(handle);
            return Err(format!("mpv_render_context_create failed ({rc})"));
        }

        // Offscreen FBO + texture the UI blits from.
        let (tex, fbo) = match gl::create_fbo_texture(RENDER_W, RENDER_H) {
            Some(pair) => pair,
            None => {
                (api.render_context_free)(render);
                (api.terminate)(handle);
                // Last resort: audio-only with a placeholder texture.
                return play_audio_only(url, title);
            }
        };

        // Kick off playback.
        let safe_url = url.replace('\\', "/").replace('"', "");
        let cmd = CString::new(format!("loadfile \"{safe_url}\" replace")).unwrap();
        let _ = (api.command_string)(handle, cmd.as_ptr());

        *PLAYER.lock() = Some(Player {
            handle: handle as usize,
            render: render as usize,
            tex,
            fbo,
            width: RENDER_W,
            height: RENDER_H,
            title: title.to_string(),
            paused: false,
            time: 0.0,
            duration: 0.0,
            volume: 100,
            has_frame: false,
        });
    }
    Ok(())
}

/// Fallback when FBO/render API is unavailable: vo=null audio + solid placeholder.
fn play_audio_only(url: &str, title: &str) -> Result<(), String> {
    let api = load_api().ok_or_else(|| "libmpv not found".to_string())?;
    unsafe {
        let handle = (api.create)();
        if handle.is_null() {
            return Err("mpv_create failed".into());
        }
        set_opt(api, handle, "vo", "null");
        set_opt(api, handle, "ytdl", "yes");
        set_opt(api, handle, "keep-open", "yes");
        set_opt(api, handle, "msg-level", "all=error");
        if (api.initialize)(handle) < 0 {
            (api.terminate)(handle);
            return Err("mpv_initialize failed".into());
        }
        let safe_url = url.replace('\\', "/").replace('"', "");
        let cmd = CString::new(format!("loadfile \"{safe_url}\" replace")).unwrap();
        let _ = (api.command_string)(handle, cmd.as_ptr());

        // Dark blue placeholder so the UI still shows a "player" box.
        let mut pixels = Vec::with_capacity((16 * 16 * 4) as usize);
        for _ in 0..(16 * 16) {
            pixels.extend_from_slice(&[30u8, 40, 70, 255]);
        }
        let tex = gl::create_rgba_texture(16, 16, &pixels);

        *PLAYER.lock() = Some(Player {
            handle: handle as usize,
            render: 0,
            tex,
            fbo: 0,
            width: 16,
            height: 16,
            title: title.to_string(),
            paused: false,
            time: 0.0,
            duration: 0.0,
            volume: 100,
            has_frame: true,
        });
    }
    Ok(())
}

pub fn toggle_pause() {
    let Some(api) = load_api() else {
        return;
    };
    let mut g = PLAYER.lock();
    if let Some(p) = g.as_mut() {
        unsafe {
            let cmd = CString::new("cycle pause").unwrap();
            let _ = (api.command_string)(p.handle as MpvHandle, cmd.as_ptr());
        }
        p.paused = !p.paused;
    }
}

pub fn stop() {
    shutdown();
}

pub fn seek_relative(seconds: f64) {
    let Some(api) = load_api() else {
        return;
    };
    let g = PLAYER.lock();
    if let Some(p) = g.as_ref() {
        unsafe {
            let cmd = CString::new(format!("seek {seconds} relative")).unwrap();
            let _ = (api.command_string)(p.handle as MpvHandle, cmd.as_ptr());
        }
    }
}

pub fn volume_delta(delta: i32) {
    let Some(api) = load_api() else {
        return;
    };
    let mut g = PLAYER.lock();
    if let Some(p) = g.as_mut() {
        p.volume = (p.volume + delta).clamp(0, 150);
        unsafe {
            let key = CString::new("volume").unwrap();
            let val = CString::new(p.volume.to_string()).unwrap();
            let _ = (api.set_property_string)(p.handle as MpvHandle, key.as_ptr(), val.as_ptr());
        }
    }
}

/// Returns (tex, width, height) for the latest rendered frame.
pub fn texture() -> Option<(u32, i32, i32)> {
    let g = PLAYER.lock();
    g.as_ref().map(|p| {
        // Audio-only placeholder is 16×16 — present it at a watchable size.
        if p.render == 0 {
            (p.tex, 320, 180)
        } else {
            (p.tex, p.width.max(1), p.height.max(1))
        }
    })
}

pub fn is_active() -> bool {
    PLAYER.lock().is_some()
}

pub fn is_paused() -> bool {
    PLAYER.lock().as_ref().map(|p| p.paused).unwrap_or(false)
}

pub fn title() -> String {
    PLAYER
        .lock()
        .as_ref()
        .map(|p| p.title.clone())
        .unwrap_or_default()
}

pub fn set_title(t: &str) {
    if let Some(p) = PLAYER.lock().as_mut() {
        if p.title.is_empty() {
            p.title = t.to_string();
        }
    }
}

pub fn time_info() -> (f64, f64, i32) {
    let g = PLAYER.lock();
    g.as_ref()
        .map(|p| (p.time, p.duration, p.volume))
        .unwrap_or((0.0, 0.0, 100))
}

fn get_double(api: &MpvApi, handle: MpvHandle, name: &str) -> Option<f64> {
    unsafe {
        let key = CString::new(name).ok()?;
        let mut out: f64 = 0.0;
        let rc = (api.get_property)(
            handle,
            key.as_ptr(),
            MPV_FORMAT_DOUBLE,
            &mut out as *mut _ as *mut c_void,
        );
        if rc < 0 {
            None
        } else {
            Some(out)
        }
    }
}

fn get_flag(api: &MpvApi, handle: MpvHandle, name: &str) -> Option<bool> {
    unsafe {
        let key = CString::new(name).ok()?;
        let mut out: c_int = 0;
        let rc = (api.get_property)(
            handle,
            key.as_ptr(),
            MPV_FORMAT_FLAG,
            &mut out as *mut _ as *mut c_void,
        );
        if rc < 0 {
            None
        } else {
            Some(out != 0)
        }
    }
}

/// Poll properties + render a new video frame into the offscreen FBO.
/// Must be called on the GL thread (swap-buffers hook).
pub fn tick() {
    let Some(api) = load_api() else {
        return;
    };
    let mut g = PLAYER.lock();
    let Some(p) = g.as_mut() else {
        return;
    };
    let handle = p.handle as MpvHandle;
    let render = p.render as MpvRenderContext;

    // Refresh playback clocks.
    if let Some(t) = get_double(api, handle, "time-pos") {
        p.time = t;
    }
    if let Some(d) = get_double(api, handle, "duration") {
        p.duration = d;
    }
    if let Some(paused) = get_flag(api, handle, "pause") {
        p.paused = paused;
    }
    // media-title from ytdl (best-effort)
    unsafe {
        let key = CString::new("media-title").unwrap();
        let mut ptr_out: *mut c_char = ptr::null_mut();
        let rc = (api.get_property)(
            handle,
            key.as_ptr(),
            MPV_FORMAT_STRING,
            &mut ptr_out as *mut _ as *mut c_void,
        );
        if rc >= 0 && !ptr_out.is_null() {
            if let Ok(s) = CStr::from_ptr(ptr_out).to_str() {
                if !s.is_empty() && (p.title.is_empty() || p.title.starts_with("http")) {
                    p.title = s.to_string();
                }
            }
            (api.free)(ptr_out as *mut c_void);
        }
    }

    // Audio-only path: no render context.
    if p.render == 0 || p.fbo == 0 {
        return;
    }

    // Ask mpv if a new frame is ready.
    let _flags = unsafe { (api.render_context_update)(render) };

    // Render into our FBO.
    let mut fbo = MpvOpenGlFbo {
        fbo: p.fbo as c_int,
        w: p.width,
        h: p.height,
        internal_format: 0,
    };
    let mut flip: c_int = 1; // OpenGL bottom-left origin
    let mut params = [
        MpvRenderParam {
            type_: MPV_RENDER_PARAM_OPENGL_FBO,
            data: &mut fbo as *mut _ as *mut _,
        },
        MpvRenderParam {
            type_: MPV_RENDER_PARAM_FLIP_Y,
            data: &mut flip as *mut _ as *mut _,
        },
        MpvRenderParam {
            type_: MPV_RENDER_PARAM_INVALID,
            data: ptr::null_mut(),
        },
    ];
    let rc = unsafe { (api.render_context_render)(render, params.as_mut_ptr()) };
    if rc >= 0 {
        p.has_frame = true;
    }
    // Leave GL state clean for Minecraft / our immediate UI.
    gl::bind_default_framebuffer();
}

pub fn shutdown() {
    let api = load_api();
    let mut g = PLAYER.lock();
    if let Some(p) = g.take() {
        if let Some(api) = api {
            unsafe {
                if p.render != 0 {
                    (api.render_context_free)(p.render as MpvRenderContext);
                }
                (api.terminate)(p.handle as MpvHandle);
            }
        }
        gl::delete_fbo_texture(p.fbo, p.tex);
    }
}

// Silence unused constant warnings for completeness.
#[allow(dead_code)]
const _KEEP: (c_int, c_int) = (MPV_FORMAT_INT64, MPV_RENDER_PARAM_ADVANCED_CONTROL);
