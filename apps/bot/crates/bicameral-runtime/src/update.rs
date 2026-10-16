//! Self-update logic for the Bicameral CLI binary.
//!
//! Checks the latest public release manifest, compares against the running version,
//! downloads the appropriate asset, verifies the SHA256 checksum (per the
//! v0 trust posture: SHA256SUMS),
//! and replaces the running binary.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

trait StagedBinaryValidator {
    fn validate(&self, staged: &Path) -> anyhow::Result<()>;
}

struct SystemStagedBinaryValidator;

#[cfg(any(target_os = "macos", test))]
fn validate_macos_staged_binary<F>(staged: &Path, mut run: F) -> anyhow::Result<()>
where
    F: FnMut(&str, &[&str], &Path) -> anyhow::Result<bool>,
{
    if !run("/usr/bin/codesign", &["--force", "--sign", "-"], staged)? {
        anyhow::bail!("macOS update signing failed");
    }
    if !run("/usr/bin/codesign", &["--verify", "--strict"], staged)? {
        anyhow::bail!("macOS update verification failed");
    }
    Ok(())
}

impl StagedBinaryValidator for SystemStagedBinaryValidator {
    fn validate(&self, staged: &Path) -> anyhow::Result<()> {
        #[cfg(target_os = "macos")]
        {
            validate_macos_staged_binary(staged, |program, args, path| {
                let status = std::process::Command::new(program)
                    .args(args)
                    .arg(path)
                    .status()
                    .map_err(|_| anyhow::anyhow!("macOS update validation could not be started"))?;
                Ok(status.success())
            })?;
        }

        #[cfg(not(target_os = "macos"))]
        let _ = staged;

        Ok(())
    }
}

/// Default public release artifact origin.
const DEFAULT_DOWNLOAD_BASE_URL: &str =
    "https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral";

/// Override public artifact origin for staging or emergency migration.
const DOWNLOAD_BASE_ENV: &str = "BICAMERAL_DOWNLOAD_BASE";

/// GitHub owner/repo for the legacy private release lookup.
const GITHUB_REPO: &str = "BicameralAI/bicameral-bot";

#[derive(Debug, Deserialize)]
struct ReleaseManifest {
    /// Semver version, with or without a leading `v`.
    version: String,
    /// URL to the SHA256SUMS file for this release.
    checksums_url: Option<String>,
    /// Platform target triple to artifact metadata.
    assets: BTreeMap<String, ReleaseManifestAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseManifestAsset {
    name: String,
    url: String,
}

/// Information about an available release.
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    /// Semver tag, e.g. "0.2.0" (stripped of leading 'v').
    pub version: String,
    /// URL to the platform-appropriate tarball.
    pub asset_url: String,
    /// URL to the SHA256SUMS file.
    pub checksums_url: String,
    /// Asset filename, e.g. "bicameral-x86_64-unknown-linux-gnu.tar.gz".
    pub asset_name: String,
}

/// Outcome of a version check.
#[derive(Debug)]
pub enum UpdateCheck {
    /// A newer version is available.
    Available(ReleaseInfo),
    /// Already running the latest version.
    UpToDate { current: String },
    /// Could not determine (e.g. dev build without version).
    Skipped { reason: String },
}

/// Verified release contents extracted from a downloaded archive.
#[derive(Debug, Clone)]
pub struct VerifiedRelease {
    /// Persisted temporary extraction root. Kept so payload paths remain valid.
    pub root: PathBuf,
    /// Path to the extracted `bicameral` binary.
    pub binary: PathBuf,
}

/// Return the target triple string used in release asset names.
fn release_target() -> &'static str {
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    {
        "x86_64-unknown-linux-gnu"
    }
    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    {
        "aarch64-unknown-linux-gnu"
    }
    #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
    {
        "x86_64-apple-darwin"
    }
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    {
        "aarch64-apple-darwin"
    }
    #[cfg(not(any(
        all(target_arch = "x86_64", target_os = "linux"),
        all(target_arch = "aarch64", target_os = "linux"),
        all(target_arch = "x86_64", target_os = "macos"),
        all(target_arch = "aarch64", target_os = "macos"),
    )))]
    {
        "unknown"
    }
}

/// Check the latest public release manifest and compare to the running version.
pub async fn check_latest(current_version: &str) -> anyhow::Result<UpdateCheck> {
    let target = release_target();
    if target == "unknown" {
        return Ok(UpdateCheck::Skipped {
            reason: "unsupported platform for release assets".into(),
        });
    }

    let download_base = release_download_base();
    let manifest_url = format!("{download_base}/latest.json");

    let client = reqwest::Client::builder()
        .user_agent("bicameral-update")
        .build()?;

    let resp = client.get(&manifest_url).send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(UpdateCheck::Skipped {
            reason: "no public release manifest found".into(),
        });
    }

    let resp = resp.error_for_status()?;
    let manifest: ReleaseManifest = resp.json().await?;
    release_from_manifest(current_version, target, &download_base, manifest)
}

#[allow(dead_code)]
async fn check_latest_from_github(current_version: &str) -> anyhow::Result<UpdateCheck> {
    let target = release_target();
    if target == "unknown" {
        return Ok(UpdateCheck::Skipped {
            reason: "unsupported platform for release assets".into(),
        });
    }

    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );

    let client = reqwest::Client::builder()
        .user_agent("bicameral-update")
        .build()?;

    let resp = client.get(&url).send().await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(UpdateCheck::Skipped {
            reason: "no releases found".into(),
        });
    }

    let resp = resp.error_for_status()?;
    let release: serde_json::Value = resp.json().await?;

    let tag = release["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing tag_name in release response"))?;

    let latest_version = tag.strip_prefix('v').unwrap_or(tag);

    if !is_newer(current_version, latest_version) {
        return Ok(UpdateCheck::UpToDate {
            current: current_version.to_string(),
        });
    }

    let expected_asset = format!("bicameral-{}.tar.gz", target);
    let checksums_name = "SHA256SUMS";

    let assets = release["assets"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("no assets in release"))?;

    let asset_url = assets
        .iter()
        .find(|a| a["name"].as_str() == Some(&expected_asset))
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "release {} missing asset {}",
                latest_version,
                expected_asset
            )
        })?
        .to_string();

    let checksums_url = assets
        .iter()
        .find(|a| a["name"].as_str() == Some(checksums_name))
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or_else(|| {
            anyhow::anyhow!("release {} missing {} file", latest_version, checksums_name)
        })?
        .to_string();

    Ok(UpdateCheck::Available(ReleaseInfo {
        version: latest_version.to_string(),
        asset_url,
        checksums_url,
        asset_name: expected_asset,
    }))
}

fn release_download_base() -> String {
    std::env::var(DOWNLOAD_BASE_ENV)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_DOWNLOAD_BASE_URL.to_string())
}

fn release_from_manifest(
    current_version: &str,
    target: &str,
    download_base: &str,
    manifest: ReleaseManifest,
) -> anyhow::Result<UpdateCheck> {
    let latest_version = manifest
        .version
        .strip_prefix('v')
        .unwrap_or(&manifest.version);

    if !is_newer(current_version, latest_version) {
        return Ok(UpdateCheck::UpToDate {
            current: current_version.to_string(),
        });
    }

    let expected_asset = format!("bicameral-{}.tar.gz", target);
    let asset = manifest.assets.get(target).ok_or_else(|| {
        anyhow::anyhow!(
            "release {} missing target asset {}",
            latest_version,
            expected_asset
        )
    })?;

    if asset.name != expected_asset {
        anyhow::bail!(
            "release {} target {} points to unexpected asset {}",
            latest_version,
            target,
            asset.name
        );
    }

    let checksums_url = manifest
        .checksums_url
        .unwrap_or_else(|| format!("{download_base}/v{latest_version}/SHA256SUMS"));

    Ok(UpdateCheck::Available(ReleaseInfo {
        version: latest_version.to_string(),
        asset_url: asset.url.clone(),
        checksums_url,
        asset_name: expected_asset,
    }))
}

/// Download the release asset and SHA256SUMS, verify checksum, and extract it.
/// Returns verified release contents in a persisted temporary directory.
pub async fn download_and_verify(release: &ReleaseInfo) -> anyhow::Result<VerifiedRelease> {
    let client = reqwest::Client::builder()
        .user_agent("bicameral-update")
        .build()?;

    // Download SHA256SUMS
    let checksums_text = client
        .get(&release.checksums_url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let expected_hash = parse_checksum(&checksums_text, &release.asset_name)?;

    // Download asset
    let asset_bytes = client
        .get(&release.asset_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    // Verify SHA256
    let actual_hash = hex_sha256(&asset_bytes);
    if actual_hash != expected_hash {
        anyhow::bail!(
            "checksum mismatch for {}:\n  expected: {}\n  actual:   {}",
            release.asset_name,
            expected_hash,
            actual_hash,
        );
    }

    tracing::info!(
        "checksum verified for {} (SHA256: {})",
        release.asset_name,
        &actual_hash[..16]
    );

    // Extract tarball to temp dir
    let tmp_dir = tempfile::tempdir()?;
    let tar_gz = std::io::Cursor::new(asset_bytes.as_ref());
    let tar = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);
    archive.unpack(tmp_dir.path())?;

    // Find release payloads in extracted contents.
    let binary_path = find_binary_in_dir(tmp_dir.path())?;

    // Persist the tempdir so the verified payload remains available to install.
    #[allow(deprecated)]
    let tmp_path = tmp_dir.into_path();

    Ok(VerifiedRelease {
        root: tmp_path,
        binary: binary_path,
    })
}

/// Replace the running install with the verified release payload.
/// The old binary is kept as `<path>.old` for rollback.
pub fn replace_binary(current: &Path, release: &VerifiedRelease) -> anyhow::Result<PathBuf> {
    replace_binary_with_validator(current, release, &SystemStagedBinaryValidator)
}

fn replace_binary_with_validator(
    current: &Path,
    release: &VerifiedRelease,
    validator: &dyn StagedBinaryValidator,
) -> anyhow::Result<PathBuf> {
    let backup = current.with_extension("old");
    let staged = current.with_extension("update-staged");

    if staged.exists() {
        std::fs::remove_file(&staged)
            .map_err(|_| anyhow::anyhow!("could not clear a prior staged update"))?;
    }

    let install_result = (|| -> anyhow::Result<()> {
        std::fs::copy(&release.binary, &staged)
            .map_err(|_| anyhow::anyhow!("could not stage the verified update"))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&staged)
                .map_err(|_| anyhow::anyhow!("could not inspect the staged update"))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&staged, perms)
                .map_err(|_| anyhow::anyhow!("could not make the staged update executable"))?;
        }

        validator.validate(&staged)?;

        if current.exists() {
            if backup.exists() {
                std::fs::remove_file(&backup)
                    .map_err(|_| anyhow::anyhow!("could not clear the prior update backup"))?;
            }
            std::fs::rename(current, &backup)
                .map_err(|_| anyhow::anyhow!("could not preserve the installed binary"))?;
        }

        if std::fs::rename(&staged, current).is_err() {
            if backup.exists() {
                let _ = std::fs::rename(&backup, current);
            }
            anyhow::bail!("could not install the validated update; the prior binary was preserved");
        }

        Ok(())
    })();

    if install_result.is_err() && staged.exists() {
        let _ = std::fs::remove_file(&staged);
    }

    // Clean up verified payloads on either success or a fail-closed install.
    std::fs::remove_dir_all(&release.root).ok();
    install_result?;

    Ok(backup)
}

/// Parse a SHA256SUMS file and return the hex digest for the given filename.
fn parse_checksum(checksums: &str, filename: &str) -> anyhow::Result<String> {
    for line in checksums.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Format: "<hash>  <filename>" or "<hash> <filename>"
        if let Some((hash, name)) = line.split_once(char::is_whitespace) {
            let name = name.trim().trim_start_matches('*');
            if name == filename {
                return Ok(hash.to_lowercase());
            }
        }
    }
    anyhow::bail!("no checksum entry found for {} in SHA256SUMS", filename)
}

fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Simple version comparison: returns true if `latest` is strictly newer than `current`.
/// Handles semver-style "X.Y.Z" strings.
fn is_newer(current: &str, latest: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let c = parse(current);
    let l = parse(latest);
    l > c
}

/// Find the `bicameral` binary inside an extracted directory.
fn find_binary_in_dir(dir: &Path) -> anyhow::Result<PathBuf> {
    // Could be at top level or one directory deep
    let direct = dir.join("bicameral");
    if direct.is_file() {
        return Ok(direct);
    }
    // Search one level deep
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let candidate = path.join("bicameral");
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }
    anyhow::bail!("bicameral binary not found in extracted archive")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison() {
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.1.0", "1.0.0"));
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(!is_newer("0.2.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("1.0.0", "0.9.0"));
    }

    #[test]
    fn parse_checksum_file() {
        let sums = "abc123  bicameral-x86_64-unknown-linux-gnu.tar.gz\ndef456  bicameral-aarch64-apple-darwin.tar.gz\n";
        assert_eq!(
            parse_checksum(sums, "bicameral-x86_64-unknown-linux-gnu.tar.gz").unwrap(),
            "abc123"
        );
        assert_eq!(
            parse_checksum(sums, "bicameral-aarch64-apple-darwin.tar.gz").unwrap(),
            "def456"
        );
        assert!(parse_checksum(sums, "nonexistent.tar.gz").is_err());
    }

    #[test]
    fn hex_sha256_known() {
        let hash = hex_sha256(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn release_target_is_known() {
        let target = release_target();
        // In CI (linux x86_64), this should be known
        assert_ne!(target, "unknown", "release target should be resolved");
    }

    #[test]
    fn release_manifest_resolves_target_asset() {
        let manifest = ReleaseManifest {
            version: "v0.2.0".to_string(),
            checksums_url: Some(
                "https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral/v0.2.0/SHA256SUMS".to_string(),
            ),
            assets: BTreeMap::from([(
                "x86_64-unknown-linux-gnu".to_string(),
                ReleaseManifestAsset {
                    name: "bicameral-x86_64-unknown-linux-gnu.tar.gz".to_string(),
                    url: "https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral/v0.2.0/bicameral-x86_64-unknown-linux-gnu.tar.gz"
                        .to_string(),
                },
            )]),
        };

        let update = release_from_manifest(
            "0.1.0",
            "x86_64-unknown-linux-gnu",
            DEFAULT_DOWNLOAD_BASE_URL,
            manifest,
        )
        .unwrap();

        match update {
            UpdateCheck::Available(release) => {
                assert_eq!(release.version, "0.2.0");
                assert_eq!(
                    release.asset_name,
                    "bicameral-x86_64-unknown-linux-gnu.tar.gz"
                );
                assert_eq!(
                    release.checksums_url,
                    "https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral/v0.2.0/SHA256SUMS"
                );
            }
            other => panic!("expected available update, got {other:?}"),
        }
    }

    #[test]
    fn release_manifest_uses_default_checksum_url() {
        let manifest = ReleaseManifest {
            version: "0.2.0".to_string(),
            checksums_url: None,
            assets: BTreeMap::from([(
                "aarch64-apple-darwin".to_string(),
                ReleaseManifestAsset {
                    name: "bicameral-aarch64-apple-darwin.tar.gz".to_string(),
                    url: "https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral/v0.2.0/bicameral-aarch64-apple-darwin.tar.gz"
                        .to_string(),
                },
            )]),
        };

        let update = release_from_manifest(
            "0.1.0",
            "aarch64-apple-darwin",
            DEFAULT_DOWNLOAD_BASE_URL,
            manifest,
        )
        .unwrap();

        match update {
            UpdateCheck::Available(release) => {
                assert_eq!(
                    release.checksums_url,
                    "https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral/v0.2.0/SHA256SUMS"
                );
            }
            other => panic!("expected available update, got {other:?}"),
        }
    }

    #[test]
    fn replace_binary_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("install");
        let release_root = tmp.path().join("release");
        let fake_current = install_dir.join("bicameral");
        let fake_new = release_root.join("bicameral");

        std::fs::create_dir_all(&install_dir).unwrap();
        std::fs::create_dir_all(&release_root).unwrap();
        std::fs::write(&fake_current, b"old-binary").unwrap();
        std::fs::write(&fake_new, b"new-binary").unwrap();

        let release = VerifiedRelease {
            root: release_root,
            binary: fake_new,
        };

        let backup = replace_binary(&fake_current, &release).unwrap();
        assert_eq!(
            std::fs::read_to_string(&fake_current).unwrap(),
            "new-binary"
        );
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "old-binary");
        assert!(
            !release.root.exists(),
            "verified extraction dir should be cleaned up"
        );
    }

    struct RejectingValidator;

    impl StagedBinaryValidator for RejectingValidator {
        fn validate(&self, _staged: &Path) -> anyhow::Result<()> {
            anyhow::bail!("platform validation failed")
        }
    }

    #[test]
    fn platform_validation_failure_preserves_installed_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("install");
        let release_root = tmp.path().join("release");
        let current = install_dir.join("bicameral");
        let incoming = release_root.join("bicameral");

        std::fs::create_dir_all(&install_dir).unwrap();
        std::fs::create_dir_all(&release_root).unwrap();
        std::fs::write(&current, b"runnable-old-binary").unwrap();
        std::fs::write(&incoming, b"unvalidated-new-binary").unwrap();

        let release = VerifiedRelease {
            root: release_root,
            binary: incoming,
        };

        let error = replace_binary_with_validator(&current, &release, &RejectingValidator)
            .unwrap_err()
            .to_string();

        assert!(error.contains("platform validation failed"));
        assert_eq!(std::fs::read(&current).unwrap(), b"runnable-old-binary");
        assert!(!current.with_extension("update-staged").exists());
    }

    #[test]
    fn macos_validation_signs_then_verifies_the_staged_binary() {
        let staged = Path::new("/tmp/bicameral.update-staged");
        let mut calls = Vec::new();

        validate_macos_staged_binary(staged, |program, args, path| {
            calls.push((
                program.to_string(),
                args.iter()
                    .map(|arg| (*arg).to_string())
                    .collect::<Vec<_>>(),
                path.to_path_buf(),
            ));
            Ok(true)
        })
        .unwrap();

        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "/usr/bin/codesign");
        assert_eq!(calls[0].1, ["--force", "--sign", "-"]);
        assert_eq!(calls[0].2, staged);
        assert_eq!(calls[1].1, ["--verify", "--strict"]);
        assert_eq!(calls[1].2, staged);
    }

    #[test]
    fn macos_signing_failure_does_not_attempt_verification() {
        let staged = Path::new("/tmp/bicameral.update-staged");
        let mut calls = 0;
        let error = validate_macos_staged_binary(staged, |_program, _args, _path| {
            calls += 1;
            Ok(false)
        })
        .unwrap_err()
        .to_string();

        assert_eq!(calls, 1);
        assert_eq!(error, "macOS update signing failed");
    }

    #[test]
    fn macos_verification_failure_is_redaction_safe() {
        let staged = Path::new("/Users/private/work/bicameral.update-staged");
        let mut calls = 0;
        let error = validate_macos_staged_binary(staged, |_program, _args, _path| {
            calls += 1;
            Ok(calls == 1)
        })
        .unwrap_err()
        .to_string();

        assert_eq!(calls, 2);
        assert_eq!(error, "macOS update verification failed");
        assert!(!error.contains("/Users/private"));
    }

    #[test]
    fn replacement_failure_preserves_installed_binary_and_dashboard_sibling() {
        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("install");
        let release_root = tmp.path().join("release");
        let current = install_dir.join("bicameral");
        let dashboard = install_dir.join("dashboard");
        let incoming = release_root.join("bicameral");
        let backup = current.with_extension("old");

        std::fs::create_dir_all(&install_dir).unwrap();
        std::fs::create_dir_all(&release_root).unwrap();
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(&current, b"runnable-old-binary").unwrap();
        std::fs::write(&dashboard, b"existing-dashboard-assets").unwrap();
        std::fs::write(&incoming, b"new-binary").unwrap();

        let release = VerifiedRelease {
            root: release_root,
            binary: incoming,
        };

        let error = replace_binary(&current, &release).unwrap_err().to_string();
        assert_eq!(error, "could not clear the prior update backup");
        assert_eq!(std::fs::read(&current).unwrap(), b"runnable-old-binary");
        assert_eq!(
            std::fs::read(&dashboard).unwrap(),
            b"existing-dashboard-assets"
        );
        assert!(!current.with_extension("update-staged").exists());
    }
}
