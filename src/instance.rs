//! One launcher per folder. A second launch brings the first window forward and
//! leaves; it would only fight the first for the WebView2 profile in `data\`.

#[cfg(windows)]
use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

#[cfg(windows)]
use crate::settings::app_dir;

/// The window class every launcher window registers, so a second copy can find it.
#[cfg(windows)]
pub const WINDOW_CLASS: &str = "JavacrafterLauncher";

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Held for the life of the process; the OS releases it on exit.
#[cfg(windows)]
pub struct Guard(#[allow(dead_code)] HANDLE);

#[cfg(not(windows))]
pub struct Guard(#[allow(dead_code)] Option<std::fs::File>);

/// Claims this folder's instance. `wait` is for a copy that was just started by an
/// update: the old copy is still on its way out, so wait for it rather than defer.
/// Returns None when another copy is running (and has been brought forward).
#[cfg(windows)]
pub fn claim(wait: bool) -> Option<Guard> {
    // Keyed on the folder (FNV-1a, stable across builds) so two copies kept in
    // different folders don't block each other.
    let key = app_dir().to_string_lossy().to_lowercase();
    let hash = key.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    let name = wide(&format!("Local\\JavacrafterLauncher-{hash:016x}"));

    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if handle.is_null() {
            return Some(Guard(handle)); // can't tell; run rather than refuse
        }
        if GetLastError() != ERROR_ALREADY_EXISTS {
            WaitForSingleObject(handle, 0);
            return Some(Guard(handle));
        }
        let timeout = if wait { 15_000 } else { 0 };
        match WaitForSingleObject(handle, timeout) {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Some(Guard(handle)),
            _ => {
                focus_existing();
                None
            }
        }
    }
}

#[cfg(windows)]
fn focus_existing() {
    let class = wide(WINDOW_CLASS);
    unsafe {
        let hwnd = FindWindowW(class.as_ptr(), std::ptr::null());
        if !hwnd.is_null() {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        }
    }
}

#[cfg(not(windows))]
pub fn claim(wait: bool) -> Option<Guard> {
    use std::fs::{self, OpenOptions};
    use std::os::unix::io::AsRawFd;
    use std::time::{Duration, Instant};

    extern "C" {
        fn flock(fd: std::ffi::c_int, operation: std::ffi::c_int) -> std::ffi::c_int;
    }
    const LOCK_EX: std::ffi::c_int = 2;
    const LOCK_NB: std::ffi::c_int = 4;

    let dir = crate::settings::data_dir();
    let _ = fs::create_dir_all(&dir);
    let Ok(file) = OpenOptions::new().create(true).write(true).open(dir.join("launcher.lock")) else {
        return Some(Guard(None)); // can't tell; run rather than refuse
    };

    let deadline = Instant::now() + if wait { Duration::from_secs(15) } else { Duration::ZERO };
    loop {
        if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0 {
            return Some(Guard(Some(file)));
        }
        if Instant::now() >= deadline {
            focus_existing();
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(not(windows))]
fn focus_existing() {
    let _ = std::process::Command::new("osascript")
        .args(["-e", "tell application id \"com.javacrafter.launcher\" to activate"])
        .status();
}
