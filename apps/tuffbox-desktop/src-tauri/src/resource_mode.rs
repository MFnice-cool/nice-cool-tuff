//! Temporarily deprioritize the launcher while the Minecraft process runs.
//!
//! This is deliberately best-effort. Java remains at its normal OS priority;
//! only the TuffBox process is moved to a background resource class and is
//! restored automatically when that Java PID exits.

#[cfg(windows)]
pub fn enter_for_game(java_pid: u32) -> Result<(), String> {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetPriorityClass, SetPriorityClass, PROCESS_CREATION_FLAGS,
    };

    // Windows background mode lowers CPU and I/O scheduling priority without
    // starving the process or changing Java's priority. The numeric constants
    // are the documented PROCESS_MODE_BACKGROUND_* values.
    const PROCESS_MODE_BACKGROUND_BEGIN: u32 = 0x0010_0000;
    const PROCESS_MODE_BACKGROUND_END: u32 = 0x0020_0000;
    let previous = unsafe {
        let process = GetCurrentProcess();
        let previous = GetPriorityClass(process);
        if previous == 0 {
            return Err(format!(
                "GetPriorityClass failed: {}",
                windows::core::Error::from_win32()
            ));
        }
        SetPriorityClass(process, PROCESS_CREATION_FLAGS(PROCESS_MODE_BACKGROUND_BEGIN))
                    .map_err(|e| format!("SetPriorityClass(background): {e}"))?;
        previous
    };

    // Restore the previous priority class once the game exits. The unsafe ops
    // run on the watcher thread after this function returned, so they need
    // their own unsafe block here (spawning itself is safe).
    std::thread::spawn(move || {
        while tuffbox_core::process::pid_is_alive(java_pid) {
            std::thread::sleep(std::time::Duration::from_millis(750));
        }
        unsafe {
            let process = GetCurrentProcess();
            // Prefer the exact previous class. If it became invalid,
            // background end still exits background mode safely.
            if SetPriorityClass(process, PROCESS_CREATION_FLAGS(previous)).is_err() {
                let _ = SetPriorityClass(process, PROCESS_CREATION_FLAGS(PROCESS_MODE_BACKGROUND_END));
            }
        }
    });
    Ok(())
}

#[cfg(not(windows))]
pub fn enter_for_game(_java_pid: u32) -> Result<(), String> {
    // Avoid shelling out to renice: changing the launcher's nice value is
    // process-global and restoring an inherited non-zero priority is unsafe.
    // A future Unix implementation should use setpriority/getpriority.
    Ok(())
}
