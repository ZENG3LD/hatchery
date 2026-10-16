//! Provider inventory: which provider CLIs this node can run and which models each one
//! exposes, read from the CLI itself (or its own config). Nothing is invented: a provider whose
//! CLI does not list models reports only what its help text / config names.
//!
//! - codex: `codex debug models` (JSON catalog; entries with `visibility = "list"`).
//! - grok: `grok models` (text list; `*` marks the default).
//! - claude: the model aliases its `--help` names for `--model`, plus `model` from
//!   `~/.claude/settings.json`.
//! - kimi: `[models.<alias>]` and `default_model` from `~/.kimi/config.toml`.

use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub efforts: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ProviderInfo {
    pub provider: String,
    /// Where the list came from (`cli`, `help`, `config`).
    pub source: String,
    pub models: Vec<ModelInfo>,
}

pub const PROVIDERS: &[&str] = &["claude", "codex", "grok", "kimi"];

fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(bin)).find(|p| p.is_file())
}

/// Run `bin args` with a deadline; stdout as text (stderr dropped, no stdin).
fn run(bin: &Path, args: &[&str], deadline: Duration) -> Option<String> {
    let mut child = Command::new(bin).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.by_ref().take(4 << 20).read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    String::from_utf8(reader.join().ok()?).ok()
}

/// Model ids may carry a vendor path or tag (`kimi-code/k3`, `name:tag`, `x@v`); entries that
/// fail are dropped one by one, never the whole provider.
fn ok_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && !s.starts_with(['/', ':']) && !s.contains("..") && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.[]/:@".contains(&b))
}

pub fn parse_codex(json: &str) -> Vec<ModelInfo> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let list = v.get("models").and_then(|m| m.as_array()).cloned().unwrap_or_default();
    list.iter()
        .filter(|m| m.get("visibility").and_then(|x| x.as_str()).is_none_or(|x| x == "list"))
        .filter_map(|m| {
            let id = m.get("slug").or_else(|| m.get("id")).and_then(|x| x.as_str())?;
            ok_id(id).then(|| ModelInfo {
                id: id.to_owned(),
                efforts: m
                    .get("supported_reasoning_levels")
                    .and_then(|l| l.as_array())
                    .map(|l| l.iter().filter_map(|e| e.get("effort").and_then(|x| x.as_str())).filter(|e| ok_id(e)).map(str::to_owned).collect())
                    .unwrap_or_default(),
                default: false,
            })
        })
        .collect()
}

pub fn parse_grok(text: &str) -> Vec<ModelInfo> {
    text.lines()
        .filter_map(|l| {
            let t = l.trim_start();
            let (default, rest) = if let Some(r) = t.strip_prefix("* ") { (true, r) } else { (false, t.strip_prefix("- ")?) };
            let id = rest.split_whitespace().next()?;
            ok_id(id).then(|| ModelInfo { id: id.to_owned(), efforts: Vec::new(), default })
        })
        .collect()
}

/// Quoted aliases in the `--model` paragraph of `claude --help`.
pub fn parse_claude_help(help: &str) -> Vec<ModelInfo> {
    let Some(start) = help.find("--model") else { return Vec::new() };
    let para: String = help[start..].lines().take_while(|l| !l.trim().is_empty()).take(8).enumerate()
        .filter(|(i, l)| *i == 0 || !l.trim_start().starts_with("--"))
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join(" ");
    let mut out: Vec<ModelInfo> = Vec::new();
    for (i, part) in para.split('\'').enumerate() {
        if i % 2 == 1 && ok_id(part) && !out.iter().any(|m| m.id == part) {
            out.push(ModelInfo { id: part.to_owned(), efforts: Vec::new(), default: false });
        }
    }
    out
}

pub fn parse_kimi_config(toml_text: &str) -> Vec<ModelInfo> {
    let mut out = Vec::new();
    let mut default = None;
    for line in toml_text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("[models.").and_then(|r| r.strip_suffix(']')) {
            let id = rest.trim_matches('"');
            if ok_id(id) {
                out.push(ModelInfo { id: id.to_owned(), efforts: Vec::new(), default: false });
            }
        } else if let Some(v) = l.strip_prefix("default_model") {
            default = v.trim_start().strip_prefix('=').map(|v| v.trim().trim_matches('"').to_owned());
        }
    }
    for m in &mut out {
        m.default = default.as_deref() == Some(m.id.as_str());
    }
    out
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Detect one provider; `None` when its CLI is not on PATH.
pub fn detect(provider: &str) -> Option<ProviderInfo> {
    let deadline = Duration::from_secs(20);
    let (bin, source, models) = match provider {
        "codex" => {
            let bin = which("codex")?;
            let m = run(&bin, &["debug", "models"], deadline).map(|s| parse_codex(&s)).unwrap_or_default();
            (bin, "cli", m)
        }
        "grok" => {
            let bin = which("grok")?;
            let m = run(&bin, &["models"], deadline).map(|s| parse_grok(&s)).unwrap_or_default();
            (bin, "cli", m)
        }
        "claude" => {
            let bin = which("claude")?;
            let mut m = run(&bin, &["--help"], deadline).map(|s| parse_claude_help(&s)).unwrap_or_default();
            if let Some(set) = home().and_then(|h| std::fs::read(h.join(".claude/settings.json")).ok()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) {
                if let Some(id) = set.get("model").and_then(|x| x.as_str()).filter(|s| ok_id(s)) {
                    match m.iter_mut().find(|x| x.id == id) {
                        Some(x) => x.default = true,
                        None => m.push(ModelInfo { id: id.to_owned(), efforts: Vec::new(), default: true }),
                    }
                }
            }
            (bin, "help", m)
        }
        "kimi" => {
            let bin = which("kimi").filter(|b| usable(b))?;
            let m = home().and_then(|h| std::fs::read_to_string(h.join(".kimi/config.toml")).ok()).map(|s| parse_kimi_config(&s)).unwrap_or_default();
            (bin, "config", m)
        }
        _ => return None,
    };
    let _ = bin;
    Some(ProviderInfo { provider: provider.to_owned(), source: source.to_owned(), models })
}

/// All providers whose CLI this node can run.
pub fn detect_all() -> Vec<ProviderInfo> {
    PROVIDERS.iter().filter_map(|p| detect(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_catalog() {
        let j = r#"{"models":[{"slug":"m-a","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}]},{"slug":"m-hidden","visibility":"hide"},{"slug":"bad slug","visibility":"list"}]}"#;
        let m = parse_codex(j);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "m-a");
        assert_eq!(m[0].efforts, ["low", "high"]);
        assert!(parse_codex("nope").is_empty());
    }

    #[test]
    fn parses_grok_list() {
        let m = parse_grok("You are not authenticated.\n\nDefault model: g-1\n\nAvailable models:\n  * g-1 (default)\n  - g-0\n");
        assert_eq!(m.iter().map(|x| (x.id.as_str(), x.default)).collect::<Vec<_>>(), [("g-1", true), ("g-0", false)]);
    }

    #[test]
    fn parses_claude_help_aliases() {
        let h = "  --mcp-config <c>    x\n  --model <model>     Model for the current session. Provide\n                      an alias for the latest model (e.g.\n                      'a1', 'b2', or 'c3') or a\n                      model's full name.\n  --name <n>          y 'zz'\n";
        let m = parse_claude_help(h);
        assert_eq!(m.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["a1", "b2", "c3"]);
    }

    #[test]
    fn parses_kimi_config() {
        let m = parse_kimi_config("default_model = \"k2\"\n[models.k1]\nprovider = \"x\"\n[models.\"k2\"]\n");
        assert_eq!(m.iter().map(|x| (x.id.as_str(), x.default)).collect::<Vec<_>>(), [("k1", false), ("k2", true)]);
    }
}

/// Binary names a provider may be installed under, in preference order. The canonical id
/// (what g4a launches) comes first. Override per provider with `HATCHERY_PROVIDER_BIN_<ID>`
/// (absolute path or a name on PATH).
pub fn aliases(provider: &str) -> &'static [&'static str] {
    match provider {
        "kimi" => &["kimi", "kimi-cli", "kimi-code"],
        "claude" => &["claude", "claude-code"],
        "codex" => &["codex"],
        "grok" => &["grok"],
        _ => &[],
    }
}

/// A candidate is usable only if `--version` answers and it is not a retired stub
/// (kimi-cli 1.x prints "no longer maintained" and does nothing).
fn usable(bin: &Path) -> bool {
    let Some(out) = run(bin, &["--version"], Duration::from_secs(5)) else { return false };
    let help = run(bin, &["--help"], Duration::from_secs(5)).unwrap_or_default();
    !out.trim().is_empty() && !help.contains("no longer maintained") && !out.contains("no longer maintained")
}

/// Resolve the binary for `provider`: override env, then each alias on PATH, then the
/// vendor's own install dirs under `extra_homes`.
pub fn resolve_binary(provider: &str, extra_homes: &[PathBuf]) -> Option<PathBuf> {
    let key = format!("HATCHERY_PROVIDER_BIN_{}", provider.to_ascii_uppercase());
    if let Some(v) = std::env::var_os(&key).filter(|v| !v.is_empty()) {
        let p = PathBuf::from(&v);
        let p = if p.is_absolute() { Some(p) } else { which(&p.to_string_lossy()) };
        return p.filter(|p| usable(p));
    }
    for name in aliases(provider) {
        if let Some(p) = which(name).filter(|p| usable(p)) {
            return Some(p);
        }
    }
    let vendor_dirs: &[&str] = match provider {
        "kimi" => &[".kimi-code/bin/kimi"],
        "claude" => &[".claude/local/claude", ".local/bin/claude"],
        _ => &[],
    };
    extra_homes
        .iter()
        .flat_map(|h| vendor_dirs.iter().map(move |d| h.join(d)))
        .find(|p| p.is_file() && usable(p))
}

/// Make every resolvable provider launchable under its canonical name: a node-private shim
/// dir with `<id> -> <resolved binary>` symlinks, prepended to PATH (children inherit it).
#[cfg(unix)]
pub fn install_shims(shim_dir: &Path, extra_homes: &[PathBuf]) -> Vec<(String, PathBuf)> {
    let mut done = Vec::new();
    if std::fs::create_dir_all(shim_dir).is_err() {
        return done;
    }
    for p in PROVIDERS {
        let link = shim_dir.join(p);
        let _ = std::fs::remove_file(&link);
        // Already launchable under its own name and usable: nothing to do.
        if which(p).filter(|b| usable(b)).is_some() {
            continue;
        }
        if let Some(bin) = resolve_binary(p, extra_homes) {
            if std::os::unix::fs::symlink(&bin, &link).is_ok() {
                done.push((p.to_string(), bin));
            }
        }
    }
    let old = std::env::var_os("PATH").unwrap_or_default();
    let mut parts = vec![shim_dir.to_path_buf()];
    parts.extend(std::env::split_paths(&old).filter(|d| d != shim_dir));
    if let Ok(joined) = std::env::join_paths(parts) {
        std::env::set_var("PATH", joined);
    }
    done
}

#[cfg(all(test, unix))]
mod alias_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn retired_stub_is_skipped_and_vendor_install_is_found() {
        let d = std::env::temp_dir().join(format!("alias-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("home/.kimi-code/bin")).unwrap();
        let stub = script(&d, "kimi-cli", "echo 'kimi-cli is no longer maintained.'");
        assert!(!usable(&stub));
        let real = script(&d.join("home/.kimi-code/bin"), "kimi", "echo 2.1.1");
        assert!(usable(&real));
        // PATH may already carry a real kimi on dev boxes; either way the stub is never chosen.
        let got = resolve_binary("kimi", &[d.join("home")]).expect("kimi resolved");
        assert_ne!(got, stub);
    }
}
