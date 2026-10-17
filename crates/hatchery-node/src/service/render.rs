//! Pure generators for OS supervision descriptors. No I/O. Paths come from the
//! caller (current_exe, home dirs, args) — never hardcoded product paths.

/// Escape a value for a systemd unit Environment= line (double-quoted).
pub fn systemd_escape_env(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' | '"' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Quote one argv element for ExecStart=.
pub fn systemd_quote_arg(arg: &str) -> String {
    if arg.is_empty()
        || arg.contains(|c: char| c.is_whitespace() || "\"'\\$".contains(c))
    {
        systemd_escape_env(arg)
    } else {
        arg.to_owned()
    }
}

pub struct SystemdUnit<'a> {
    pub description: &'a str,
    pub exe: &'a str,
    pub args: &'a [String],
    pub env: &'a [(String, String)],
    /// When true, Supervised wrapper is the ExecStart (first arg run-supervised).
    pub supervised: bool,
}

pub fn render_systemd_unit(u: &SystemdUnit<'_>) -> String {
    let mut exec = String::new();
    exec.push_str(&systemd_quote_arg(u.exe));
    if u.supervised {
        exec.push(' ');
        exec.push_str("run-supervised");
        exec.push(' ');
        exec.push_str("--");
    }
    for a in u.args {
        exec.push(' ');
        exec.push_str(&systemd_quote_arg(a));
    }
    let mut body = String::new();
    body.push_str("[Unit]\n");
    body.push_str(&format!("Description={}\n", u.description));
    body.push_str("After=network-online.target\n");
    body.push_str("Wants=network-online.target\n\n");
    body.push_str("[Service]\n");
    body.push_str("Type=simple\n");
    body.push_str("Restart=always\n");
    body.push_str("RestartSec=5\n");
    for (k, v) in u.env {
        body.push_str("Environment=");
        body.push_str(k);
        body.push('=');
        body.push_str(&systemd_escape_env(v));
        body.push('\n');
    }
    body.push_str("ExecStart=");
    body.push_str(&exec);
    body.push('\n');
    body.push_str("KillMode=control-group\n");
    body.push_str("TimeoutStopSec=30\n\n");
    body.push_str("[Install]\n");
    body.push_str("WantedBy=default.target\n");
    body
}

pub struct LaunchPlist<'a> {
    pub label: &'a str,
    pub exe: &'a str,
    pub args: &'a [String],
    pub env: &'a [(String, String)],
    pub supervised: bool,
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn render_launchd_plist(p: &LaunchPlist<'_>) -> String {
    let mut prog = Vec::new();
    prog.push(p.exe.to_owned());
    if p.supervised {
        prog.push("run-supervised".into());
        prog.push("--".into());
    }
    prog.extend(p.args.iter().cloned());
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(
        "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
    );
    out.push_str("<plist version=\"1.0\">\n<dict>\n");
    out.push_str(&format!(
        "  <key>Label</key>\n  <string>{}</string>\n",
        xml_escape(p.label)
    ));
    out.push_str("  <key>ProgramArguments</key>\n  <array>\n");
    for a in &prog {
        out.push_str(&format!("    <string>{}</string>\n", xml_escape(a)));
    }
    out.push_str("  </array>\n");
    if !p.env.is_empty() {
        out.push_str("  <key>EnvironmentVariables</key>\n  <dict>\n");
        for (k, v) in p.env {
            out.push_str(&format!(
                "    <key>{}</key>\n    <string>{}</string>\n",
                xml_escape(k),
                xml_escape(v)
            ));
        }
        out.push_str("  </dict>\n");
    }
    out.push_str("  <key>RunAtLoad</key>\n  <true/>\n");
    out.push_str("  <key>KeepAlive</key>\n  <true/>\n");
    out.push_str("  <key>ThrottleInterval</key>\n  <integer>5</integer>\n");
    out.push_str("</dict>\n</plist>\n");
    out
}

pub struct SchtasksXml<'a> {
    pub task_name: &'a str,
    pub exe: &'a str,
    pub args: &'a [String],
    pub working_dir: &'a str,
    pub author: &'a str,
    /// When false, `exe` is gate4agent-nodew which inserts run-supervised itself.
    pub supervised: bool,
}

pub fn render_schtasks_xml(t: &SchtasksXml<'_>) -> String {
    // run-supervised covers exit codes Task Scheduler misses; nodew inserts it.
    let mut arguments = String::new();
    if t.supervised {
        arguments.push_str("run-supervised --");
    }
    for a in t.args {
        if !arguments.is_empty() {
            arguments.push(' ');
        }
        if a.is_empty() || a.contains(char::is_whitespace) || a.contains('"') {
            arguments.push('"');
            arguments.push_str(&a.replace('"', "\\\""));
            arguments.push('"');
        } else {
            arguments.push_str(a);
        }
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>{author}</Author>
    <Description>gate4agent-node ({name})</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>true</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <DisallowStartOnRemoteAppSession>false</DisallowStartOnRemoteAppSession>
    <UseUnifiedSchedulingEngine>true</UseUnifiedSchedulingEngine>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>999</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>{args}</Arguments>
      <WorkingDirectory>{cwd}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#,
        author = xml_escape(t.author),
        name = xml_escape(t.task_name),
        exe = xml_escape(t.exe),
        args = xml_escape(&arguments),
        cwd = xml_escape(t.working_dir),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn systemd_unit_roundtrip_in_tempdir() {
        let dir = TempDir::new().unwrap();
        let unit = render_systemd_unit(&SystemdUnit {
            description: "node test",
            exe: "/opt/bin/gate4agent-node",
            args: &[
                "--enrolled".into(),
                "--enroll-state".into(),
                "/var/lib/node/enroll".into(),
            ],
            env: &[("RUST_LOG".into(), "info".into())],
            supervised: false,
        });
        assert!(unit.contains("Restart=always"));
        assert!(unit.contains("RestartSec=5"));
        assert!(unit.contains("ExecStart=/opt/bin/gate4agent-node --enrolled"));
        assert!(unit.contains("Environment=RUST_LOG=\"info\""));
        assert!(!unit.to_ascii_lowercase().contains("hatchery-box"));
        let path = dir.path().join("node.service");
        fs::write(&path, &unit).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), unit);
    }

    #[test]
    fn systemd_supervised_inserts_wrapper() {
        let unit = render_systemd_unit(&SystemdUnit {
            description: "n",
            exe: "/x/gate4agent-node",
            args: &["--enrolled".into()],
            env: &[],
            supervised: true,
        });
        assert!(unit.contains("run-supervised -- --enrolled"));
    }

    #[test]
    fn launchd_plist_keepalive() {
        let dir = TempDir::new().unwrap();
        let plist = render_launchd_plist(&LaunchPlist {
            label: "com.example.node",
            exe: "/usr/local/bin/gate4agent-node",
            args: &["--enrolled".into()],
            env: &[("GATE4AGENT_NODE_TOKEN".into(), "secret".into())],
            supervised: true,
        });
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("run-supervised"));
        assert!(plist.contains("<string>secret</string>"));
        let path = dir.path().join("agent.plist");
        fs::write(&path, &plist).unwrap();
        assert!(fs::read_to_string(path).unwrap().contains("com.example.node"));
    }

    #[test]
    fn schtasks_xml_hidden_and_supervised() {
        let dir = TempDir::new().unwrap();
        let xml = render_schtasks_xml(&SchtasksXml {
            task_name: "gate4agent-node",
            exe: r"C:\Users\x\.cargo\bin\gate4agent-node.exe",
            args: &["--enrolled".into(), "--enroll-state".into(), r"C:\Users\x\state".into()],
            working_dir: r"C:\Users\x",
            author: "gate4agent-node",
            supervised: true,
        });
        assert!(xml.contains("<Hidden>true</Hidden>"));
        assert!(xml.contains("ExecutionTimeLimit>PT0S"));
        assert!(xml.contains("RestartOnFailure"));
        assert!(xml.contains("run-supervised --"));
        assert!(xml.contains("LogonTrigger"));
        let path = dir.path().join("task.xml");
        fs::write(&path, &xml).unwrap();
        assert!(fs::metadata(path).unwrap().len() > 100);
    }

    #[test]
    fn env_escaping_quotes_and_backslash() {
        assert_eq!(systemd_escape_env(r#"a"b\c"#), r#""a\"b\\c""#);
    }
}
