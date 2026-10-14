//! One launcher per folder. A second launch brings the first window forward and
//! leaves; it would only fight the first for the WebView2 profile in `data\`.

use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

use crate::settings::app_dir;

/// The window class every launcher window registers, so a second copy can find it.
pub const WINDOW_CLASS: &str = "JavacrafterLauncher";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Held for the life of the process; Windows releases it on exit.
pub struct Guard(#[allow(dead_code)] HANDLE);

/// Claims this folder's instance. `wait` is for a copy that was just started by an
/// update: the old copy is still on its way out, so wait for it rather than defer.
/// Returns None when another copy is running (and has been brought forward).
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
