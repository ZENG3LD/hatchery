//! Windows GUI-subsystem trampoline: no console window is allocated.
//!
//! Forwards to sibling `gate4agent-node.exe run-supervised -- <args…>`.
//! Child stdio is owned by run-supervised (files under enroll-state), not null.
#![cfg(windows)]
#![windows_subsystem = "windows"]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            let _ = std::fs::write("gate4agent-nodew.spawn-error", e.to_string());
            std::process::exit(1);
        }
    };
    let sibling = exe
        .parent()
        .map(|p| p.join("gate4agent-node.exe"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| exe.clone());

    let mut cmd = std::process::Command::new(&sibling);
    let already = args.first().map(String::as_str) == Some("run-supervised");
    if !already {
        cmd.arg("run-supervised").arg("--");
    }
    cmd.args(&args);
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    // Inherit nothing from a GUI parent; run-supervised opens its own log files.
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    match cmd.spawn() {
        Ok(mut child) => {
            let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(1);
            std::process::exit(code);
        }
        Err(e) => {
            if let Some(dir) = exe.parent() {
                let _ = std::fs::write(dir.join("gate4agent-nodew.spawn-error"), e.to_string());
            }
            std::process::exit(1);
        }
    }
}
