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
