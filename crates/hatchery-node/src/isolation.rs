//! Node-side isolation: run provider sessions as an unprivileged user, and apply the egress
//! kill-switch rendered by `hatchery_enroll::KillSwitch`. Linux/Unix only; both are opt-in
//! and fail closed (a requested isolation that cannot be set up stops the node).

#![cfg(unix)]

use gate4agent_catalog::AgentRegistry;
use hatchery_enroll::KillSwitch;
use std::ffi::CString;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

static RUN_AS: OnceLock<String> = OnceLock::new();

#[derive(Debug, thiserror::Error)]
pub enum IsolationError {
    #[error("user '{0}' does not exist")]
    UnknownUser(String),
    #[error("run-as user must not be root")]
    RootRefused,
    #[error("kill-switch: {0}")]
    KillSwitch(String),
}

struct Account {
    uid: libc::uid_t,
    gid: libc::gid_t,
    home: String,
}

fn lookup(user: &str) -> Result<Account, IsolationError> {
    let name = CString::new(user).map_err(|_| IsolationError::UnknownUser(user.to_owned()))?;
    // SAFETY: getpwnam returns a pointer into static storage that we copy out immediately.
    let pw = unsafe { libc::getpwnam(name.as_ptr()) };
    if pw.is_null() {
        return Err(IsolationError::UnknownUser(user.to_owned()));
    }
    // SAFETY: non-null checked above.
    let (uid, gid, home) = unsafe {
        let pw = &*pw;
        (pw.pw_uid, pw.pw_gid, std::ffi::CStr::from_ptr(pw.pw_dir).to_string_lossy().into_owned())
    };
    if uid == 0 {
        return Err(IsolationError::RootRefused);
    }
    Ok(Account { uid, gid, home })
}

/// Validate and remember the unprivileged user for provider sessions.
pub fn set_run_as(user: &str) -> Result<(), IsolationError> {
    lookup(user)?;
    let _ = RUN_AS.set(user.to_owned());
    Ok(())
}

pub fn run_as() -> Option<&'static str> {
    RUN_AS.get().map(String::as_str)
}

/// Make every catalog launcher start through `<node> exec-as USER -- <program> <args>`.
/// The version probe keeps using the original catalog, so only spawned sessions change.
pub fn wrap_registry(catalog: AgentRegistry) -> Result<AgentRegistry, String> {
    let Some(user) = run_as() else { return Ok(catalog) };
    let me = std::env::current_exe().map_err(|e| format!("cannot locate the node binary: {e}"))?;
    let mut specs = Vec::new();
    for spec in catalog.iter() {
        let mut spec = spec.clone();
        let mut args = vec!["exec-as".to_owned(), user.to_owned(), "--".to_owned(), spec.launch.program.clone()];
        args.append(&mut spec.launch.fixed_args);
        spec.launch.program = me.to_string_lossy().into_owned();
        spec.launch.fixed_args = args;
        specs.push(spec);
    }
    AgentRegistry::new(specs).map_err(|e| e.to_string())
}

/// `exec-as USER -- program args...`: drop to USER (groups, gid, uid), prove the drop cannot be
/// undone, set HOME/USER/LOGNAME, and exec. Never returns on success.
pub fn exec_as(user: &str, argv: &[String]) -> Result<std::convert::Infallible, String> {
    let account = lookup(user).map_err(|e| e.to_string())?;
    let (program, rest) = argv.split_first().ok_or("exec-as needs a program")?;
    let cuser = CString::new(user).map_err(|e| e.to_string())?;
    // SAFETY: plain libc credential calls, in order: groups, gid, uid. A process that is not root
    // may do this only while it holds CAP_SETUID/CAP_SETGID (see `drop_privileges`); either way
    // the drop must be irreversible before anything is exec'd.
    unsafe {
        if libc::geteuid() != account.uid {
            if libc::initgroups(cuser.as_ptr(), account.gid as _) != 0
                || libc::setgid(account.gid) != 0
                || libc::setuid(account.uid) != 0
            {
                return Err(format!("cannot drop privileges: {}", std::io::Error::last_os_error()));
            }
            // A caller that already ran as a non-root user keeps its capabilities across the id
            // change, so they are removed explicitly: nothing may survive into the session.
            libc::prctl(libc::PR_CAP_AMBIENT, libc::PR_CAP_AMBIENT_CLEAR_ALL as libc::c_ulong, 0, 0, 0);
            let header = CapHeader { version: 0x2008_0522, pid: 0 };
            let none = [CapData { effective: 0, permitted: 0, inheritable: 0 }; 2];
            if libc::syscall(libc::SYS_capset, &header as *const CapHeader, none.as_ptr()) != 0 {
                return Err(format!("cannot clear capabilities: {}", std::io::Error::last_os_error()));
            }
            if libc::setuid(0) == 0 {
                return Err("privilege drop is reversible; refusing to exec".to_owned());
            }
        }
    }
    use std::os::unix::process::CommandExt;
    let error = Command::new(program)
        .args(rest)
        .env("HOME", &account.home)
        .env("USER", user)
        .env("LOGNAME", user)
        .exec();
    Err(format!("cannot exec {program}: {error}"))
}

/// Install the kill-switch with `nft` (preferred) or `iptables-restore`; verify afterwards.
pub fn apply_kill_switch(ks: &KillSwitch) -> Result<&'static str, IsolationError> {
    let fail = |m: String| IsolationError::KillSwitch(m);
    let feed = |tool: &str, args: &[&str], text: &str| -> Result<(), String> {
        let mut child = Command::new(tool)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{tool}: {e}"))?;
        child.stdin.take().expect("piped").write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!("{tool} failed: {}", String::from_utf8_lossy(&out.stderr).trim()))
        }
    };
    // Replace our own table atomically if it already exists.
    let ruleset = format!("table inet {t}\ndelete table inet {t}\n{}", ks.nft_ruleset(), t = KillSwitch::TABLE);
    // A file, not stdin: minimal guests have no /dev/stdin link.
    let rules_file = std::env::temp_dir().join(format!("hatchery-killswitch-{}.nft", std::process::id()));
    std::fs::write(&rules_file, &ruleset).map_err(|e| fail(e.to_string()))?;
    let nft = Command::new("nft")
        .arg("-f")
        .arg(&rules_file)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("nft: {e}"))
        .and_then(|out| {
            if out.status.success() {
                Ok(())
            } else {
                Err(format!("nft failed: {}", String::from_utf8_lossy(&out.stderr).trim()))
            }
        });
    let _ = std::fs::remove_file(&rules_file);
    match nft {
        Ok(()) => {
            let list = Command::new("nft")
                .args(["list", "table", "inet", KillSwitch::TABLE])
                .output()
                .map_err(|e| fail(e.to_string()))?;
            if !list.status.success() {
                return Err(fail("nft accepted the ruleset but the table is not present".into()));
            }
            Ok("nft")
        }
        Err(nft_error) => match ks.iptables_restore() {
            Some(text) => feed("iptables-restore", &[], &text)
                .map(|()| "iptables")
                .map_err(|e| fail(format!("{nft_error}; {e}"))),
            None => Err(fail(nft_error)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_and_unknown_users_are_refused() {
        assert!(matches!(set_run_as("root"), Err(IsolationError::RootRefused)));
        assert!(matches!(set_run_as("no-such-user-hatchery"), Err(IsolationError::UnknownUser(_))));
    }

    #[test]
    fn without_run_as_the_catalog_is_untouched() {
        let catalog = gate4agent_catalog::builtin_registry().clone();
        let before = catalog.iter().map(|s| s.launch.program.clone()).collect::<Vec<_>>();
        let after = wrap_registry(catalog).unwrap();
        assert_eq!(before, after.iter().map(|s| s.launch.program.clone()).collect::<Vec<_>>());
    }
}

const CAP_SETGID: u32 = 6;
const CAP_SETUID: u32 = 7;

#[repr(C)]
struct CapHeader {
    version: u32,
    pid: i32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CapData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

fn last_cap() -> u32 {
    std::fs::read_to_string("/proc/sys/kernel/cap_last_cap").ok().and_then(|t| t.trim().parse().ok()).unwrap_or(40)
}

/// `drop-privileges --user USER -- program args...` (run as root, once, before the long-lived
/// node starts). The program runs as USER with: no_new_privs (no su/sudo/setuid escalation), a
/// capability bounding set of only CAP_SETUID and CAP_SETGID (so it can start sessions as another
/// user with `exec-as`, but can never gain CAP_NET_ADMIN, so the tunnel and the kill-switch
/// installed earlier by `net-up` stay out of its reach), and those two capabilities as ambient.
pub fn drop_privileges(user: &str, argv: &[String]) -> Result<std::convert::Infallible, String> {
    let account = lookup(user).map_err(|e| e.to_string())?;
    let (program, rest) = argv.split_first().ok_or("drop-privileges needs a program")?;
    let cuser = CString::new(user).map_err(|e| e.to_string())?;
    // SAFETY: libc credential and capability calls, each checked; order matters (bounding set and
    // keepcaps first, then ids, then capset, then ambient).
    unsafe {
        if libc::geteuid() != 0 {
            return Err("drop-privileges needs root".to_owned());
        }
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(format!("no_new_privs: {}", std::io::Error::last_os_error()));
        }
        for cap in 0..=last_cap() {
            if cap != CAP_SETUID && cap != CAP_SETGID && libc::prctl(libc::PR_CAPBSET_DROP, cap as libc::c_ulong, 0, 0, 0) != 0 {
                return Err(format!("cannot drop capability {cap}: {}", std::io::Error::last_os_error()));
            }
        }
        if libc::prctl(libc::PR_SET_KEEPCAPS, 1, 0, 0, 0) != 0 {
            return Err(format!("keepcaps: {}", std::io::Error::last_os_error()));
        }
        if libc::initgroups(cuser.as_ptr(), account.gid as _) != 0 || libc::setgid(account.gid) != 0 || libc::setuid(account.uid) != 0 {
            return Err(format!("cannot switch user: {}", std::io::Error::last_os_error()));
        }
        let mask = (1_u32 << CAP_SETUID) | (1_u32 << CAP_SETGID);
        let header = CapHeader { version: 0x2008_0522, pid: 0 };
        let data = [CapData { effective: mask, permitted: mask, inheritable: mask }, CapData { effective: 0, permitted: 0, inheritable: 0 }];
        if libc::syscall(libc::SYS_capset, &header as *const CapHeader, data.as_ptr()) != 0 {
            return Err(format!("capset: {}", std::io::Error::last_os_error()));
        }
        for cap in [CAP_SETUID, CAP_SETGID] {
            if libc::prctl(libc::PR_CAP_AMBIENT, libc::PR_CAP_AMBIENT_RAISE as libc::c_ulong, cap as libc::c_ulong, 0, 0) != 0 {
                return Err(format!("ambient capability {cap}: {}", std::io::Error::last_os_error()));
            }
        }
    }
    use std::os::unix::process::CommandExt;
    let error = Command::new(program)
        .args(rest)
        .env("HOME", &account.home)
        .env("USER", user)
        .env("LOGNAME", user)
        .exec();
    Err(format!("cannot exec {program}: {error}"))
}

/// `net-up --enroll-state DIR [--wg-key FILE] [--interface hatch0] [--kill-switch]` (root): bring up
/// the kernel WireGuard interface from the enrolled state and optionally install the egress
/// kill-switch. After this the long-lived node can run unprivileged with `--no-wg`, and the
/// WireGuard private key can live in a root-only place the node never reads.
pub fn net_up(state_dir: &std::path::Path, wg_key: Option<&std::path::Path>, interface: &str, kill_switch: bool) -> Result<(), String> {
    let (_, state) = crate::enroll_client::load_enrolled_signing_only(state_dir).map_err(|e| e.to_string())?;
    let r = &state.response;
    let need = |v: &Option<String>, n: &str| v.clone().ok_or_else(|| format!("enrolled state lacks {n}"));
    let (addr, peer_addr) = (need(&r.tunnel_ip, "tunnel address")?, need(&r.c2_tunnel_ip, "C2 tunnel address")?);
    let (peer_key, endpoint) = (need(&r.c2_wg_public_key, "C2 public key")?, need(&r.c2_wg_endpoint, "C2 endpoint")?);
    let key_path = wg_key.map(std::path::Path::to_path_buf).unwrap_or_else(|| hatchery_enroll::NodeIdentity::wg_path(state_dir));
    let run = |program: &str, args: &[&str]| -> Result<(), String> {
        let out = Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
        if out.status.success() { Ok(()) } else { Err(format!("{program} {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim())) }
    };
    if kill_switch {
        // First, so there is no window in which the interface is up and egress is open.
        let ks = KillSwitch::new(interface, &endpoint).map_err(|e| e.to_string())?;
        apply_kill_switch(&ks).map_err(|e| e.to_string())?;
    }
    let _ = Command::new("ip").args(["link", "del", "dev", interface]).output();
    run("ip", &["link", "add", "dev", interface, "type", "wireguard"])?;
    let family = if peer_addr.contains(':') { 128 } else { 32 };
    let allowed = format!("{peer_addr}/{family}");
    run("wg", &["set", interface, "private-key", &key_path.to_string_lossy(), "peer", &peer_key, "endpoint", &endpoint, "allowed-ips", &allowed, "persistent-keepalive", "25"])?;
    run("ip", &["addr", "replace", &format!("{addr}/{family}"), "dev", interface])?;
    run("ip", &["link", "set", "up", "dev", interface])?;
    run("ip", &["route", "replace", &allowed, "dev", interface])?;
    Ok(())
}
