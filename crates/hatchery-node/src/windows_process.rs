//! Windows process-creation helpers for non-PTY children.
//!
//! PTY / ConPTY spawns must keep a console; everything else (ACP, app-server,
//! m4a-agent, spawn profiles, probes, schtasks, powershell helpers) must not
//! pop a visible window. Closing a console must not kill a supervised node.

/// `CREATE_NO_WINDOW` (0x08000000). Safe to OR with other creation flags.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// `DETACHED_PROCESS` (0x00000008) — no console inheritance.
pub const DETACHED_PROCESS: u32 = 0x0000_0008;
/// `CREATE_NEW_PROCESS_GROUP` (0x00000200).
pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Flags for a headless node / child: no window, detached from the parent's console.
pub const HEADLESS_SPAWN_FLAGS: u32 = CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;

/// Apply [`CREATE_NO_WINDOW`] on Windows. No-op elsewhere. Does not touch
/// program, args, env, cwd, or stdio. Call once: a second `creation_flags`
/// replaces the previous value — use [`set_creation_flags`] to OR.
pub fn hide_console(command: &mut std::process::Command) {
    set_creation_flags(command, CREATE_NO_WINDOW);
}

/// Same for tokio's command (via `as_std_mut`).
pub fn hide_console_tokio(command: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        hide_console(command.as_std_mut());
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// OR `flags` into the command's Windows creation flags.
pub fn set_creation_flags(command: &mut std::process::Command, flags: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // creation_flags replaces; callers that need OR must pass the full mask.
        command.creation_flags(flags);
    }
    #[cfg(not(windows))]
    {
        let _ = (command, flags);
    }
}

/// Spawn flags for the supervised child node (no console, detached).
pub fn hide_console_detached(command: &mut std::process::Command) {
    set_creation_flags(command, HEADLESS_SPAWN_FLAGS);
}

/// Detach this process from its console (if any). No-op on non-Windows.
pub fn free_console() {
    #[cfg(windows)]
    {
        // SAFETY: FreeConsole is process-wide and idempotent when unattached.
        unsafe {
            let _ = windows_sys::Win32::System::Console::FreeConsole();
        }
    }
}

/// Ignore CTRL_CLOSE / CTRL_LOGOFF / CTRL_SHUTDOWN so closing a terminal that
/// somehow still owns us does not kill a supervised node. CTRL_C still works
/// for interactive CLI. No-op on non-Windows.
pub fn ignore_console_close_events() {
    #[cfg(windows)]
    {
        unsafe extern "system" fn handler(ctrl: u32) -> i32 {
            use windows_sys::Win32::System::Console::{
                CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
            };
            match ctrl {
                CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT => 1, // TRUE = handled
                _ => 0,
            }
        }
        unsafe {
            let _ = windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(handler), 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hide_console_keeps_program_and_args() {
        let mut c = std::process::Command::new("prog");
        c.arg("a").arg("b");
        hide_console(&mut c);
        assert_eq!(c.get_program(), std::ffi::OsStr::new("prog"));
        assert_eq!(c.get_args().collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn create_no_window_constant() {
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
        assert_eq!(DETACHED_PROCESS, 0x0000_0008);
        assert_ne!(HEADLESS_SPAWN_FLAGS & CREATE_NO_WINDOW, 0);
    }
}


/// Current user SID (`S-1-5-…`) and `DOMAIN\User` for Task Scheduler XML.
/// Empty strings on non-Windows.
pub fn current_user_identity() -> Result<(String, String), String> {
    #[cfg(windows)]
    {
        windows_current_user_identity()
    }
    #[cfg(not(windows))]
    {
        Err("current_user_identity is Windows-only".into())
    }
}

#[cfg(windows)]
fn windows_current_user_identity() -> Result<(String, String), String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, LookupAccountSidW, TokenUser, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(format!("OpenProcessToken failed ({})", GetLastError()));
        }
        let mut needed: u32 = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        let mut buf = vec![0u8; needed as usize];
        if GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr() as *mut _,
            needed,
            &mut needed,
        ) == 0
        {
            let err = GetLastError();
            CloseHandle(token);
            return Err(format!("GetTokenInformation TokenUser failed ({err})"));
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let sid = user.User.Sid;

        let mut sid_str: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut sid_str) == 0 {
            let err = GetLastError();
            CloseHandle(token);
            return Err(format!("ConvertSidToStringSidW failed ({err})"));
        }
        let sid_os = {
            let mut len = 0usize;
            while *sid_str.add(len) != 0 {
                len += 1;
            }
            OsString::from_wide(std::slice::from_raw_parts(sid_str, len))
        };
        LocalFree(sid_str as *mut _);
        let sid_string = sid_os.to_string_lossy().into_owned();

        let mut name = vec![0u16; 256];
        let mut domain = vec![0u16; 256];
        let mut name_len = name.len() as u32;
        let mut domain_len = domain.len() as u32;
        let mut use_type = 0u32;
        if LookupAccountSidW(
            std::ptr::null(),
            sid,
            name.as_mut_ptr(),
            &mut name_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut use_type,
        ) == 0
        {
            let err = GetLastError();
            CloseHandle(token);
            return Err(format!("LookupAccountSidW failed ({err})"));
        }
        CloseHandle(token);
        let name_s = OsString::from_wide(&name[..name_len as usize])
            .to_string_lossy()
            .into_owned();
        let domain_s = OsString::from_wide(&domain[..domain_len as usize])
            .to_string_lossy()
            .into_owned();
        let logon = if domain_s.is_empty() {
            name_s
        } else {
            format!("{domain_s}\\{name_s}")
        };
        Ok((sid_string, logon))
    }
}

/// Re-launch this process elevated via UAC (`runas`). Returns after the
/// elevated process exits, or an error if the user declines / API fails.
pub fn relaunch_elevated(args: &[String]) -> Result<i32, String> {
    #[cfg(windows)]
    {
        windows_relaunch_elevated(args)
    }
    #[cfg(not(windows))]
    {
        let _ = args;
        Err("relaunch_elevated is Windows-only".into())
    }
}

#[cfg(windows)]
fn windows_relaunch_elevated(args: &[String]) -> Result<i32, String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut params = String::new();
    for a in args {
        if !params.is_empty() {
            params.push(' ');
        }
        if a.is_empty() || a.contains(char::is_whitespace) || a.contains('"') {
            params.push('"');
            params.push_str(&a.replace('"', "\"\""));
            params.push('"');
        } else {
            params.push_str(a);
        }
    }
    fn wide(s: &OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }
    let mut exe_w = wide(exe.as_os_str());
    let mut params_w = wide(OsStr::new(&params));
    let mut verb_w = wide(OsStr::new("runas"));
    let mut dir_w = wide(
        exe.parent()
            .map(|p| p.as_os_str())
            .unwrap_or_else(|| OsStr::new(".")),
    );
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb_w.as_mut_ptr();
        info.lpFile = exe_w.as_mut_ptr();
        info.lpParameters = params_w.as_mut_ptr();
        info.lpDirectory = dir_w.as_mut_ptr();
        info.nShow = 0; // SW_HIDE
        if ShellExecuteExW(&mut info) == 0 {
            return Err(
                "UAC elevation was declined or ShellExecuteExW failed (need Administrator for --system)"
                    .into(),
            );
        }
        if info.hProcess.is_null() {
            return Ok(0);
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code: u32 = 0;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        Ok(code as i32)
    }
}
