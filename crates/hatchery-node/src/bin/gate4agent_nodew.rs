//! Windows GUI-subsystem trampoline: no console window is allocated.
//!
//! Forwards to `gate4agent-node run-supervised -- <args…>` when invoked as a
//! scheduled task / service helper. Interactive CLI stays on `gate4agent-node`
//! (console subsystem) so m4a-adopt and friends still print.
#![cfg(windows)]
#![windows_subsystem = "windows"]

fn main() {
    // Re-exec the console binary next to us as run-supervised, or ourselves
    // with the same argv prefixed. Prefer sibling gate4agent-node.exe.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe = std::env::current_exe().expect("current_exe");
    let sibling = exe
        .parent()
        .map(|p| p.join("gate4agent-node.exe"))
        .filter(|p| p.is_file())
        .unwrap_or(exe.clone());

    let mut cmd = std::process::Command::new(&sibling);
    // If caller already passed run-supervised, honor it; else wrap.
    let already = args.first().map(String::as_str) == Some("run-supervised");
    if !already {
        cmd.arg("run-supervised").arg("--");
    }
    cmd.args(&args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    match cmd.spawn() {
        Ok(mut child) => {
            let _ = child.wait();
        }
        Err(e) => {
            // Last resort: write next to exe — no console to print to.
            if let Some(dir) = exe.parent() {
                let _ = std::fs::write(dir.join("gate4agent-nodew.spawn-error"), e.to_string());
            }
            std::process::exit(1);
        }
    }
}
