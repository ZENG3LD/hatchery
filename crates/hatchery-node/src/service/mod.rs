//! OS-native supervision for the node binary: install / uninstall / status /
//! restart, plus an in-process `run-supervised` parent for platforms where the
//! OS restart policy is incomplete (Windows Task Scheduler).

mod render;

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::node_log;
use crate::windows_process::{
    current_user_identity, free_console, hide_console, ignore_console_close_events,
    relaunch_elevated, set_creation_flags, CREATE_NO_WINDOW,
};

pub use render::{
    render_launchd_plist, render_schtasks_xml, render_systemd_unit, LaunchPlist, SchtasksXml,
    SystemdUnit,
};

/// Prefixes of environment variables captured at install time into the unit.
const ENV_PREFIXES: &[&str] = &["GATE4AGENT_", "HATCHERY_", "RUST_LOG"];

#[derive(Clone, Debug)]
pub struct ServiceSpec {
    pub system: bool,
    pub exe: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub enroll_state: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct ServiceStatus {
    pub installed: bool,
    pub running: bool,
    pub scope: &'static str,
    pub unit_name: String,
    pub detail: String,
    /// Best-effort probe of the node HTTP health endpoint from saved args / env.
    pub node_health: String,
}

impl ServiceSpec {
    pub fn from_install_args(system: bool, node_args: &[String]) -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        let enroll_state = extract_enroll_state(node_args);
        let env = capture_env();
        Ok(Self {
            system,
            exe,
            args: node_args.to_vec(),
            env,
            enroll_state,
        })
    }

    pub fn unit_name(&self) -> String {
        match &self.enroll_state {
            Some(p) => {
                let key = p.to_string_lossy();
                let digest = short_digest(&key);
                format!("gate4agent-node-{digest}")
            }
            None => "gate4agent-node".to_owned(),
        }
    }

    pub fn launchd_label(&self) -> String {
        format!("com.gate4agent.node.{}", self.unit_name())
    }
}

fn extract_enroll_state(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|w| w[0] == "--enroll-state")
        .map(|w| PathBuf::from(&w[1]))
}

fn capture_env() -> Vec<(String, String)> {
    let mut out = BTreeMap::new();
    for (k, v) in std::env::vars() {
        if ENV_PREFIXES.iter().any(|p| k.starts_with(p)) || k == "RUST_LOG" {
            out.insert(k, v);
        }
    }
    out.into_iter().collect()
}

fn windows_task_exe(exe: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let w = exe.with_file_name("gate4agent-nodew.exe");
        if w.is_file() {
            return w;
        }
    }
    let _ = exe;
    exe.to_path_buf()
}

fn short_digest(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(s.as_bytes());
    hex::encode(&h[..6])
}

fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "HOME/USERPROFILE is unset".to_owned())
}

/// Install the node under the OS supervisor for this platform.
pub fn install(spec: &ServiceSpec) -> Result<String, String> {
    require_admin_if_system(spec.system)?;
    #[cfg(target_os = "linux")]
    {
        return install_linux(spec);
    }
    #[cfg(target_os = "macos")]
    {
        return install_macos(spec);
    }
    #[cfg(windows)]
    {
        return install_windows(spec);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = spec;
        Err("service install is unsupported on this OS".into())
    }
}

pub fn uninstall(system: bool) -> Result<String, String> {
    require_admin_if_system(system)?;
    // Reconstruct name from saved marker if present; else default.
    let name = saved_unit_name(system).unwrap_or_else(|| "gate4agent-node".into());
    #[cfg(target_os = "linux")]
    {
        return uninstall_linux(system, &name);
    }
    #[cfg(target_os = "macos")]
    {
        return uninstall_macos(system, &name);
    }
    #[cfg(windows)]
    {
        return uninstall_windows(system, &name);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (system, name);
        Err("service uninstall is unsupported on this OS".into())
    }
}

pub fn status(system: bool) -> Result<ServiceStatus, String> {
    let name = saved_unit_name(system).unwrap_or_else(|| "gate4agent-node".into());
    #[cfg(target_os = "linux")]
    {
        return status_linux(system, &name);
    }
    #[cfg(target_os = "macos")]
    {
        return status_macos(system, &name);
    }
    #[cfg(windows)]
    {
        return status_windows(system, &name);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (system, name);
        Err("service-status is unsupported on this OS".into())
    }
}

pub fn restart(system: bool) -> Result<String, String> {
    require_admin_if_system(system)?;
    let name = saved_unit_name(system).unwrap_or_else(|| "gate4agent-node".into());
    #[cfg(target_os = "linux")]
    {
        return restart_linux(system, &name);
    }
    #[cfg(target_os = "macos")]
    {
        return restart_macos(system, &name);
    }
    #[cfg(windows)]
    {
        return restart_windows(system, &name);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (system, name);
        Err("service-restart is unsupported on this OS".into())
    }
}

fn require_admin_if_system(system: bool) -> Result<(), String> {
    if !system {
        return Ok(());
    }
    if is_elevated() {
        Ok(())
    } else {
        Err("--system requires administrator / root privileges".into())
    }
}

#[cfg(windows)]
fn is_elevated_windows() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elev = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut needed = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elev as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut needed,
        );
        CloseHandle(token);
        ok != 0 && elev.TokenIsElevated != 0
    }
}

fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(windows)]
    {
        is_elevated_windows()
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        false
    }
}

fn marker_path(system: bool) -> Result<PathBuf, String> {
    let base = if system {
        #[cfg(unix)]
        {
            PathBuf::from("/var/lib/gate4agent-node")
        }
        #[cfg(not(unix))]
        {
            std::env::var_os("ProgramData")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
                .join("gate4agent-node")
        }
    } else {
        home_dir()?.join(".local").join("state").join("gate4agent-node")
    };
    Ok(base.join("service-unit-name"))
}

fn save_unit_name(system: bool, name: &str) -> Result<(), String> {
    let path = marker_path(system)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(&path, name).map_err(|e| format!("write {}: {e}", path.display()))
}

fn saved_unit_name(system: bool) -> Option<String> {
    let path = marker_path(system).ok()?;
    fs::read_to_string(path).ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

fn clear_unit_name(system: bool) {
    if let Ok(path) = marker_path(system) {
        let _ = fs::remove_file(path);
    }
}

// ----- Linux -----

#[cfg(target_os = "linux")]
fn unit_dir(system: bool) -> Result<PathBuf, String> {
    if system {
        Ok(PathBuf::from("/etc/systemd/system"))
    } else {
        Ok(home_dir()?.join(".config").join("systemd").join("user"))
    }
}

#[cfg(target_os = "linux")]
fn install_linux(spec: &ServiceSpec) -> Result<String, String> {
    let name = spec.unit_name();
    let dir = unit_dir(spec.system)?;
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{name}.service"));
    // systemd itself restarts; supervised wrapper not required on Linux.
    let body = render_systemd_unit(&SystemdUnit {
        description: "gate4agent-node",
        exe: &spec.exe.to_string_lossy(),
        args: &spec.args,
        env: &spec.env,
        supervised: false,
    });
    fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))?;
    save_unit_name(spec.system, &name)?;

    let mut notes = Vec::new();
    if !spec.system {
        match Command::new("loginctl")
            .args(["enable-linger"])
            .status()
        {
            Ok(s) if s.success() => notes.push("linger enabled".into()),
            Ok(_) | Err(_) => notes.push(
                "warning: loginctl enable-linger failed or unavailable; user units may stop at logout"
                    .into(),
            ),
        }
    }
    match run_ok(&mut systemctl_cmd(spec.system).args(["daemon-reload"]))
        .and_then(|_| run_ok(&mut systemctl_cmd(spec.system).args(["enable", "--now", &name])))
    {
        Ok(()) => notes.push(format!("installed and started {name} ({})", path.display())),
        Err(e) => notes.push(format!(
            "wrote {} but systemctl failed ({e}); start later when the user bus is up",
            path.display()
        )),
    }
    Ok(notes.join("; "))
}

#[cfg(target_os = "linux")]
fn uninstall_linux(system: bool, name: &str) -> Result<String, String> {
    let _ = systemctl_cmd(system).args(["disable", "--now", name]).status();
    let path = unit_dir(system)?.join(format!("{name}.service"));
    let _ = fs::remove_file(&path);
    let _ = systemctl_cmd(system).args(["daemon-reload"]).status();
    clear_unit_name(system);
    Ok(format!("removed {name}"))
}

#[cfg(target_os = "linux")]
fn status_linux(system: bool, name: &str) -> Result<ServiceStatus, String> {
    let path = unit_dir(system)?.join(format!("{name}.service"));
    let installed = path.is_file();
    let out = systemctl_cmd(system).args(["is-active", name]).output();
    let (running, detail) = match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_owned();
            (s == "active", s)
        }
        Err(e) => (false, e.to_string()),
    };
    Ok(ServiceStatus {
        installed,
        running,
        scope: if system { "system" } else { "user" },
        unit_name: name.to_owned(),
        detail,
        node_health: probe_node_health(),
    })
}

#[cfg(target_os = "linux")]
fn restart_linux(system: bool, name: &str) -> Result<String, String> {
    run_ok(&mut systemctl_cmd(system).args(["restart", name]))?;
    Ok(format!("restarted {name}"))
}

#[cfg(target_os = "linux")]
fn systemctl_cmd(system: bool) -> Command {
    let mut c = Command::new("systemctl");
    if !system {
        c.arg("--user");
    }
    c
}

// ----- macOS -----

#[cfg(target_os = "macos")]
fn plist_path(system: bool, label: &str) -> Result<PathBuf, String> {
    if system {
        Ok(PathBuf::from("/Library/LaunchDaemons").join(format!("{label}.plist")))
    } else {
        Ok(home_dir()?
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{label}.plist")))
    }
}

#[cfg(target_os = "macos")]
fn install_macos(spec: &ServiceSpec) -> Result<String, String> {
    let label = spec.launchd_label();
    let path = plist_path(spec.system, &label)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let body = render_launchd_plist(&LaunchPlist {
        label: &label,
        exe: &spec.exe.to_string_lossy(),
        args: &spec.args,
        env: &spec.env,
        supervised: false, // KeepAlive is enough on launchd
    });
    fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))?;
    save_unit_name(spec.system, &spec.unit_name())?;
    let domain = if spec.system {
        "system".to_owned()
    } else {
        format!("gui/{}", unsafe { libc::getuid() })
    };
    let _ = Command::new("launchctl")
        .args(["bootout", &domain, &path.to_string_lossy()])
        .status();
    run_ok(
        &mut Command::new("launchctl").args(["bootstrap", &domain, &path.to_string_lossy()]),
    )?;
    let _ = Command::new("launchctl")
        .args(["enable", &format!("{domain}/{label}")])
        .status();
    Ok(format!("installed {label} ({})", path.display()))
}

#[cfg(target_os = "macos")]
fn uninstall_macos(system: bool, name: &str) -> Result<String, String> {
    let label = format!("com.gate4agent.node.{name}");
    let path = plist_path(system, &label)?;
    let domain = if system {
        "system".to_owned()
    } else {
        format!("gui/{}", unsafe { libc::getuid() })
    };
    let _ = Command::new("launchctl")
        .args(["bootout", &domain, &path.to_string_lossy()])
        .status();
    let _ = fs::remove_file(&path);
    clear_unit_name(system);
    Ok(format!("removed {label}"))
}

#[cfg(target_os = "macos")]
fn status_macos(system: bool, name: &str) -> Result<ServiceStatus, String> {
    let label = format!("com.gate4agent.node.{name}");
    let path = plist_path(system, &label)?;
    let installed = path.is_file();
    let out = Command::new("launchctl")
        .args(["print", &format!("system/{label}")])
        .output();
    // try user domain too
    let running = match out {
        Ok(o) if o.status.success() => true,
        _ => Command::new("launchctl")
            .args(["list", &label])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
    };
    Ok(ServiceStatus {
        installed,
        running,
        scope: if system { "system" } else { "user" },
        unit_name: name.to_owned(),
        detail: label,
        node_health: probe_node_health(),
    })
}

#[cfg(target_os = "macos")]
fn restart_macos(system: bool, name: &str) -> Result<String, String> {
    let label = format!("com.gate4agent.node.{name}");
    let path = plist_path(system, &label)?;
    let domain = if system {
        "system".to_owned()
    } else {
        format!("gui/{}", unsafe { libc::getuid() })
    };
    let _ = Command::new("launchctl")
        .args(["kickstart", "-k", &format!("{domain}/{label}")])
        .status();
    let _ = path;
    Ok(format!("restarted {label}"))
}

// ----- Windows -----

#[cfg(windows)]
fn install_windows(spec: &ServiceSpec) -> Result<String, String> {
    let name = spec.unit_name();
    let (user_sid, user_logon) = current_user_identity()
        .map_err(|e| format!("cannot resolve current user SID for Task Scheduler: {e}"))?;
    if spec.system {
        // Windows Service via sc.exe; binPath runs run-supervised so the
        // service process is the parent (service-main is approximate: we use
        // a Task Scheduler SYSTEM task when sc create of a non-service binary
        // is refused — see below).
        return install_windows_system(spec, &name);
    }
    let dir = home_dir()?
        .join("AppData")
        .join("Local")
        .join("gate4agent-node");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let xml_path = dir.join(format!("{name}.xml"));
    let cwd = home_dir()?;
    let task_exe = windows_task_exe(&spec.exe);
    let use_nodew = task_exe
        .file_name()
        .and_then(|s| s.to_str())
        == Some("gate4agent-nodew.exe");
    let xml = render_schtasks_xml(&SchtasksXml {
        task_name: &name,
        exe: &task_exe.to_string_lossy(),
        args: &spec.args,
        working_dir: &cwd.to_string_lossy(),
        author: "gate4agent-node",
        supervised: !use_nodew,
        user_sid: &user_sid,
        user_logon: &user_logon,
    });
    // Write UTF-16 LE with BOM — schtasks expects it for /XML.
    let mut encoded: Vec<u8> = vec![0xFF, 0xFE];
    for u in xml.encode_utf16() {
        encoded.extend_from_slice(&u.to_le_bytes());
    }
    fs::write(&xml_path, encoded).map_err(|e| format!("write {}: {e}", xml_path.display()))?;
    // Persist env into a sidecar the supervised parent reads? Task XML has no
    // env block in our template; bake GATE4AGENT_* into a small env file.
    write_env_file(&dir.join(format!("{name}.env")), &spec.env)?;
    let _ = Command::new("schtasks")
        .args(["/Delete", "/TN", &name, "/F"])
        .status();
    run_ok(&mut Command::new("schtasks").args([
        "/Create",
        "/TN",
        &name,
        "/XML",
        &xml_path.to_string_lossy(),
        "/F",
    ]))?;
    let _ = Command::new("schtasks")
        .args(["/Run", "/TN", &name])
        .status();
    save_unit_name(false, &name)?;
    Ok(format!(
        "installed scheduled task {name}; upgrade: gate4agent-node service-restart then cargo install then service-restart"
    ))
}

#[cfg(windows)]
fn install_windows_system(spec: &ServiceSpec, name: &str) -> Result<String, String> {
    let (user_sid, user_logon) = current_user_identity()
        .map_err(|e| format!("cannot resolve current user SID: {e}"))?;
    // Prefer a SYSTEM scheduled task with highest privileges — reliable without
    // a native service control handler. Recovery is run-supervised + RestartOnFailure.
    let dir = PathBuf::from(
        std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData")),
    )
    .join("gate4agent-node");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let xml_path = dir.join(format!("{name}.xml"));
    let task_exe = windows_task_exe(&spec.exe);
    let use_nodew = task_exe
        .file_name()
        .and_then(|s| s.to_str())
        == Some("gate4agent-nodew.exe");
    let mut xml = render_schtasks_xml(&SchtasksXml {
        task_name: name,
        exe: &task_exe.to_string_lossy(),
        args: &spec.args,
        working_dir: &dir.to_string_lossy(),
        author: "gate4agent-node",
        supervised: !use_nodew,
        user_sid: &user_sid,
        user_logon: &user_logon,
    });
    xml = xml.replace(
        "<RunLevel>LeastPrivilege</RunLevel>",
        "<RunLevel>HighestAvailable</RunLevel>",
    );
    xml = xml.replace(
        "<LogonType>InteractiveToken</LogonType>",
        "<LogonType>S4U</LogonType>",
    );
    let mut encoded: Vec<u8> = vec![0xFF, 0xFE];
    for u in xml.encode_utf16() {
        encoded.extend_from_slice(&u.to_le_bytes());
    }
    fs::write(&xml_path, encoded).map_err(|e| e.to_string())?;
    write_env_file(&dir.join(format!("{name}.env")), &spec.env)?;
    let _ = Command::new("schtasks")
        .args(["/Delete", "/TN", &name, "/F"])
        .status();
    let status = Command::new("schtasks")
        .args([
            "/Create",
            "/TN",
            name,
            "/XML",
            &xml_path.to_string_lossy(),
            "/RU",
            "SYSTEM",
            "/F",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !status.status.success() {
        return Err(format!(
            "--system install failed (need Administrator): {}",
            String::from_utf8_lossy(&status.stderr)
        ));
    }
    let _ = Command::new("schtasks")
        .args(["/Run", "/TN", name])
        .status();
    save_unit_name(true, name)?;
    Ok(format!("installed SYSTEM task {name}"))
}

#[cfg(windows)]
fn uninstall_windows(system: bool, name: &str) -> Result<String, String> {
    let _ = Command::new("schtasks")
        .args(["/End", "/TN", name])
        .status();
    let st = Command::new("schtasks")
        .args(["/Delete", "/TN", name, "/F"])
        .output()
        .map_err(|e| e.to_string())?;
    clear_unit_name(system);
    if st.status.success() {
        Ok(format!("removed task {name}"))
    } else {
        Err(format!(
            "schtasks delete failed: {}",
            String::from_utf8_lossy(&st.stderr)
        ))
    }
}

#[cfg(windows)]
fn status_windows(system: bool, name: &str) -> Result<ServiceStatus, String> {
    let out = Command::new("schtasks")
        .args(["/Query", "/TN", name, "/V", "/FO", "LIST"])
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let installed = out.status.success();
    let running = text.to_ascii_lowercase().contains("running");
    Ok(ServiceStatus {
        installed,
        running,
        scope: if system { "system" } else { "user" },
        unit_name: name.to_owned(),
        detail: text.lines().take(8).collect::<Vec<_>>().join(" | "),
        node_health: probe_node_health(),
    })
}

#[cfg(windows)]
fn restart_windows(system: bool, name: &str) -> Result<String, String> {
    let _ = system;
    let _ = Command::new("schtasks")
        .args(["/End", "/TN", name])
        .status();
    std::thread::sleep(Duration::from_secs(2));
    run_ok(&mut Command::new("schtasks").args(["/Run", "/TN", name]))?;
    Ok(format!(
        "restarted {name}; Windows locks a running exe — stop before cargo install, then service-restart"
    ))
}

#[cfg(windows)]
fn write_env_file(path: &Path, env: &[(String, String)]) -> Result<(), String> {
    let mut f = File::create(path).map_err(|e| e.to_string())?;
    for (k, v) in env {
        writeln!(f, "{k}={v}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn probe_node_health() -> String {
    let addr = std::env::var("GATE4AGENT_HEALTH_URL")
        .or_else(|_| {
            std::env::var("GATE4AGENT_API_LISTEN").map(|a| format!("http://{a}/health"))
        })
        .unwrap_or_else(|_| "http://127.0.0.1:18330/health".into());
    let hostport = addr
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("127.0.0.1:18330");
    use std::io::{Read, Write};
    use std::net::ToSocketAddrs;
    let sock = match hostport.to_socket_addrs() {
        Ok(mut i) => match i.next() {
            Some(a) => a,
            None => return "bad_addr".into(),
        },
        Err(_) => return "bad_addr".into(),
    };
    let mut stream = match std::net::TcpStream::connect_timeout(&sock, Duration::from_secs(2)) {
        Ok(s) => s,
        Err(_) => return "down".into(),
    };
    let req = format!("GET /health HTTP/1.1
Host: {hostport}
Connection: close

");
    if stream.write_all(req.as_bytes()).is_err() {
        return "unreachable".into();
    }
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).unwrap_or(0);
    let body = String::from_utf8_lossy(&buf[..n]);
    if body.contains("\"ok\":true") || body.contains("\"ok\": true") {
        "ok".into()
    } else if n == 0 {
        "no_response".into()
    } else {
        "responded_not_ok".into()
    }
}

fn run_ok(cmd: &mut Command) -> Result<(), String> {
    hide_console(cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let combined = format!("{stdout} {stderr}").to_ascii_lowercase();
        let hint = if combined.contains("access is denied") || combined.contains("access denied") {
            " (Access denied: for a user-level task the XML must include your UserId/SID; try reinstalling this build, or run from an elevated prompt for --system)"
        } else {
            ""
        };
        Err(format!(
            "command failed{hint}: stdout={stdout} stderr={stderr}"
        ))
    }
}

// ----- run-supervised + instance lock + panic hook -----

/// Hold an exclusive lock on `{enroll-state}/node.instance.lock`.
pub struct InstanceLock {
    _file: File,
    path: PathBuf,
}

pub fn acquire_instance_lock(enroll_state: &Path) -> Result<InstanceLock, String> {
    fs::create_dir_all(enroll_state).map_err(|e| e.to_string())?;
    let path = enroll_state.join("node.instance.lock");
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        OpenOptions::new()
            .create(true)
            .write(true)
            .share_mode(0) // deny share: second open fails while held
            .open(&path)
            .map_err(|e| {
                format!(
                    "another gate4agent-node already holds {} ({e})",
                    path.display()
                )
            })?
    };
    #[cfg(not(windows))]
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            return Err(format!(
                "another gate4agent-node already holds {}",
                path.display()
            ));
        }
    }
    Ok(InstanceLock { _file: file, path })
}

pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_owned()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "panic".into()
        };
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".into());
        let _ = node_log::init_from_env();
        node_log::error(
            "process_panic",
            &[("message", msg), ("location", loc)],
        );
        prev(info);
    }));
}

/// Parent process: respawn `current_exe` with `child_args` until forever.
///
/// Child stdout/stderr go to files under the enroll-state (or a temp dir) so a
/// headless Windows spawn never inherits invalid console handles (which made
/// `println!`/`eprintln!` panic). Interactive CLI never enters this path.
pub fn run_supervised(child_args: &[String]) -> ! {
    apply_sidecar_env(child_args);
    ensure_node_log_from_args(child_args);
    let _ = node_log::init_from_env();
    install_panic_hook();
    free_console();
    ignore_console_close_events();
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            node_log::error("process_exit", &[("reason", format!("current_exe: {e}"))]);
            std::process::exit(1);
        }
    };
    let log_dir = child_log_dir(child_args);
    let mut n: u64 = 0;
    let mut backoff = Duration::from_secs(1);
    loop {
        n = n.saturating_add(1);
        let stdout_path = log_dir.join("node.stdout.log");
        let stderr_path = log_dir.join("node.stderr.log");
        rotate_log_file(&stdout_path, 2_000_000);
        rotate_log_file(&stderr_path, 2_000_000);
        let mut cmd = Command::new(&exe);
        cmd.args(child_args).stdin(Stdio::null());
        match (OpenOptions::new().create(true).append(true).open(&stdout_path),
               OpenOptions::new().create(true).append(true).open(&stderr_path))
        {
            (Ok(out), Ok(err)) => {
                cmd.stdout(Stdio::from(out)).stderr(Stdio::from(err));
            }
            _ => {
                // Last resort: inherit (Unix) or null (Windows) — never panic.
                #[cfg(windows)]
                {
                    cmd.stdout(Stdio::null()).stderr(Stdio::null());
                }
                #[cfg(not(windows))]
                {
                    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
                }
            }
        }
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW only — not DETACHED_PROCESS: with redirected
            // stdio, DETACHED was redundant and historically hid failures.
            set_creation_flags(&mut cmd, CREATE_NO_WINDOW);
        }
        cmd.env("GATE4AGENT_SUPERVISED_CHILD", "1");
        // Re-apply sidecar env each spawn (install may have refreshed it).
        for (k, v) in load_sidecar_env(child_args) {
            cmd.env(k, v);
        }
        node_log::info(
            "supervised_start",
            &[
                ("n", n.to_string()),
                ("args_len", child_args.len().to_string()),
                ("stderr_log", stderr_path.display().to_string()),
            ],
        );
        let status = match cmd.status() {
            Ok(s) => s,
            Err(e) => {
                node_log::error(
                    "process_exit",
                    &[
                        ("reason", format!("spawn: {e}")),
                        ("code", "spawn_error".into()),
                        ("supervised_n", n.to_string()),
                    ],
                );
                node_log::warn(
                    "supervised_restart",
                    &[("n", n.to_string()), ("backoff_secs", backoff.as_secs().to_string())],
                );
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(30));
                continue;
            }
        };
        let code = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".into());
        let stderr_tail = read_tail(&stderr_path, 4_000);
        node_log::warn(
            "process_exit",
            &[
                ("reason", "child_exit".into()),
                ("code", code.clone()),
                ("supervised_n", n.to_string()),
                ("stderr_tail", stderr_tail),
            ],
        );
        node_log::warn(
            "supervised_restart",
            &[("n", n.to_string()), ("backoff_secs", backoff.as_secs().to_string())],
        );
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(Duration::from_secs(30));
        if status.success() {
            backoff = Duration::from_secs(1);
        }
    }
}

fn child_log_dir(child_args: &[String]) -> PathBuf {
    if let Some(p) = extract_enroll_state(child_args) {
        let _ = fs::create_dir_all(&p);
        return p;
    }
    std::env::temp_dir().join("gate4agent-node-supervised")
}

fn ensure_node_log_from_args(child_args: &[String]) {
    if std::env::var_os(crate::node_log::env_name()).is_some() {
        return;
    }
    if let Some(dir) = extract_enroll_state(child_args) {
        let path = crate::node_log::path_beside_state(&dir);
        // SAFETY: single-threaded before spawn loop; sets log path for parent.
        std::env::set_var(crate::node_log::env_name(), &path);
    }
}

fn sidecar_env_path(child_args: &[String]) -> Option<PathBuf> {
    let name = match extract_enroll_state(child_args) {
        Some(p) => {
            let key = p.to_string_lossy();
            format!("gate4agent-node-{}", short_digest(&key))
        }
        None => "gate4agent-node".into(),
    };
    #[cfg(windows)]
    {
        let base = home_dir().ok()?.join("AppData").join("Local").join("gate4agent-node");
        return Some(base.join(format!("{name}.env")));
    }
    #[cfg(not(windows))]
    {
        let base = home_dir().ok()?.join(".local").join("state").join("gate4agent-node");
        Some(base.join(format!("{name}.env")))
    }
}

fn load_sidecar_env(child_args: &[String]) -> Vec<(String, String)> {
    let Some(path) = sidecar_env_path(child_args) else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.push((k.to_owned(), v.to_owned()));
        }
    }
    out
}

fn apply_sidecar_env(child_args: &[String]) {
    for (k, v) in load_sidecar_env(child_args) {
        if std::env::var_os(&k).is_none() {
            std::env::set_var(&k, &v);
        }
    }
}

fn rotate_log_file(path: &Path, max_bytes: u64) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.len() < max_bytes {
        return;
    }
    let bak = path.with_extension("log.1");
    let _ = fs::remove_file(&bak);
    let _ = fs::rename(path, bak);
}

fn read_tail(path: &Path, max_chars: usize) -> String {
    let Ok(data) = fs::read(path) else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&data);
    let t = text.trim();
    if t.len() <= max_chars {
        return t.to_owned();
    }
    t[t.len() - max_chars..].to_owned()
}

/// Parse `install|uninstall|service-status|service-restart|run-supervised`.
/// Returns `true` if handled (caller should return).
pub fn dispatch(raw_args: &[String]) -> Result<bool, String> {
    let Some(cmd) = raw_args.first().map(String::as_str) else {
        return Ok(false);
    };
    match cmd {
        "install" => {
            let (system, rest) = split_system_and_args(&raw_args[1..]);
            let node_args = strip_leading_dd(&rest);
            if node_args.is_empty() {
                return Err("install requires node arguments after optional --system (and optional --)".into());
            }
            #[cfg(windows)]
            if system && !is_elevated_windows() {
                let mut elevated_args = vec!["install".into(), "--system".into(), "--".into()];
                elevated_args.extend(node_args);
                let code = relaunch_elevated(&elevated_args)?;
                if code != 0 {
                    return Err(format!("elevated install exited with code {code}"));
                }
                println!("elevated --system install finished");
                return Ok(true);
            }
            let spec = ServiceSpec::from_install_args(system, &node_args)?;
            let msg = install(&spec)?;
            println!("{msg}");
            Ok(true)
        }
        "uninstall" => {
            let (system, _) = split_system_and_args(&raw_args[1..]);
            println!("{}", uninstall(system)?);
            Ok(true)
        }
        "service-status" => {
            let (system, _) = split_system_and_args(&raw_args[1..]);
            let st = status(system)?;
            println!(
                "scope={} unit={} installed={} running={} node_health={} detail={}",
                st.scope, st.unit_name, st.installed, st.running, st.node_health, st.detail
            );
            Ok(true)
        }
        "service-restart" => {
            let (system, _) = split_system_and_args(&raw_args[1..]);
            println!("{}", restart(system)?);
            Ok(true)
        }
        "run-supervised" => {
            let rest = strip_leading_dd(&raw_args[1..]);
            if rest.is_empty() {
                return Err("run-supervised requires `--` and node arguments".into());
            }
            run_supervised(&rest);
        }
        _ => Ok(false),
    }
}

fn split_system_and_args(args: &[String]) -> (bool, Vec<String>) {
    let mut system = false;
    let mut rest = Vec::new();
    for a in args {
        if a == "--system" {
            system = true;
        } else {
            rest.push(a.clone());
        }
    }
    (system, rest)
}

fn strip_leading_dd(args: &[String]) -> Vec<String> {
    match args.first().map(String::as_str) {
        Some("--") => args[1..].to_vec(),
        _ => args.to_vec(),
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    #[test]
    fn unit_name_stable_for_enroll_path() {
        let spec = ServiceSpec {
            system: false,
            exe: PathBuf::from("/bin/gate4agent-node"),
            args: vec![
                "--enrolled".into(),
                "--enroll-state".into(),
                "/tmp/enroll-a".into(),
            ],
            env: vec![],
            enroll_state: Some(PathBuf::from("/tmp/enroll-a")),
        };
        let n = spec.unit_name();
        assert!(n.starts_with("gate4agent-node-"));
        assert_eq!(n, spec.unit_name());
        assert!(!n.contains('/'));
    }

    #[test]
    fn extract_enroll_state_from_args() {
        let args = vec![
            "--enrolled".into(),
            "--enroll-state".into(),
            "/var/lib/x".into(),
        ];
        assert_eq!(
            extract_enroll_state(&args),
            Some(PathBuf::from("/var/lib/x"))
        );
    }
}
