//! Collect host machine resources + installed provider CLI versions for topology inventory.

use hatchery_core::{MachineEnvironment, ProviderCliReport};
use std::fs;
use std::process::Command;

const PROVIDERS: &[&str] = &["claude", "codex", "grok", "kimi", "kimi-cli", "bash"];

/// Snapshot the host environment. Best-effort: missing fields become 0 / empty.
pub fn collect() -> MachineEnvironment {
    let (os, kernel, arch) = uname();
    let hostname = fs::read_to_string("/etc/hostname")
        .or_else(|_| fs::read_to_string("/proc/sys/kernel/hostname"))
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| "unknown".into());
    let uptime_secs = fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .map(|s| s as u64)
        .unwrap_or(0);
    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);
    let cpu_model = cpu_model();
    let (ram_total_bytes, ram_available_bytes) = meminfo();
    let (disk_total_bytes, disk_available_bytes) = disk_root();
    let provider_clis = PROVIDERS
        .iter()
        .filter_map(|id| probe_cli(id))
        .collect();
    MachineEnvironment {
        os,
        kernel,
        arch,
        hostname,
        uptime_secs,
        cpu_cores,
        cpu_model,
        ram_total_bytes,
        ram_available_bytes,
        disk_total_bytes,
        disk_available_bytes,
        provider_clis,
    }
}

fn uname() -> (String, String, String) {
    let out = Command::new("uname").args(["-srm"]).output().ok();
    let text = out
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let mut parts = text.split_whitespace();
    let os = parts.next().unwrap_or(std::env::consts::OS).to_owned();
    let kernel = parts.next().unwrap_or("unknown").to_owned();
    let arch = parts
        .next()
        .unwrap_or(std::env::consts::ARCH)
        .to_owned();
    (os, kernel, arch)
}

fn cpu_model() -> Option<String> {
    let text = fs::read_to_string("/proc/cpuinfo").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("model name") {
            return Some(rest.trim_start_matches(|c: char| c == ':' || c.is_whitespace()).to_owned());
        }
    }
    None
}

fn meminfo() -> (u64, u64) {
    let text = match fs::read_to_string("/proc/meminfo") {
        Ok(t) => t,
        Err(_) => return (0, 0),
    };
    let mut total_kb = 0u64;
    let mut avail_kb = 0u64;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            total_kb = parse_kb(v);
        } else if let Some(v) = line.strip_prefix("MemAvailable:") {
            avail_kb = parse_kb(v);
        }
    }
    (total_kb.saturating_mul(1024), avail_kb.saturating_mul(1024))
}

fn parse_kb(v: &str) -> u64 {
    v.split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn disk_root() -> (u64, u64) {
    // Prefer `statvfs` via df -B1 /
    let out = Command::new("df").args(["-B1", "/"]).output().ok();
    let text = out.and_then(|o| String::from_utf8(o.stdout).ok()).unwrap_or_default();
    for line in text.lines().skip(1) {
        let cols: Vec<_> = line.split_whitespace().collect();
        if cols.len() >= 4 {
            let total = cols[1].parse().unwrap_or(0);
            let avail = cols[3].parse().unwrap_or(0);
            return (total, avail);
        }
    }
    (0, 0)
}

fn probe_cli(id: &str) -> Option<ProviderCliReport> {
    let path = Command::new("sh")
        .args(["-c", &format!("command -v {id}")])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_owned())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty())?;
    let version = Command::new(&path)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            let mut text = String::from_utf8_lossy(&o.stdout).to_string();
            if text.trim().is_empty() {
                text = String::from_utf8_lossy(&o.stderr).to_string();
            }
            let line = text.lines().next()?.trim().to_owned();
            if line.is_empty() { None } else { Some(line) }
        });
    Some(ProviderCliReport {
        id: id.to_owned(),
        version,
        path: Some(path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_returns_nonzero_cores_on_linux() {
        let env = collect();
        assert!(env.cpu_cores >= 1);
        assert!(!env.os.is_empty());
        assert!(!env.arch.is_empty());
    }
}
