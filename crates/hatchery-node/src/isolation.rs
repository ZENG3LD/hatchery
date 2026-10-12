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
    // SAFETY: plain libc credential calls, checked in order: groups first, then gid, then uid.
    unsafe {
        if libc::geteuid() == 0 {
            if libc::initgroups(cuser.as_ptr(), account.gid as _) != 0
                || libc::setgid(account.gid) != 0
                || libc::setuid(account.uid) != 0
            {
                return Err(format!("cannot drop privileges: {}", std::io::Error::last_os_error()));
            }
            if libc::setuid(0) == 0 {
                return Err("privilege drop is reversible; refusing to exec".to_owned());
            }
        } else if libc::geteuid() != account.uid {
            return Err("exec-as needs root to switch user".to_owned());
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
    let nft = feed("nft", &["-f", "-"], &ruleset);
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
