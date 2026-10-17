//! Windows process-creation helpers for non-PTY children.
//!
//! PTY / ConPTY spawns must keep a console; everything else (ACP, app-server,
//! m4a-agent, spawn profiles, probes) must not pop a visible window.

/// `CREATE_NO_WINDOW` (0x08000000). Safe to OR with other creation flags.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Apply [`CREATE_NO_WINDOW`] on Windows. No-op elsewhere. Does not touch
/// program, args, env, cwd, or stdio. Call once: a second `creation_flags`
/// replaces the previous value.
pub fn hide_console(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// Same for tokio's command (via `as_std_mut`).
pub fn hide_console_tokio(command: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
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
    }
}
