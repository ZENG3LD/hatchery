//! Core's m4a gateway: core talks to sessions only through m4a (decisions 4 and 8).
//!
//! - **wake**: `POST /v1/sessions/{id}/wake` sends an m4a letter to the session's nick;
//!   the node's letter hook wakes the session with a short line, and the session reads
//!   the letter (and any task text in it) from its own m4a inbox.
//! - **reader**: core polls the DM rooms of sessions that hold an active run and acts on
//!   letters of the form `run.close <run_id> <review|done|failed|cancelled> [branch] [commit]`
//!   from the run's own session nick (door `task.close_run` stays as a fallback).
//!
//! Core uses its own m4a identity through the m4a CLI and a running m4a client; it never
//! holds keys itself. Off unless `HATCHERY_CORE_M4A_CLI` and `HATCHERY_CORE_M4A_AS` are set.

use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

pub const CLI_ENV: &str = "HATCHERY_CORE_M4A_CLI";
pub const AS_ENV: &str = "HATCHERY_CORE_M4A_AS";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Letter {
    pub event_id: String,
    pub from: String,
    pub body: String,
}

pub trait M4a: Send + Sync {
    fn send(&self, to: &str, text: &str) -> Result<(), String>;
    /// Newest `n` letters of the DM room with `nick`, oldest first.
    fn read_dm(&self, nick: &str, n: u32) -> Result<Vec<Letter>, String>;
    fn me(&self) -> &str;
}

pub struct CliM4a {
    bin: PathBuf,
    as_nick: String,
}

impl CliM4a {
    pub fn from_env() -> Option<Self> {
        let bin = std::env::var(CLI_ENV).ok().filter(|v| !v.is_empty())?;
        let as_nick = std::env::var(AS_ENV).ok().filter(|v| !v.is_empty())?;
        Some(Self { bin: bin.into(), as_nick })
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
        use std::io::Write;
        let mut child = Command::new(&self.bin)
            .args(["--as", &self.as_nick])
            .args(args)
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("m4a: {e}"))?;
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(text.as_bytes()).map_err(|e| format!("m4a: {e}"))?;
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                let out = child.wait_with_output().map_err(|e| e.to_string())?;
                return if status.success() {
                    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
                } else {
                    // stderr may name rooms; it never carries keys.
                    Err(format!("m4a exited {}: {}", status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).chars().take(200).collect::<String>()))
                };
            }
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                return Err("m4a timed out".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

pub fn parse_rows(out: &str) -> Vec<Letter> {
    let v: Value = serde_json::from_str(out.trim()).unwrap_or(Value::Null);
    let rows = v.as_array().cloned().or_else(|| v["messages"].as_array().cloned()).or_else(|| v["rows"].as_array().cloned()).unwrap_or_default();
    let mut letters: Vec<(i64, Letter)> = rows
        .iter()
        .filter(|r| r["kind"].as_str() != Some("undecryptable"))
        .filter_map(|r| {
            Some((
                r["ts"].as_i64().unwrap_or(0),
                Letter { event_id: r["event_id"].as_str()?.to_owned(), from: r["from"].as_str()?.to_owned(), body: r["body"].as_str().unwrap_or("").to_owned() },
            ))
        })
        .collect();
    letters.sort_by_key(|(ts, _)| *ts);
    letters.into_iter().map(|(_, l)| l).collect()
}

impl M4a for CliM4a {
    fn send(&self, to: &str, text: &str) -> Result<(), String> {
        self.run(&["send", to, "-"], Some(text)).map(|_| ())
    }
    fn read_dm(&self, nick: &str, n: u32) -> Result<Vec<Letter>, String> {
        self.run(&["--json", "read", nick, "-n", &n.to_string()], None).map(|o| parse_rows(&o))
    }
    fn me(&self) -> &str {
        &self.as_nick
    }
}

/// `run.close <run_id> <status> [branch] [commit]`, first line only.
#[derive(Debug, PartialEq, Eq)]
pub struct CloseCmd {
    pub run_id: i64,
    pub status: String,
    pub branch: Option<String>,
    pub commit: Option<String>,
}

pub fn parse_close(body: &str) -> Option<CloseCmd> {
    let line = body.lines().next()?.trim();
    let mut w = line.split_whitespace();
    if w.next()? != "run.close" {
        return None;
    }
    let run_id = w.next()?.trim_start_matches('#').parse().ok()?;
    let status = w.next()?.to_owned();
    let clean = |s: &str| Some(s.to_owned()).filter(|s| s.len() <= 200 && !s.chars().any(char::is_control));
    Some(CloseCmd { run_id, status, branch: w.next().and_then(clean), commit: w.next().and_then(clean) })
}

/// Strip a matrix-style `@nick:server` to `nick` for comparisons.
pub fn bare_nick(n: &str) -> &str {
    let n = n.trim_start_matches('@');
    n.split(':').next().unwrap_or(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_close_letters() {
        assert_eq!(parse_close("run.close 7 done feat/x abc123\nthanks"), Some(CloseCmd { run_id: 7, status: "done".into(), branch: Some("feat/x".into()), commit: Some("abc123".into()) }));
        assert_eq!(parse_close("run.close #3 review").unwrap().run_id, 3);
        assert!(parse_close("hello run.close 1 done").is_none());
        assert!(parse_close("run.close x done").is_none());
    }

    #[test]
    fn parses_rows_oldest_first_and_skips_undecryptable() {
        let out = r#"[{"event_id":"$b","from":"@s1:h","body":"two","ts":2},{"event_id":"$a","from":"@s1:h","body":"one","ts":1},{"event_id":"$c","from":"x","kind":"undecryptable","ts":3}]"#;
        let l = parse_rows(out);
        assert_eq!(l.iter().map(|x| x.body.as_str()).collect::<Vec<_>>(), ["one", "two"]);
        assert_eq!(bare_nick("@s1:h"), "s1");
        assert!(parse_rows("garbage").is_empty());
    }
}
