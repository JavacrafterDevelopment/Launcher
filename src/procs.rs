//! Finding and stopping the processes an installed app is running.
//!
//! A process belongs to an app when its exe lives inside that app's folder. An
//! administrator process can't be inspected or ended from here (Windows keeps
//! normal programs out of them), which is why Lighthouse is stopped through its
//! own `--quit` instead; see [`crate::apps`].

use std::path::Path;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, INVALID_HANDLE_VALUE, LPARAM};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
};

/// Ids of the processes whose exe is inside `dir`, among those this launcher is
/// allowed to look at.
pub fn running_in(dir: &Path) -> Vec<u32> {
    let prefix = format!("{}\\", dir.to_string_lossy().trim_end_matches('\\').to_lowercase());
    let mut found = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return found;
        }
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snap, &mut entry) != 0;
        while more {
            let pid = entry.th32ProcessID;
            if let Some(path) = image_path(pid) {
                if path.to_lowercase().starts_with(&prefix) {
                    found.push(pid);
                }
            }
            more = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }
    found
}

fn image_path(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0;
        CloseHandle(process);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Asks every visible window of these processes to close, as clicking ✕ would.
/// Returns how many windows were asked.
pub fn close_windows(pids: &[u32]) -> usize {
    struct Search<'a> {
        pids: &'a [u32],
        asked: usize,
    }
    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> i32 {
        let search = &mut *(data as *mut Search);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if search.pids.contains(&pid) && IsWindowVisible(hwnd) != 0 {
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
            search.asked += 1;
        }
        1 // keep enumerating
    }
    let mut search = Search { pids, asked: 0 };
    unsafe {
        EnumWindows(Some(each), &mut search as *mut Search as LPARAM);
    }
    search.asked
}

/// Ends these processes outright. Only for when asking nicely didn't work.
/// Returns how many were ended.
pub fn terminate(pids: &[u32]) -> usize {
    let mut ended = 0;
    for &pid in pids {
        unsafe {
            let process = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !process.is_null() {
                ended += (TerminateProcess(process, 1) != 0) as usize;
                CloseHandle(process);
            }
        }
    }
    ended
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    /// Starts an app from a folder, finds it, asks it to close, then ends it.
    ///
    ///   set LAUNCHER_TEST_EXE=C:\path\to\App.exe
    ///   set LAUNCHER_TEST_ARGS=--no-elevate      (optional)
    ///   cargo test find_close_and_end -- --ignored --nocapture
    #[test]
    #[ignore]
    fn find_close_and_end() {
        let exe = PathBuf::from(std::env::var("LAUNCHER_TEST_EXE").expect("LAUNCHER_TEST_EXE"));
        let dir = exe.parent().unwrap().to_path_buf();
        let args = std::env::var("LAUNCHER_TEST_ARGS").unwrap_or_default();
        let mut child = std::process::Command::new(&exe)
            .args(args.split_whitespace())
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_secs(6));

        let pids = running_in(&dir);
        println!("running from {}: {pids:?}", dir.display());
        assert!(pids.contains(&child.id()));

        let asked = close_windows(&pids);
        println!("asked {asked} windows to close");
        std::thread::sleep(Duration::from_secs(4));
        let after_close = running_in(&dir);
        println!("still running after closing windows: {after_close:?}");

        if !after_close.is_empty() {
            let ended = terminate(&after_close);
            println!("ended {ended}");
            std::thread::sleep(Duration::from_secs(1));
        }
        assert!(running_in(&dir).is_empty());
        let _ = child.wait();
    }
}
