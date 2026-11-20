//! `codegraph update`: self-update the binary from GitHub Releases.
//!
//! Flow: resolve the latest release tag (GET <base>/releases/latest with
//! redirects disabled; the Location header carries the tag) → download
//! `codegraph-<target>[.exe]` and `checksums.txt` → verify SHA-256 →
//! atomically replace the current executable.
//!
//! The release base defaults to the project's GitHub repo and can be
//! overridden with `CODEGRAPH_RELEASE_BASE` (used by tests and for
//! unofficial mirrors). No other network access happens, ever.
//!
//! A checksum mismatch aborts the update and leaves the current binary
//! untouched. An older-or-equal latest tag is a no-op.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

/// Default release location. Overridable via `CODEGRAPH_RELEASE_BASE`.
pub const RELEASES_BASE: &str = "https://github.com/BuildTeamRYS/codegraph";
pub const ENV_BASE: &str = "CODEGRAPH_RELEASE_BASE";

pub fn releases_base() -> String {
    std::env::var(ENV_BASE)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| RELEASES_BASE.to_string())
}

/// Release target triple for this build. Linux targets are static musl
/// (zero runtime dependencies).
pub fn target_triple() -> &'static str {
    cfg_if::cfg_if! {
        if #[cfg(all(target_os = "linux", target_arch = "x86_64"))] {
            "x86_64-unknown-linux-musl"
        } else if #[cfg(all(target_os = "linux", target_arch = "aarch64"))] {
            "aarch64-unknown-linux-musl"
        } else if #[cfg(all(target_os = "macos", target_arch = "aarch64"))] {
            "aarch64-apple-darwin"
        } else if #[cfg(all(target_os = "macos", target_arch = "x86_64"))] {
            "x86_64-apple-darwin"
        } else if #[cfg(all(target_os = "windows", target_arch = "x86_64"))] {
            "x86_64-pc-windows-msvc"
        } else if #[cfg(all(target_os = "windows", target_arch = "aarch64"))] {
            "aarch64-pc-windows-msvc"
        } else {
            "unsupported"
        }
    }
}

pub fn asset_name(target: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("codegraph-{target}.exe")
    } else {
        format!("codegraph-{target}")
    }
}

fn agent_for(base: &str) -> ureq::Agent {
    // Allow plain http only for test/mirror bases; production is always https.
    let https_only = !base.starts_with("http://");
    ureq::Agent::config_builder()
        .https_only(https_only)
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .new_agent()
}

/// Resolve the latest release tag (e.g. `v0.2.0`) via the redirect that
/// GitHub serves at `<base>/releases/latest`.
pub fn latest_tag(base: &str) -> Result<String> {
    let url = format!("{base}/releases/latest");
    let agent = agent_for(base);
    let mut res = agent
        .get(&url)
        .call()
        .with_context(|| format!("failed to reach {url} (offline?)"))?;
    let status = res.status().as_u16();
    if status == 404 {
        bail!("no releases found at {base} yet — nothing to update to");
    }
    let location = res
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .with_context(|| format!("unexpected response {status} from {url} (no Location header); are releases published at this base URL?"))?;
    res.body_mut().read_to_string().ok();
    let tag = location
        .rsplit("/tag/")
        .next()
        .filter(|t| !t.is_empty() && !t.contains('/'))
        .with_context(|| format!("could not parse release tag from Location: {location}"))?;
    Ok(tag.to_string())
}

/// Download `asset` and `checksums.txt` from release `tag` at `base`,
/// verify the SHA-256, and return the binary bytes. Aborts (Err) on any
/// mismatch — the caller must not touch the current binary in that case.
pub fn download_verified(base: &str, tag: &str, asset: &str) -> Result<Vec<u8>> {
    let bin_url = format!("{base}/releases/download/{tag}/{asset}");
    let sums_url = format!("{base}/releases/download/{tag}/checksums.txt");

    let bin = fetch_bytes(&bin_url).with_context(|| format!("failed to download {bin_url}"))?;
    let sums = fetch_string(&sums_url).with_context(|| format!("failed to download {sums_url}"))?;

    let expected = parse_checksums(&sums)
        .and_then(|m| m.get(asset).cloned())
        .with_context(|| format!("checksums.txt has no entry for {asset}"))?;
    let actual = sha256_hex(&bin);
    if actual != expected {
        bail!(
            "checksum mismatch for {asset} (expected {expected}, got {actual}) — refusing to install; your existing binary was not touched"
        );
    }
    Ok(bin)
}

fn fetch_bytes(url: &str) -> Result<Vec<u8>> {
    // GitHub asset URLs redirect to a CDN; redirects are expected here.
    // The checksum gate is the real integrity check.
    let res = ureq::get(url).call()?;
    if res.status().as_u16() != 200 {
        bail!("unexpected status {} for {url}", res.status().as_u16());
    }
    let mut body: Vec<u8> = Vec::new();
    res.into_body()
        .into_reader()
        .take(512 * 1024 * 1024)
        .read_to_end(&mut body)?;
    Ok(body)
}

fn fetch_string(url: &str) -> Result<String> {
    let bytes = fetch_bytes(url)?;
    Ok(String::from_utf8(bytes)?)
}

/// Parse `checksums.txt` lines of the form `<sha256>  <name>` (optionally
/// with a leading `*` binary marker, sha256sum-style).
pub fn parse_checksums(sums: &str) -> Option<std::collections::HashMap<String, String>> {
    let mut map = std::collections::HashMap::new();
    for line in sums.lines() {
        let mut parts = line.split_whitespace();
        let (Some(hash), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        let name = name.trim_start_matches('*');
        map.insert(name.to_string(), hash.to_ascii_lowercase());
    }
    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Replace `exe` with `bytes` atomically as far as the OS allows.
/// On Windows the running executable cannot be overwritten; we rename it
/// aside and move the new one into place instead.
pub fn replace_executable(exe: &Path, bytes: &[u8]) -> Result<()> {
    let tmp: PathBuf = exe.with_file_name(format!(".codegraph-update-{}", std::process::id()));
    fs::write(&tmp, bytes).with_context(|| format!("cannot write {}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
        fs::rename(&tmp, exe).with_context(|| format!("cannot replace {}", exe.display()))?;
    }
    #[cfg(windows)]
    {
        let old = exe.with_extension("old");
        let _ = fs::remove_file(&old);
        fs::rename(exe, &old).with_context(|| format!("cannot move {} aside", exe.display()))?;
        if let Err(e) = fs::rename(&tmp, exe) {
            let _ = fs::rename(&old, exe); // roll back
            bail!("cannot install updated binary: {e}");
        }
        let _ = fs::remove_file(&old); // best effort; running exe may block this
    }
    Ok(())
}

pub enum UpdateOutcome {
    UpToDate { version: String },
    UpdateAvailable { current: String, latest: String },
    Updated { from: String, to: String },
}

impl UpdateOutcome {
    pub fn message(&self) -> String {
        match self {
            UpdateOutcome::UpToDate { version } => {
                format!("codegraph is up to date (v{version})")
            }
            UpdateOutcome::UpdateAvailable { current, latest } => {
                format!("update available: v{current} -> {latest} (run `codegraph update`)")
            }
            UpdateOutcome::Updated { from, to } => {
                format!("updated codegraph v{from} -> {to}")
            }
        }
    }
}

/// `check_only`: report only. Otherwise download, verify and replace the
/// current executable.
pub fn update(current_version: &str, check_only: bool) -> Result<UpdateOutcome> {
    let base = releases_base();
    let target = target_triple();
    if target == "unsupported" {
        bail!("no release target for this platform");
    }
    let tag = latest_tag(&base)?;
    let latest = tag.trim_start_matches('v').to_string();
    if versions_equal(&latest, current_version) {
        return Ok(UpdateOutcome::UpToDate {
            version: current_version.to_string(),
        });
    }
    if check_only {
        return Ok(UpdateOutcome::UpdateAvailable {
            current: current_version.to_string(),
            latest,
        });
    }
    let asset = asset_name(target);
    let bytes = download_verified(&base, &tag, &asset)?;
    let exe = std::env::current_exe().context("cannot locate current executable")?;
    replace_executable(&exe, &bytes)?;
    Ok(UpdateOutcome::Updated {
        from: current_version.to_string(),
        to: latest,
    })
}

fn versions_equal(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim_start_matches('v').trim().to_string();
    norm(a) == norm(b)
}
