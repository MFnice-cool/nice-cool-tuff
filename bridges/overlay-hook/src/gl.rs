//! Minimal OpenGL helpers loaded via wglGetProcAddress / opengl32.

use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use windows::core::PCSTR;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::OpenGL::*;
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

pub type GLenum = u32;
pub type GLuint = u32;
pub type GLint = i32;
pub type GLsizei = i32;
pub type GLfloat = f32;
pub type GLbitfield = u32;
pub type GLboolean = u8;

pub const GL_TEXTURE_2D: GLenum = 0x0DE1;
pub const GL_RGBA: GLenum = 0x1908;
pub const GL_RGB: GLenum = 0x1907;
pub const GL_UNSIGNED_BYTE: GLenum = 0x1401;
pub const GL_LINEAR: GLenum = 0x2601;
pub const GL_CLAMP_TO_EDGE: GLenum = 0x812F;
pub const GL_TEXTURE_MIN_FILTER: GLenum = 0x2801;
pub const GL_TEXTURE_MAG_FILTER: GLenum = 0x2800;
pub const GL_TEXTURE_WRAP_S: GLenum = 0x2802;
pub const GL_TEXTURE_WRAP_T: GLenum = 0x2803;
pub const GL_BLEND: GLenum = 0x0BE2;
pub const GL_SRC_ALPHA: GLenum = 0x0302;
pub const GL_ONE_MINUS_SRC_ALPHA: GLenum = 0x0303;
pub const GL_MODELVIEW: GLenum = 0x1700;
pub const GL_PROJECTION: GLenum = 0x1701;
pub const GL_COLOR_BUFFER_BIT: GLbitfield = 0x00004000;
pub const GL_QUADS: GLenum = 0x0007;
pub const GL_DEPTH_TEST: GLenum = 0x0B71;
pub const GL_CULL_FACE: GLenum = 0x0B44;
pub const GL_FRAMEBUFFER: GLenum = 0x8D40;
pub const GL_COLOR_ATTACHMENT0: GLenum = 0x8CE0;
pub const GL_FRAMEBUFFER_COMPLETE: GLenum = 0x8CD5;
pub const GL_RGBA8: GLenum = 0x8058;

type WglGetProcAddressFn = unsafe extern "system" fn(PCSTR) -> *mut core::ffi::c_void;

static PROCS: OnceCell<GlProcs> = OnceCell::new();
static VIEWPORT: Mutex<(i32, i32, i32, i32)> = Mutex::new((0, 0, 0, 0));
/// Screen origin of the game client area + client size (for DPI / GUI-scale).
static CLIENT_MAP: Mutex<ClientMap> = Mutex::new(ClientMap::identity());

#[derive(Clone, Copy)]
struct ClientMap {
    origin_x: i32,
    origin_y: i32,
    client_w: i32,
    client_h: i32,
    vp_w: i32,
    vp_h: i32,
}

impl ClientMap {
    const fn identity() -> Self {
        Self {
            origin_x: 0,
            origin_y: 0,
            client_w: 1,
            client_h: 1,
            vp_w: 1,
            vp_h: 1,
        }
    }
}

pub struct GlProcs {
    pub gen_textures: unsafe extern "system" fn(GLsizei, *mut GLuint),
    pub delete_textures: unsafe extern "system" fn(GLsizei, *const GLuint),
    pub bind_texture: unsafe extern "system" fn(GLenum, GLuint),
    pub tex_image_2d: unsafe extern "system" fn(
        GLenum,
        GLint,
        GLint,
        GLsizei,
        GLsizei,
        GLint,
        GLenum,
        GLenum,
        *const core::ffi::c_void,
    ),
    pub tex_sub_image_2d: unsafe extern "system" fn(
        GLenum,
        GLint,
        GLint,
        GLint,
        GLsizei,
        GLsizei,
        GLenum,
        GLenum,
        *const core::ffi::c_void,
    ),
    pub tex_parameteri: unsafe extern "system" fn(GLenum, GLenum, GLint),
    pub gen_framebuffers: unsafe extern "system" fn(GLsizei, *mut GLuint),
    pub delete_framebuffers: unsafe extern "system" fn(GLsizei, *const GLuint),
    pub bind_framebuffer: unsafe extern "system" fn(GLenum, GLuint),
    pub framebuffer_texture_2d:
        unsafe extern "system" fn(GLenum, GLenum, GLenum, GLuint, GLint),
    pub check_framebuffer_status: unsafe extern "system" fn(GLenum) -> GLenum,
}

/// Public trampoline used by libmpv's GL loader.
pub fn get_proc_address(name: &[u8]) -> *mut core::ffi::c_void {
    load_proc(name)
}

fn load_proc(name: &[u8]) -> *mut core::ffi::c_void {
    unsafe {
        let wgl: WglGetProcAddressFn = {
            let opengl = GetModuleHandleA(PCSTR::from_raw(b"opengl32.dll\0".as_ptr())).ok();
            let Some(m) = opengl else {
                return std::ptr::null_mut();
            };
            let p = GetProcAddress(m, PCSTR::from_raw(b"wglGetProcAddress\0".as_ptr()));
            match p {
                Some(f) => std::mem::transmute(f),
                None => return std::ptr::null_mut(),
            }
        };
        let mut p = wgl(PCSTR::from_raw(name.as_ptr()));
        if p.is_null() {
            if let Ok(m) = GetModuleHandleA(PCSTR::from_raw(b"opengl32.dll\0".as_ptr())) {
                if let Some(f) = GetProcAddress(m, PCSTR::from_raw(name.as_ptr())) {
                    p = f as *mut _;
                }
            }
        }
        p
    }
}

macro_rules! load {
    ($name:expr) => {{
        let p = load_proc($name);
        if p.is_null() {
            return None;
        }
        std::mem::transmute(p)
    }};
}

/// Load core + FBO procs. FBO symbols may come from ARB suffixes on old drivers.
pub fn ensure_procs() -> Option<&'static GlProcs> {
    if let Some(p) = PROCS.get() {
        return Some(p);
    }
    unsafe {
        let gen_fb = {
            let p = load_proc(b"glGenFramebuffers\0");
            if p.is_null() {
                load_proc(b"glGenFramebuffersEXT\0")
            } else {
                p
            }
        };
        let del_fb = {
            let p = load_proc(b"glDeleteFramebuffers\0");
            if p.is_null() {
                load_proc(b"glDeleteFramebuffersEXT\0")
            } else {
                p
            }
        };
        let bind_fb = {
            let p = load_proc(b"glBindFramebuffer\0");
            if p.is_null() {
                load_proc(b"glBindFramebufferEXT\0")
            } else {
                p
            }
        };
        let fb_tex = {
            let p = load_proc(b"glFramebufferTexture2D\0");
            if p.is_null() {
                load_proc(b"glFramebufferTexture2DEXT\0")
            } else {
                p
            }
        };
        let check_fb = {
            let p = load_proc(b"glCheckFramebufferStatus\0");
            if p.is_null() {
                load_proc(b"glCheckFramebufferStatusEXT\0")
            } else {
                p
            }
        };
        if gen_fb.is_null()
            || del_fb.is_null()
            || bind_fb.is_null()
            || fb_tex.is_null()
            || check_fb.is_null()
        {
            // Still install texture-only procs so thumbnails work without FBO.
            let procs = GlProcs {
                gen_textures: load!(b"glGenTextures\0"),
                delete_textures: load!(b"glDeleteTextures\0"),
                bind_texture: load!(b"glBindTexture\0"),
                tex_image_2d: load!(b"glTexImage2D\0"),
                tex_sub_image_2d: load!(b"glTexSubImage2D\0"),
                tex_parameteri: load!(b"glTexParameteri\0"),
                gen_framebuffers: dummy_gen,
                delete_framebuffers: dummy_del,
                bind_framebuffer: dummy_bind,
                framebuffer_texture_2d: dummy_fb_tex,
                check_framebuffer_status: dummy_check,
            };
            let _ = PROCS.set(procs);
            return PROCS.get();
        }
        let procs = GlProcs {
            gen_textures: load!(b"glGenTextures\0"),
            delete_textures: load!(b"glDeleteTextures\0"),
            bind_texture: load!(b"glBindTexture\0"),
            tex_image_2d: load!(b"glTexImage2D\0"),
            tex_sub_image_2d: load!(b"glTexSubImage2D\0"),
            tex_parameteri: load!(b"glTexParameteri\0"),
            gen_framebuffers: std::mem::transmute(gen_fb),
            delete_framebuffers: std::mem::transmute(del_fb),
            bind_framebuffer: std::mem::transmute(bind_fb),
            framebuffer_texture_2d: std::mem::transmute(fb_tex),
            check_framebuffer_status: std::mem::transmute(check_fb),
        };
        let _ = PROCS.set(procs);
    }
    PROCS.get()
}

unsafe extern "system" fn dummy_gen(_: GLsizei, _: *mut GLuint) {}
unsafe extern "system" fn dummy_del(_: GLsizei, _: *const GLuint) {}
unsafe extern "system" fn dummy_bind(_: GLenum, _: GLuint) {}
unsafe extern "system" fn dummy_fb_tex(_: GLenum, _: GLenum, _: GLenum, _: GLuint, _: GLint) {}
unsafe extern "system" fn dummy_check(_: GLenum) -> GLenum {
    0
}

pub fn begin_overlay_frame(hdc: HDC) {
    unsafe {
        let mut vp = [0i32; 4];
        glGetIntegerv(0x0BA2, vp.as_mut_ptr()); // GL_VIEWPORT
        *VIEWPORT.lock() = (vp[0], vp[1], vp[2], vp[3]);
        let w = if vp[2] > 0 { vp[2] } else { 1280 };
        let h = if vp[3] > 0 { vp[3] } else { 720 };

        use windows::Win32::Foundation::{POINT, RECT};
        use windows::Win32::Graphics::Gdi::WindowFromDC;
        use windows::Win32::UI::WindowsAndMessaging::{ClientToScreen, GetClientRect};
        let hwnd = WindowFromDC(hdc);
        // Windowed / borderless / exclusive fullscreen:
        // ClientToScreen = top-left of client on the virtual desktop.
        // GetClientRect = logical client size. Scale by viewport/client so
        // GUI-scale and DPI still hit-test correctly.
        if hwnd.0 as usize != 0 {
            let mut origin = POINT { x: 0, y: 0 };
            let _ = ClientToScreen(hwnd, &mut origin);
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let cw = (rc.right - rc.left).max(1);
            let ch = (rc.bottom - rc.top).max(1);
            *CLIENT_MAP.lock() = ClientMap {
                origin_x: origin.x,
                origin_y: origin.y,
                client_w: cw,
                client_h: ch,
                vp_w: w,
                vp_h: h,
            };
        } else {
            let mut m = *CLIENT_MAP.lock();
            m.vp_w = w;
            m.vp_h = h;
            if m.client_w <= 1 {
                m.client_w = w;
                m.client_h = h;
            }
            *CLIENT_MAP.lock() = m;
        }

        glPushAttrib(0x000FFFFF);
        glDisable(GL_DEPTH_TEST);
        glDisable(GL_CULL_FACE);
        glEnable(GL_BLEND);
        glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
        glMatrixMode(GL_PROJECTION);
        glPushMatrix();
        glLoadIdentity();
        glOrtho(0.0, w as f64, h as f64, 0.0, -1.0, 1.0);
        glMatrixMode(GL_MODELVIEW);
        glPushMatrix();
        glLoadIdentity();
    }
}

/// Convert screen-space cursor (GetCursorPos) into overlay/GL coordinates.
pub fn screen_to_overlay(sx: i32, sy: i32) -> (f32, f32) {
    let m = *CLIENT_MAP.lock();
    let lx = (sx - m.origin_x) as f32;
    let ly = (sy - m.origin_y) as f32;
    let sx_scale = m.vp_w as f32 / m.client_w.max(1) as f32;
    let sy_scale = m.vp_h as f32 / m.client_h.max(1) as f32;
    (lx * sx_scale, ly * sy_scale)
}

/// True if a screen point is inside the game client rectangle.
pub fn screen_in_client(sx: i32, sy: i32) -> bool {
    let m = *CLIENT_MAP.lock();
    sx >= m.origin_x
        && sy >= m.origin_y
        && sx < m.origin_x + m.client_w
        && sy < m.origin_y + m.client_h
}

pub fn end_overlay_frame() {
    unsafe {
        glMatrixMode(GL_MODELVIEW);
        glPopMatrix();
        glMatrixMode(GL_PROJECTION);
        glPopMatrix();
        glPopAttrib();
    }
}

pub fn viewport_size() -> (i32, i32) {
    let (_, _, w, h) = *VIEWPORT.lock();
    (w.max(1), h.max(1))
}

pub fn fill_rect(x: f32, y: f32, w: f32, h: f32, r: f32, g: f32, b: f32, a: f32) {
    unsafe {
        glDisable(GL_TEXTURE_2D);
        glColor4f(r, g, b, a);
        glBegin(GL_QUADS);
        glVertex2f(x, y);
        glVertex2f(x + w, y);
        glVertex2f(x + w, y + h);
        glVertex2f(x, y + h);
        glEnd();
        glColor4f(1.0, 1.0, 1.0, 1.0);
    }
}

pub fn textured_rect(tex: GLuint, x: f32, y: f32, w: f32, h: f32) {
    textured_rect_uv(tex, x, y, w, h, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0);
}

/// Textured quad with explicit UVs + vertex colour (font atlases).
pub fn textured_rect_uv(
    tex: GLuint,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
) {
    if tex == 0 {
        fill_rect(x, y, w, h, 0.2, 0.2, 0.25, a);
        return;
    }
    unsafe {
        glEnable(GL_TEXTURE_2D);
        glBindTexture(GL_TEXTURE_2D, tex);
        glColor4f(r, g, b, a);
        glBegin(GL_QUADS);
        glTexCoord2f(u0, v0);
        glVertex2f(x, y);
        glTexCoord2f(u1, v0);
        glVertex2f(x + w, y);
        glTexCoord2f(u1, v1);
        glVertex2f(x + w, y + h);
        glTexCoord2f(u0, v1);
        glVertex2f(x, y + h);
        glEnd();
        glBindTexture(GL_TEXTURE_2D, 0);
        glDisable(GL_TEXTURE_2D);
        glColor4f(1.0, 1.0, 1.0, 1.0);
    }
}

/// Classic Minecraft raised/inset bevel (light top-left, dark bottom-right).
pub fn bevel_rect(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    face: (f32, f32, f32, f32),
    raised: bool,
) {
    fill_rect(x, y, w, h, face.0, face.1, face.2, face.3);
    let light = if raised {
        (1.0, 1.0, 1.0, 0.40)
    } else {
        (0.0, 0.0, 0.0, 0.45)
    };
    let dark = if raised {
        (0.0, 0.0, 0.0, 0.50)
    } else {
        (1.0, 1.0, 1.0, 0.25)
    };
    let bt = 2.0;
    fill_rect(x, y, w, bt, light.0, light.1, light.2, light.3);
    fill_rect(x, y, bt, h, light.0, light.1, light.2, light.3);
    fill_rect(x, y + h - bt, w, bt, dark.0, dark.1, dark.2, dark.3);
    fill_rect(x + w - bt, y, bt, h, dark.0, dark.1, dark.2, dark.3);
}

/// Outer dark frame + inner raised face (dirt/stone panel).
pub fn mc_panel(x: f32, y: f32, w: f32, h: f32, face: (f32, f32, f32, f32)) {
    fill_rect(x, y, w, h, 0.08, 0.06, 0.04, 1.0);
    bevel_rect(x + 1.0, y + 1.0, w - 2.0, h - 2.0, face, true);
}

/// Cheap procedural dirt speckles (no external textures).
pub fn dirt_fill(x: f32, y: f32, w: f32, h: f32, base: (f32, f32, f32, f32)) {
    fill_rect(x, y, w, h, base.0, base.1, base.2, base.3);
    let mut row = 0i32;
    while (row as f32) < h {
        let mut col = (row * 17 + 3) % 13;
        while (col as f32) < w {
            let n = ((row * 131 + col * 17) & 7) as f32 / 40.0;
            let px = x + col as f32;
            let py = y + row as f32;
            if px + 3.0 <= x + w && py + 3.0 <= y + h {
                fill_rect(
                    px,
                    py,
                    3.0,
                    3.0,
                    (base.0 - n).max(0.0),
                    (base.1 - n * 0.8).max(0.0),
                    (base.2 - n * 0.5).max(0.0),
                    0.35,
                );
            }
            col += 11 + ((row + col) % 5);
        }
        row += 9;
    }
}

/// Fit a video texture into a destination box (letterbox / pillarbox).
pub fn textured_rect_fit(tex: GLuint, x: f32, y: f32, w: f32, h: f32, src_w: f32, src_h: f32) {
    if tex == 0 || src_w <= 0.0 || src_h <= 0.0 {
        fill_rect(x, y, w, h, 0.05, 0.05, 0.08, 1.0);
        return;
    }
    let src_a = src_w / src_h;
    let dst_a = w / h;
    let (dw, dh) = if src_a > dst_a {
        (w, w / src_a)
    } else {
        (h * src_a, h)
    };
    let dx = x + (w - dw) * 0.5;
    let dy = y + (h - dh) * 0.5;
    fill_rect(x, y, w, h, 0.0, 0.0, 0.0, 1.0);
    textured_rect(tex, dx, dy, dw, dh);
}

pub fn create_rgba_texture(width: i32, height: i32, pixels: &[u8]) -> GLuint {
    let Some(p) = ensure_procs() else {
        return 0;
    };
    let mut tex = 0u32;
    unsafe {
        (p.gen_textures)(1, &mut tex);
        (p.bind_texture)(GL_TEXTURE_2D, tex);
        (p.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR as i32);
        (p.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR as i32);
        (p.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE as i32);
        (p.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE as i32);
        (p.tex_image_2d)(
            GL_TEXTURE_2D,
            0,
            GL_RGBA as i32,
            width,
            height,
            0,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            pixels.as_ptr() as *const _,
        );
        (p.bind_texture)(GL_TEXTURE_2D, 0);
    }
    tex
}

pub fn update_rgba_texture(tex: GLuint, width: i32, height: i32, pixels: &[u8]) {
    let Some(p) = ensure_procs() else {
        return;
    };
    if tex == 0 {
        return;
    }
    unsafe {
        (p.bind_texture)(GL_TEXTURE_2D, tex);
        (p.tex_sub_image_2d)(
            GL_TEXTURE_2D,
            0,
            0,
            0,
            width,
            height,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            pixels.as_ptr() as *const _,
        );
        (p.bind_texture)(GL_TEXTURE_2D, 0);
    }
}

pub fn delete_texture(tex: GLuint) {
    if tex == 0 {
        return;
    }
    if let Some(p) = ensure_procs() {
        unsafe {
            (p.delete_textures)(1, &tex);
        }
    }
}

/// Create an FBO + colour texture for libmpv offscreen rendering.
/// Returns `(tex, fbo)` or `None` if FBO extensions are unavailable.
pub fn create_fbo_texture(width: i32, height: i32) -> Option<(GLuint, GLuint)> {
    let p = ensure_procs()?;
    // Detect dummy FBO procs (no real extension).
    let mut probe = 0u32;
    unsafe {
        (p.gen_framebuffers)(1, &mut probe);
        if probe == 0 {
            return None;
        }
        (p.delete_framebuffers)(1, &probe);
    }

    let mut tex = 0u32;
    let mut fbo = 0u32;
    unsafe {
        (p.gen_textures)(1, &mut tex);
        (p.bind_texture)(GL_TEXTURE_2D, tex);
        (p.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR as i32);
        (p.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR as i32);
        (p.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE as i32);
        (p.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE as i32);
        (p.tex_image_2d)(
            GL_TEXTURE_2D,
            0,
            GL_RGBA8 as i32,
            width,
            height,
            0,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            std::ptr::null(),
        );

        (p.gen_framebuffers)(1, &mut fbo);
        (p.bind_framebuffer)(GL_FRAMEBUFFER, fbo);
        (p.framebuffer_texture_2d)(
            GL_FRAMEBUFFER,
            GL_COLOR_ATTACHMENT0,
            GL_TEXTURE_2D,
            tex,
            0,
        );
        let status = (p.check_framebuffer_status)(GL_FRAMEBUFFER);
        (p.bind_framebuffer)(GL_FRAMEBUFFER, 0);
        (p.bind_texture)(GL_TEXTURE_2D, 0);

        if status != GL_FRAMEBUFFER_COMPLETE {
            (p.delete_framebuffers)(1, &fbo);
            (p.delete_textures)(1, &tex);
            return None;
        }
    }
    Some((tex, fbo))
}

pub fn delete_fbo_texture(fbo: GLuint, tex: GLuint) {
    if let Some(p) = ensure_procs() {
        unsafe {
            if fbo != 0 {
                (p.delete_framebuffers)(1, &fbo);
            }
            if tex != 0 {
                (p.delete_textures)(1, &tex);
            }
        }
    }
}

pub fn bind_default_framebuffer() {
    if let Some(p) = ensure_procs() {
        unsafe {
            (p.bind_framebuffer)(GL_FRAMEBUFFER, 0);
        }
    }
}

// Legacy fixed-pipeline entry points from opengl32 (always present on Windows).
#[link(name = "opengl32")]
extern "system" {
    fn glPushAttrib(mask: GLbitfield);
    fn glPopAttrib();
    fn glDisable(cap: GLenum);
    fn glEnable(cap: GLenum);
    fn glBlendFunc(sfactor: GLenum, dfactor: GLenum);
    fn glMatrixMode(mode: GLenum);
    fn glPushMatrix();
    fn glPopMatrix();
    fn glLoadIdentity();
    fn glOrtho(left: f64, right: f64, bottom: f64, top: f64, znear: f64, zfar: f64);
    fn glColor4f(r: GLfloat, g: GLfloat, b: GLfloat, a: GLfloat);
    fn glBegin(mode: GLenum);
    fn glEnd();
    fn glVertex2f(x: GLfloat, y: GLfloat);
    fn glTexCoord2f(s: GLfloat, t: GLfloat);
    fn glGetIntegerv(pname: GLenum, params: *mut GLint);
    fn glBindTexture(target: GLenum, texture: GLuint);
}
