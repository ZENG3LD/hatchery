//! Signed session plugins (node-owned session environment).
//!
//! A plugin is a directory with a manifest `plugin.json` and a detached signature
//! `plugin.sig`. The manifest names the plugin, its version, the providers it applies to
//! and every file with its SHA-256. The signature is Ed25519 over the exact manifest
//! bytes, by a publisher key the node trusts (`trusted-keys` file: one hex public key
//! per line, `#` comments allowed). The node verifies the signature, every file hash,
//! and that the package holds no extra files, links or path escapes before it copies
//! the plugin into its plugin root. Core never sees plugin contents.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

pub const MANIFEST: &str = "plugin.json";
pub const SIGNATURE: &str = "plugin.sig";
const MAX_FILES: usize = 4096;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    /// Provider ids this plugin applies to (empty = all).
    #[serde(default)]
    pub providers: Vec<String>,
    /// Relative path -> lowercase hex SHA-256.
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub signer: String,
}

fn ident_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)) && !s.starts_with('.')
}

fn rel_ok(p: &str) -> bool {
    let path = Path::new(p);
    !p.is_empty() && p != MANIFEST && p != SIGNATURE && path.components().all(|c| matches!(c, Component::Normal(_)))
}

pub fn load_trusted_keys(file: &Path) -> Result<Vec<VerifyingKey>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("trusted keys: {e}"))?;
    let mut keys = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let hex_key = line.split_whitespace().next().unwrap_or("");
        let bytes: [u8; 32] = hex::decode(hex_key).ok().and_then(|b| b.try_into().ok()).ok_or("trusted keys: bad key")?;
        keys.push(VerifyingKey::from_bytes(&bytes).map_err(|_| "trusted keys: bad key".to_owned())?);
    }
    Ok(keys)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ft = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if ft.is_symlink() {
            return Err(format!("link not allowed: {}", path.strip_prefix(root).unwrap_or(&path).display()));
        } else if ft.is_dir() {
            walk(root, &path, out)?;
        } else if ft.is_file() {
            let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
            out.push(rel.to_str().ok_or("non-UTF-8 path")?.replace('\\', "/"));
        } else {
            return Err("special file not allowed".into());
        }
        if out.len() > MAX_FILES + 2 {
            return Err("too many files".into());
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(hex::encode(Sha256::digest(&bytes)))
}

/// Verify a plugin package. Returns the manifest and the hex key that signed it.
pub fn verify(pkg: &Path, trusted: &[VerifyingKey]) -> Result<(PluginManifest, String), String> {
    let manifest_bytes = std::fs::read(pkg.join(MANIFEST)).map_err(|e| format!("manifest: {e}"))?;
    let sig_text = std::fs::read_to_string(pkg.join(SIGNATURE)).map_err(|e| format!("signature: {e}"))?;
    let sig_bytes: [u8; 64] = hex::decode(sig_text.trim()).ok().and_then(|b| b.try_into().ok()).ok_or("signature: malformed")?;
    let sig = Signature::from_bytes(&sig_bytes);
    let signer = trusted
        .iter()
        .find(|k| k.verify(&manifest_bytes, &sig).is_ok())
        .ok_or("signature: not signed by a trusted key")?;
    let manifest: PluginManifest = serde_json::from_slice(&manifest_bytes).map_err(|e| format!("manifest: {e}"))?;
    if !ident_ok(&manifest.name) || !ident_ok(&manifest.version) {
        return Err("manifest: bad name or version".into());
    }
    if manifest.files.len() > MAX_FILES || manifest.files.keys().any(|p| !rel_ok(p)) {
        return Err("manifest: bad file list".into());
    }
    let mut present = Vec::new();
    walk(pkg, pkg, &mut present)?;
    present.retain(|p| p != MANIFEST && p != SIGNATURE);
    present.sort();
    let declared: Vec<&String> = manifest.files.keys().collect();
    if present.iter().collect::<Vec<_>>() != declared {
        return Err("package files do not match the manifest".into());
    }
    let mut total = 0u64;
    for (rel, want) in &manifest.files {
        let path = pkg.join(rel);
        total += std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
        if total > MAX_TOTAL_BYTES {
            return Err("package too large".into());
        }
        if sha256_file(&path)? != want.to_ascii_lowercase() {
            return Err(format!("hash mismatch: {rel}"));
        }
    }
    Ok((manifest, hex::encode(signer.as_bytes())))
}

/// Verify, then copy into `<root>/<name>/<version>` (atomic rename). An existing
/// identical version is replaced only after the new copy verified.
pub fn install(pkg: &Path, root: &Path, trusted: &[VerifyingKey]) -> Result<Installed, String> {
    let (manifest, signer) = verify(pkg, trusted)?;
    let dest = root.join(&manifest.name).join(&manifest.version);
    let parent = dest.parent().expect("has parent");
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = parent.join(format!(".{}.tmp-{}", manifest.version, std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    for rel in manifest.files.keys().map(String::as_str).chain([MANIFEST, SIGNATURE]) {
        let to = tmp.join(rel);
        std::fs::create_dir_all(to.parent().expect("parent")).map_err(|e| e.to_string())?;
        std::fs::copy(pkg.join(rel), &to).map_err(|e| e.to_string())?;
    }
    // Re-verify the copy (guards against the source changing during the copy).
    if let Err(e) = verify(&tmp, trusted) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("copy failed verification: {e}"));
    }
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(Installed { name: manifest.name, version: manifest.version, path: dest, signer })
}

/// List installed plugins that still verify (tampered ones are skipped).
pub fn list(root: &Path, trusted: &[VerifyingKey]) -> Vec<Result<Installed, (PathBuf, String)>> {
    let mut out = Vec::new();
    let Ok(names) = std::fs::read_dir(root) else { return out };
    for name in names.flatten() {
        let Ok(versions) = std::fs::read_dir(name.path()) else { continue };
        for v in versions.flatten() {
            let path = v.path();
            if v.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            out.push(match verify(&path, trusted) {
                Ok((m, signer)) => Ok(Installed { name: m.name, version: m.version, path, signer }),
                Err(e) => Err((path, e)),
            });
        }
    }
    out
}

/// Publisher side: write `plugin.json` for the files in `pkg` and sign it.
/// `secret_hex` is the 32-byte Ed25519 seed (hex); it is never printed.
pub fn sign(pkg: &Path, name: &str, version: &str, providers: Vec<String>, secret_hex: &str) -> Result<String, String> {
    let seed: [u8; 32] = hex::decode(secret_hex.trim()).ok().and_then(|b| b.try_into().ok()).ok_or("bad secret key")?;
    let key = SigningKey::from_bytes(&seed);
    let mut rels = Vec::new();
    walk(pkg, pkg, &mut rels)?;
    rels.retain(|p| p != MANIFEST && p != SIGNATURE);
    let mut files = BTreeMap::new();
    for rel in rels {
        files.insert(rel.clone(), sha256_file(&pkg.join(&rel))?);
    }
    let manifest = PluginManifest { name: name.into(), version: version.into(), providers, files };
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(pkg.join(MANIFEST), &bytes).map_err(|e| e.to_string())?;
    std::fs::write(pkg.join(SIGNATURE), hex::encode(key.sign(&bytes).to_bytes())).map_err(|e| e.to_string())?;
    Ok(hex::encode(key.verifying_key().as_bytes()))
}

/// New publisher key: returns (secret seed hex, public key hex).
pub fn keygen() -> (String, String) {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("os randomness");
    let key = SigningKey::from_bytes(&seed);
    (hex::encode(seed), hex::encode(key.verifying_key().as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf, Vec<VerifyingKey>, String) {
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("pkg");
        std::fs::create_dir_all(pkg.join("skills/review")).unwrap();
        std::fs::write(pkg.join("skills/review/SKILL.md"), b"review").unwrap();
        std::fs::write(pkg.join("README"), b"hi").unwrap();
        let (secret, public) = keygen();
        sign(&pkg, "review-kit", "1.0.0", vec!["claude".into()], &secret).unwrap();
        let key = VerifyingKey::from_bytes(&hex::decode(&public).unwrap().try_into().unwrap()).unwrap();
        (dir, pkg, vec![key], secret)
    }

    #[test]
    fn signed_plugin_installs_and_lists() {
        let (dir, pkg, trusted, _) = setup();
        let root = dir.path().join("plugins");
        let inst = install(&pkg, &root, &trusted).unwrap();
        assert_eq!(inst.path, root.join("review-kit/1.0.0"));
        assert!(inst.path.join("skills/review/SKILL.md").is_file());
        let listed = list(&root, &trusted);
        assert_eq!(listed.len(), 1);
        assert!(listed[0].is_ok());
        // Tampering after install is detected on listing.
        std::fs::write(inst.path.join("README"), b"evil").unwrap();
        assert!(list(&root, &trusted)[0].is_err());
    }

    #[test]
    fn refuses_untrusted_tampered_extra_and_links() {
        let (dir, pkg, trusted, _) = setup();
        let root = dir.path().join("plugins");
        let (_, other) = keygen();
        let stranger = vec![VerifyingKey::from_bytes(&hex::decode(other).unwrap().try_into().unwrap()).unwrap()];
        assert!(install(&pkg, &root, &stranger).unwrap_err().contains("trusted"));
        std::fs::write(pkg.join("README"), b"changed").unwrap();
        assert!(install(&pkg, &root, &trusted).unwrap_err().contains("hash mismatch"));
        std::fs::write(pkg.join("README"), b"hi").unwrap();
        std::fs::write(pkg.join("extra"), b"x").unwrap();
        assert!(install(&pkg, &root, &trusted).unwrap_err().contains("do not match"));
        std::fs::remove_file(pkg.join("extra")).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/passwd", pkg.join("link")).unwrap();
            assert!(install(&pkg, &root, &trusted).unwrap_err().contains("link"));
            std::fs::remove_file(pkg.join("link")).unwrap();
        }
        // Manifest edit breaks the signature.
        let m = std::fs::read_to_string(pkg.join(MANIFEST)).unwrap().replace("1.0.0", "9.9.9");
        std::fs::write(pkg.join(MANIFEST), m).unwrap();
        assert!(install(&pkg, &root, &trusted).is_err());
        assert!(!root.join("review-kit").exists());
    }

    #[test]
    fn rejects_path_escape_in_manifest() {
        assert!(!rel_ok("../x"));
        assert!(!rel_ok("/etc/x"));
        assert!(!rel_ok(MANIFEST));
        assert!(rel_ok("skills/a/SKILL.md"));
    }

    #[test]
    fn trusted_keys_file_parses_comments() {
        let dir = tempfile::tempdir().unwrap();
        let (_, public) = keygen();
        let f = dir.path().join("keys");
        std::fs::write(&f, format!("# publishers\n{public} owner\n\n")).unwrap();
        assert_eq!(load_trusted_keys(&f).unwrap().len(), 1);
        std::fs::write(&f, "zz\n").unwrap();
        assert!(load_trusted_keys(&f).is_err());
    }
}

/// Provider config directory (relative to the provider HOME) a plugin's files land in.
pub fn provider_dir(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "claude" => ".claude",
        "codex" => ".codex",
        "kimi" => ".kimi",
        "grok" => ".grok",
        "gemini" => ".gemini",
        _ => return None,
    })
}

const MANAGED_LIST: &str = ".hatchery-plugins.json";

/// Load verified plugins into provider homes so new sessions see them: each file goes to
/// `<home>/<provider dir>/<path>` for every provider the plugin names (all known ones when
/// it names none). Only files this sync placed are ever replaced or removed (tracked in
/// `<home>/.hatchery-plugins.json`); an existing foreign file is left alone and reported.
/// Tampered plugins are skipped. Returns (placed, skipped-with-reason).
pub fn sync_into_home(root: &Path, trusted: &[VerifyingKey], home: &Path) -> (usize, Vec<String>) {
    const ALL: &[&str] = &["claude", "codex", "kimi", "grok", "gemini"];
    let list_path = home.join(MANAGED_LIST);
    let previous: BTreeMap<String, String> = std::fs::read(&list_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut managed: BTreeMap<String, String> = BTreeMap::new();
    let mut skipped = Vec::new();
    // Newest version per plugin name wins (lexicographic by version dir).
    let mut chosen: BTreeMap<String, Installed> = BTreeMap::new();
    for item in list(root, trusted) {
        match item {
            Ok(i) => {
                if chosen.get(&i.name).is_none_or(|c| c.version < i.version) {
                    chosen.insert(i.name.clone(), i);
                }
            }
            Err((p, e)) => skipped.push(format!("{}: {e}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
        }
    }
    for inst in chosen.values() {
        let Ok(bytes) = std::fs::read(inst.path.join(MANIFEST)) else { continue };
        let Ok(m) = serde_json::from_slice::<PluginManifest>(&bytes) else { continue };
        let providers: Vec<&str> = if m.providers.is_empty() { ALL.to_vec() } else { m.providers.iter().map(String::as_str).collect() };
        for p in providers {
            let Some(dir) = provider_dir(p) else { continue };
            for (rel, hash) in &m.files {
                let target_rel = format!("{dir}/{rel}");
                let target = home.join(&target_rel);
                let ours = previous.contains_key(&target_rel) || managed.contains_key(&target_rel);
                if target.exists() && !ours {
                    if sha256_file(&target).ok().as_deref() != Some(hash.as_str()) {
                        skipped.push(format!("{target_rel}: exists and is not managed"));
                    }
                    continue;
                }
                if sha256_file(&target).ok().as_deref() != Some(hash.as_str()) {
                    let _ = std::fs::create_dir_all(target.parent().expect("parent"));
                    if let Err(e) = std::fs::copy(inst.path.join(rel), &target) {
                        skipped.push(format!("{target_rel}: {e}"));
                        continue;
                    }
                }
                managed.insert(target_rel, hash.clone());
            }
        }
    }
    // Remove files we placed earlier that no plugin provides any more (only if unchanged).
    for (rel, hash) in &previous {
        if !managed.contains_key(rel) {
            let path = home.join(rel);
            if sha256_file(&path).ok().as_deref() == Some(hash.as_str()) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    let placed = managed.len();
    let _ = std::fs::write(&list_path, serde_json::to_vec_pretty(&managed).unwrap_or_default());
    (placed, skipped)
}

#[cfg(test)]
mod sync_tests {
    use super::*;

    #[test]
    fn sync_places_updates_and_removes_only_managed_files() {
        let dir = tempfile::tempdir().unwrap();
        let (secret, public) = keygen();
        let trusted = vec![VerifyingKey::from_bytes(&hex::decode(public).unwrap().try_into().unwrap()).unwrap()];
        let pkg = dir.path().join("pkg");
        std::fs::create_dir_all(pkg.join("skills/review")).unwrap();
        std::fs::write(pkg.join("skills/review/SKILL.md"), b"v1").unwrap();
        sign(&pkg, "kit", "1.0.0", vec!["claude".into()], &secret).unwrap();
        let root = dir.path().join("plugins");
        install(&pkg, &root, &trusted).unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".claude/skills/mine")).unwrap();
        std::fs::write(home.join(".claude/skills/mine/SKILL.md"), b"user").unwrap();

        let (placed, skipped) = sync_into_home(&root, &trusted, &home);
        assert_eq!((placed, skipped.len()), (1, 0));
        assert_eq!(std::fs::read(home.join(".claude/skills/review/SKILL.md")).unwrap(), b"v1");
        assert!(!home.join(".codex").exists());

        // Newer version replaces our file.
        std::fs::write(pkg.join("skills/review/SKILL.md"), b"v2").unwrap();
        sign(&pkg, "kit", "1.1.0", vec!["claude".into()], &secret).unwrap();
        install(&pkg, &root, &trusted).unwrap();
        sync_into_home(&root, &trusted, &home);
        assert_eq!(std::fs::read(home.join(".claude/skills/review/SKILL.md")).unwrap(), b"v2");

        // Plugin gone -> our file removed; the user's own skill stays.
        std::fs::remove_dir_all(&root).unwrap();
        let (placed, _) = sync_into_home(&root, &trusted, &home);
        assert_eq!(placed, 0);
        assert!(!home.join(".claude/skills/review/SKILL.md").exists());
        assert_eq!(std::fs::read(home.join(".claude/skills/mine/SKILL.md")).unwrap(), b"user");
    }

    #[test]
    fn foreign_file_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let (secret, public) = keygen();
        let trusted = vec![VerifyingKey::from_bytes(&hex::decode(public).unwrap().try_into().unwrap()).unwrap()];
        let pkg = dir.path().join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("AGENTS.md"), b"plugin").unwrap();
        sign(&pkg, "kit", "1.0.0", vec!["codex".into()], &secret).unwrap();
        let root = dir.path().join("plugins");
        install(&pkg, &root, &trusted).unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(home.join(".codex/AGENTS.md"), b"mine").unwrap();
        let (placed, skipped) = sync_into_home(&root, &trusted, &home);
        assert_eq!(placed, 0);
        assert!(skipped[0].contains("not managed"));
        assert_eq!(std::fs::read(home.join(".codex/AGENTS.md")).unwrap(), b"mine");
    }
}
