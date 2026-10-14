//! Core's m4a letters: core talks to sessions only through its own in-process m4a mailbox
//! engine (the hub messenger). `POST /v1/sessions/{id}/wake` sends a letter from the operator
//! mailbox to the session's node naming the session; sessions close their run by sending
//! `run.close <run_id> <review|done|failed|cancelled> [branch] [commit]` to the operator
//! mailbox (door `task.close_run` stays as a fallback).

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
    fn bare_nicks() {
        assert_eq!(bare_nick("@s1:h"), "s1");
    }
}
