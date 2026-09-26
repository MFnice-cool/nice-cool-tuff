//! TuffBox overlay GL hook — injected into Minecraft (Windows).

#![allow(non_snake_case)]

#[cfg(windows)]
mod emoji;
#[cfg(windows)]
mod font;
#[cfg(windows)]
mod gl;
#[cfg(windows)]
mod input;
#[cfg(windows)]
mod ipc;
#[cfg(windows)]
mod lru;
#[cfg(windows)]
mod mpv;
#[cfg(windows)]
mod textutil;
#[cfg(windows)]
mod theme;
#[cfg(windows)]
mod ui;

#[cfg(windows)]
mod win {
    use super::*;
    use once_cell::sync::OnceCell;
    use parking_lot::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::core::PCSTR;
    use windows::Win32::Foundation::{BOOL, HINSTANCE, LPARAM, LRESULT, TRUE, WPARAM};
    use windows::Win32::Graphics::Gdi::HDC;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_F8, VK_F9, VK_F10};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT,
        MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_MOUSEWHEEL, WM_SYSKEYDOWN,
    };

    static OVERLAY_OPEN: AtomicBool = AtomicBool::new(false);
    static HOOK_READY: AtomicBool = AtomicBool::new(false);
    static INPUT_HOOK: Mutex<Option<usize>> = Mutex::new(None);
    static MOUSE_HOOK: Mutex<Option<usize>> = Mutex::new(None);
    static UI_STATE: OnceCell<Mutex<ui::UiState>> = OnceCell::new();
    static ORIGINAL_SWAP: OnceCell<WglSwapBuffersFn> = OnceCell::new();
    static MH_ENABLED: AtomicBool = AtomicBool::new(false);

    type WglSwapBuffersFn = unsafe extern "system" fn(HDC) -> BOOL;

    fn ui_state() -> &'static Mutex<ui::UiState> {
        UI_STATE.get_or_init(|| Mutex::new(ui::UiState::new()))
    }

    unsafe extern "system" fn hooked_swap_buffers(hdc: HDC) -> BOOL {
        poll_hotkeys();
        // Always tick mpv on the GL thread so frames keep flowing for PiP.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(mut st) = ui_state().try_lock() {
                if OVERLAY_OPEN.load(Ordering::SeqCst) {
                    st.tick_and_draw(hdc);
                } else {
                    st.tick_background(hdc);
                }
            }
        }));
        if let Some(orig) = ORIGINAL_SWAP.get() {
            orig(hdc)
        } else {
            TRUE
        }
    }

    fn poll_hotkeys() {
        static F8_WAS: AtomicBool = AtomicBool::new(false);
        static F9_WAS: AtomicBool = AtomicBool::new(false);
        static F10_WAS: AtomicBool = AtomicBool::new(false);

        let f8 = unsafe { GetAsyncKeyState(VK_F8.0 as i32) } < 0;
        let f8_was = F8_WAS.load(Ordering::SeqCst);
        if f8 && !f8_was {
            let open = !OVERLAY_OPEN.load(Ordering::SeqCst);
            OVERLAY_OPEN.store(open, Ordering::SeqCst);
            input::set_overlay_open(open);
            if open {
                if let Some(mut st) = ui_state().try_lock() {
                    st.on_open();
                }
                install_input_hooks();
            } else {
                remove_input_hooks();
                if let Some(mut st) = ui_state().try_lock() {
                    st.on_close();
                }
            }
        }
        F8_WAS.store(f8, Ordering::SeqCst);

        if OVERLAY_OPEN.load(Ordering::SeqCst)
            && unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) } < 0
        {
            OVERLAY_OPEN.store(false, Ordering::SeqCst);
            input::set_overlay_open(false);
            remove_input_hooks();
            if let Some(mut st) = ui_state().try_lock() {
                st.on_close();
            }
        }

        // Global media hotkeys (work with overlay closed — PiP transport).
        let f9 = unsafe { GetAsyncKeyState(VK_F9.0 as i32) } < 0;
        if f9 && !F9_WAS.load(Ordering::SeqCst) {
            mpv::toggle_pause();
        }
        F9_WAS.store(f9, Ordering::SeqCst);

        let f10 = unsafe { GetAsyncKeyState(VK_F10.0 as i32) } < 0;
        if f10 && !F10_WAS.load(Ordering::SeqCst) {
            mpv::stop();
        }
        F10_WAS.store(f10, Ordering::SeqCst);
    }

    unsafe extern "system" fn low_level_keyboard(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code == HC_ACTION as i32 && OVERLAY_OPEN.load(Ordering::SeqCst) {
            let msg = wparam.0 as u32;
            if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                let vk = kb.vkCode;

                // Always let F8 / Esc / F9 / F10 through.
                if vk == VK_F8.0 as u32
                    || vk == VK_ESCAPE.0 as u32
                    || vk == VK_F9.0 as u32
                    || vk == VK_F10.0 as u32
                {
                    return CallNextHookEx(HHOOK::default(), code, wparam, lparam);
                }

                if let Some(ch) = vk_to_char(vk) {
                    if let Some(mut st) = ui_state().try_lock() {
                        st.push_char(ch);
                    }
                }

                return LRESULT(1);
            }
        }
        CallNextHookEx(HHOOK::default(), code, wparam, lparam)
    }

    unsafe extern "system" fn low_level_mouse(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code == HC_ACTION as i32 && OVERLAY_OPEN.load(Ordering::SeqCst) {
            if wparam.0 as u32 == WM_MOUSEWHEEL {
                let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                // Only capture wheel when the cursor is over the game client
                // (windowed / borderless / fullscreen). Lets the user scroll
                // other apps on a second monitor.
                if gl::screen_in_client(ms.pt.x, ms.pt.y) {
                    let delta = ((ms.mouseData >> 16) as i16) as i32;
                    input::push_wheel(delta);
                    return LRESULT(1); // swallow so MC doesn't zoom/scroll
                }
            }
        }
        CallNextHookEx(HHOOK::default(), code, wparam, lparam)
    }

    fn vk_to_char(vk: u32) -> Option<char> {
        let shift = unsafe { GetAsyncKeyState(0x10) } < 0; // VK_SHIFT
        if (0x41..=0x5A).contains(&vk) {
            let base = if shift { b'A' } else { b'a' };
            return Some((base + (vk as u8 - 0x41)) as char);
        }
        if (0x30..=0x39).contains(&vk) {
            if shift {
                const SHIFTED: [char; 10] = [')', '!', '@', '#', '$', '%', '^', '&', '*', '('];
                return Some(SHIFTED[(vk - 0x30) as usize]);
            }
            return Some((b'0' + (vk as u8 - 0x30)) as char);
        }
        if (0x60..=0x69).contains(&vk) {
            return Some((b'0' + (vk as u8 - 0x60)) as char);
        }
        if vk == 0x20 {
            return Some(' ');
        }
        match (vk, shift) {
            (0xBA, false) => Some(';'),
            (0xBA, true) => Some(':'),
            (0xBB, false) => Some('='),
            (0xBB, true) => Some('+'),
            (0xBC, false) => Some(','),
            (0xBC, true) => Some('<'),
            (0xBD, false) => Some('-'),
            (0xBD, true) => Some('_'),
            (0xBE, false) => Some('.'),
            (0xBE, true) => Some('>'),
            (0xBF, false) => Some('/'),
            (0xBF, true) => Some('?'),
            (0xC0, false) => Some('`'),
            (0xC0, true) => Some('~'),
            (0xDB, false) => Some('['),
            (0xDB, true) => Some('{'),
            (0xDC, false) => Some('\\'),
            (0xDC, true) => Some('|'),
            (0xDD, false) => Some(']'),
            (0xDD, true) => Some('}'),
            (0xDE, false) => Some('\''),
            (0xDE, true) => Some('"'),
            (0x6A, _) => Some('*'),
            (0x6B, _) => Some('+'),
            (0x6D, _) => Some('-'),
            (0x6E, _) => Some('.'),
            (0x6F, _) => Some('/'),
            _ => None,
        }
    }

    fn install_input_hooks() {
        {
            let mut g = INPUT_HOOK.lock();
            if g.is_none() {
                unsafe {
                    if let Ok(h) =
                        SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard), None, 0)
                    {
                        *g = Some(h.0 as usize);
                    }
                }
            }
        }
        {
            let mut g = MOUSE_HOOK.lock();
            if g.is_none() {
                unsafe {
                    if let Ok(h) = SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse), None, 0) {
                        *g = Some(h.0 as usize);
                    }
                }
            }
        }
    }

    fn remove_input_hooks() {
        if let Some(raw) = INPUT_HOOK.lock().take() {
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(raw as *mut _));
            }
        }
        if let Some(raw) = MOUSE_HOOK.lock().take() {
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(raw as *mut _));
            }
        }
    }

    fn install_gl_hook() -> Result<(), String> {
        unsafe {
            let opengl = GetModuleHandleA(PCSTR::from_raw(b"opengl32.dll\0".as_ptr()))
                .map_err(|e| format!("opengl32: {e}"))?;
            let sym = GetProcAddress(opengl, PCSTR::from_raw(b"wglSwapBuffers\0".as_ptr()))
                .ok_or_else(|| "wglSwapBuffers missing".to_string())?;
            let target: WglSwapBuffersFn = std::mem::transmute(sym);
            let _ = ORIGINAL_SWAP.set(target);

            minhook::MinHook::create_hook(target as _, hooked_swap_buffers as _)
                .map_err(|e| format!("MinHook create: {e:?}"))?;
            minhook::MinHook::enable_hook(target as _)
                .map_err(|e| format!("MinHook enable: {e:?}"))?;
            MH_ENABLED.store(true, Ordering::SeqCst);
        }
        HOOK_READY.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn bootstrap() {
        std::thread::spawn(|| {
            for _ in 0..50 {
                if unsafe { GetModuleHandleA(PCSTR::from_raw(b"opengl32.dll\0".as_ptr())).is_ok() }
                {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            match install_gl_hook() {
                Ok(()) => {
                    let _ = ipc::log_debug("tuffbox overlay hook ready (F8, wheel, PiP)");
                }
                Err(e) => {
                    let _ = ipc::log_debug(&format!("overlay hook failed: {e}"));
                }
            }
        });
    }

    #[no_mangle]
    pub unsafe extern "system" fn DllMain(
        _hinst: HINSTANCE,
        reason: u32,
        _reserved: *mut core::ffi::c_void,
    ) -> BOOL {
        match reason {
            DLL_PROCESS_ATTACH => bootstrap(),
            DLL_PROCESS_DETACH => {
                remove_input_hooks();
                if MH_ENABLED.load(Ordering::SeqCst) {
                    if let Some(orig) = ORIGINAL_SWAP.get() {
                        let _ = minhook::MinHook::disable_hook((*orig) as _);
                        let _ = minhook::MinHook::remove_hook((*orig) as _);
                    }
                }
                mpv::shutdown();
            }
            _ => {}
        }
        TRUE
    }
}

#[cfg(windows)]
pub use win::*;

#[cfg(not(windows))]
#[no_mangle]
pub extern "C" fn tuffbox_overlay_hook_unsupported() {}
