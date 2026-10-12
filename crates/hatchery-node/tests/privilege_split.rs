//! Needs root (it switches users); skipped otherwise. Run with:
//!   cargo test -p hatchery-node --test privilege_split --no-run   (note the binary path)
//!   sudo <that binary>
#![cfg(target_os = "linux")]

use std::process::Command;

fn root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

fn node() -> &'static str {
    env!("CARGO_BIN_EXE_gate4agent-node")
}

fn run(args: &[&str]) -> (bool, String) {
    let out = Command::new(node()).args(args).output().unwrap();
    (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

fn field(text: &str, name: &str) -> String {
    text.lines().find_map(|l| l.strip_prefix(name)).map(|v| v.trim().to_owned()).unwrap_or_default()
}

#[test]
fn long_lived_node_user_keeps_only_setid_capabilities_and_cannot_touch_the_network_or_root_files() {
    if !root() {
        eprintln!("skipped: needs root");
        return;
    }
    let dir = std::env::temp_dir().join(format!("hatchery-split-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let secret = dir.join("wg.key");
    std::fs::write(&secret, "root-only").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let inner = format!(
        "id -u; grep -E '^(CapEff|CapBnd|CapAmb|NoNewPrivs):' /proc/self/status; \
         ip link add d0 type dummy >/dev/null 2>&1 && echo NETADMIN=yes || echo NETADMIN=no; \
         cat {secret} >/dev/null 2>&1 && echo ROOTFILE=readable || echo ROOTFILE=denied; \
         {node} exec-as daemon -- sh -c 'id -u; grep -E \"^(CapEff|CapBnd|CapAmb):\" /proc/self/status' 2>&1; \
         {node} exec-as root -- id 2>&1 | head -1",
        secret = secret.display(),
        node = node(),
    );
    let (ok, text) = run(&["drop-privileges", "--user", "nobody", "--", "sh", "-c", &inner]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ok, "{text}");
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("65534"), "node user is nobody: {text}");
    // setuid+setgid only: bits 6 and 7 = 0xc0
    assert_eq!(field(&text, "CapEff:"), "00000000000000c0", "{text}");
    assert_eq!(field(&text, "CapBnd:"), "00000000000000c0", "{text}");
    assert_eq!(field(&text, "CapAmb:"), "00000000000000c0", "{text}");
    assert_eq!(field(&text, "NoNewPrivs:"), "1", "{text}");
    assert!(text.contains("NETADMIN=no"), "{text}");
    assert!(text.contains("ROOTFILE=denied"), "{text}");
    // The session user obtained through exec-as has no capabilities at all and a closed bounding set.
    let daemon_part = text.split("ROOTFILE=denied").nth(1).unwrap();
    assert!(daemon_part.contains("\n1\n"), "daemon uid is 1: {daemon_part}");
    assert!(daemon_part.contains("CapEff:\t0000000000000000"), "{daemon_part}");
    assert!(daemon_part.contains("CapAmb:\t0000000000000000"), "{daemon_part}");
    assert!(daemon_part.contains("run-as user must not be root"), "{daemon_part}");
}

#[test]
fn drop_privileges_refuses_root_and_non_root_callers() {
    if !root() {
        eprintln!("skipped: needs root");
        return;
    }
    let (ok, text) = run(&["drop-privileges", "--user", "root", "--", "true"]);
    assert!(!ok && text.contains("must not be root"), "{text}");
    let (ok, text) = run(&["drop-privileges", "--user", "no-such-user-hatchery", "--", "true"]);
    assert!(!ok && text.contains("does not exist"), "{text}");
}
