// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctl-selftest` updating itself from the local relay's own release channel.
//!
//! There is no second trust scheme. The self-test is shipped inside the relay's signed bundle for each
//! platform (publish_relay_binary.sh), so it is updated from that bundle: the version comes from the
//! relay bridge's `latest_info` call, the bundle from its `download` route, and the bundle is accepted only
//! if its sha256 matches and its zipsign signature verifies against the relay's release key, which the
//! relay's wrapper binary hands in (packaging/release-signing.pub). There is no version floor and no
//! rollback protection, by the relay's standing rule: a bad release is answered by a newer one or by the
//! signed revocation list. Nothing is ever updated without a yes, or `--update`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::upload::request;

/// Where the relay's release channel lives unless a test says otherwise.
pub const DEFAULT_BASE: &str = "https://hams.com";
/// The version check gives up after this long.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(5);
/// The download gives up after this long.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);
/// The largest bundle accepted.
pub const MAX_BUNDLE: usize = 256 * 1024 * 1024;

/// Everything the update needs to know; the relay's wrapper binary supplies it.
#[derive(Debug, Clone)]
pub struct UpdateConfig {
    /// The release channel, such as [`DEFAULT_BASE`] (https, or http to this computer).
    pub base_url: String,
    /// The version of the bundle this program shipped in (the relay's version).
    pub current_version: String,
    /// The release keys a bundle may be signed with (32-byte ed25519 public keys).
    pub keys: Vec<[u8; 32]>,
    /// The running executable, which is replaced.
    pub exe: PathBuf,
}

/// What the release channel says is newest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    /// The version, `X.Y.Z`.
    pub version: String,
    /// The sha256 of the bundle, in hexadecimal.
    pub sha256: String,
}

// [@ANCHOR: radioctld_update_platform]
/// The release platform name of this build, as the relay's channel calls it, or `None` on a
/// system the relay is not released for.
pub fn platform() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux-x86_64")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("linux-aarch64")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("macos")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("windows")
    } else {
        None
    }
}

// [@ANCHOR: radioctld_update_entry_name]
/// The file name of the self-test inside a bundle.
fn entry_name() -> &'static str {
    if cfg!(windows) {
        "radioctl-selftest.exe"
    } else {
        "radioctl-selftest"
    }
}

// [@ANCHOR: radioctld_update_is_newer]
/// True when `candidate` is a higher `X.Y.Z` than `current`. Anything else is an error.
pub fn is_newer(candidate: &str, current: &str) -> Result<bool, String> {
    let parse = |v: &str| -> Result<Vec<u64>, String> {
        let parts: Result<Vec<u64>, _> = v.trim().split('.').map(str::parse).collect();
        match parts {
            Ok(p) if p.len() == 3 => Ok(p),
            _ => Err(format!("{v:?} is not a version like 1.2.3")),
        }
    };
    Ok(parse(candidate)? > parse(current)?)
}

// [@ANCHOR: radioctld_update_fetch_latest]
/// Asks the release channel for the newest version of `platform` (the relay bridge's
/// `latest_info`, a JSON-RPC call). Within [`CHECK_TIMEOUT`]; any failure is one short line.
pub async fn fetch_latest(base_url: &str, platform: &str) -> Result<Latest, String> {
    let url = format!("{base_url}/api/relay_bridge/binary/latest_info");
    let body = json!({"jsonrpc": "2.0", "method": "call", "params": {"platform": platform}});
    let (code, answer) = request(
        "POST",
        &url,
        body.to_string().as_bytes(),
        65_536,
        CHECK_TIMEOUT,
    )
    .await?;
    if code != 200 {
        return Err(format!("the server answered {code}"));
    }
    let doc: serde_json::Value =
        serde_json::from_slice(&answer).map_err(|e| format!("not JSON: {e}"))?;
    let result = doc.get("result").ok_or("no result in the answer")?;
    if result.get("status").and_then(|s| s.as_str()) != Some("success") {
        return Err("the server has no release for this platform".into());
    }
    let field = |k: &str| result.get(k).and_then(|v| v.as_str()).map(str::to_string);
    Ok(Latest {
        version: field("version").ok_or("no version in the answer")?,
        sha256: field("sha256").ok_or("no sha256 in the answer")?,
    })
}

// [@ANCHOR: radioctld_update_download]
/// Downloads the signed bundle of `platform` (at most [`MAX_BUNDLE`] bytes, within [`DOWNLOAD_TIMEOUT`]).
pub async fn download(base_url: &str, platform: &str) -> Result<Vec<u8>, String> {
    let url = format!("{base_url}/api/relay_bridge/binary/download/{platform}");
    let (code, bytes) = request("GET", &url, b"", MAX_BUNDLE, DOWNLOAD_TIMEOUT).await?;
    if code != 200 || bytes.len() > MAX_BUNDLE {
        return Err(format!("the download answered {code}"));
    }
    Ok(bytes)
}

// [@ANCHOR: radioctld_update_verify_and_extract]
/// The self-test executable out of `bundle`, but only if the bundle's sha256 is `sha256` and its
/// zipsign signature (empty context, as publish_relay_binary.sh signs) verifies against one of
/// `keys`. A wrong hash, a missing or wrong signature, or a missing entry is an `Err`.
pub fn verify_and_extract(
    bundle: &[u8],
    sha256: &str,
    keys: &[[u8; 32]],
) -> Result<Vec<u8>, String> {
    let actual = Sha256::digest(bundle)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if !actual.eq_ignore_ascii_case(sha256) {
        return Err("the download does not match the published sha256".into());
    }
    let keys = zipsign_api::verify::collect_keys(keys.iter().map(|k| Ok(*k)))
        .map_err(|e| format!("bad release key: {e}"))?;
    zipsign_api::verify::verify_zip(&mut std::io::Cursor::new(bundle), &keys, None)
        .map_err(|e| format!("the signature does not verify: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bundle)).map_err(|e| e.to_string())?;
    let mut file = archive
        .by_name(entry_name())
        .map_err(|_| format!("{} is not in the bundle", entry_name()))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

// [@ANCHOR: radioctld_update_replace_exe]
/// Replaces `exe` with `new_bytes`, keeping the old file beside it as `NAME.prev`. The new file is
/// written next to `exe` first and moved into place by a rename, so a crash leaves either the old
/// program or the new one, never a half-written one. On any error the old program is left in place.
pub fn replace_exe(exe: &Path, new_bytes: &[u8]) -> Result<PathBuf, String> {
    let name = exe
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("no file name")?;
    let (fresh, prev) = (
        exe.with_file_name(format!("{name}.new")),
        exe.with_file_name(format!("{name}.prev")),
    );
    let fail = |what: &str, e: std::io::Error| format!("{what}: {e}");
    std::fs::write(&fresh, new_bytes).map_err(|e| fail("cannot write the new program", e))?;
    #[cfg(unix)]
    std::fs::set_permissions(&fresh, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .map_err(|e| fail("cannot make the new program runnable", e))?;
    let _ = std::fs::remove_file(&prev);
    if let Err(e) = std::fs::copy(exe, &prev) {
        let _ = std::fs::remove_file(&fresh);
        return Err(fail("cannot keep the old program", e));
    }
    if let Err(e) = std::fs::rename(&fresh, exe) {
        let _ = std::fs::remove_file(&fresh);
        return Err(fail("cannot put the new program in place", e));
    }
    Ok(prev)
}

// [@ANCHOR: radioctld_update_restart]
/// Starts the replaced program with the same arguments and returns its exit status (on Unix the
/// process is replaced and this returns only if that failed). `RADIOCTL_SELFTEST_UPDATED` is set
/// so the new run does not offer another update.
pub fn restart(exe: &Path) -> i32 {
    let mut command = std::process::Command::new(exe);
    command
        .args(std::env::args().skip(1))
        .env("RADIOCTL_SELFTEST_UPDATED", "1");
    #[cfg(unix)]
    {
        let e = std::os::unix::process::CommandExt::exec(&mut command);
        eprintln!("radioctl-selftest: cannot restart after the update: {e}");
        1
    }
    #[cfg(not(unix))]
    {
        match command.status() {
            Ok(status) => status.code().unwrap_or(1),
            Err(e) => {
                eprintln!("radioctl-selftest: cannot restart after the update: {e}");
                1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use zipsign_api::SigningKey;

    fn signed_bundle(seed: u8, content: &[u8]) -> (Vec<u8>, [u8; 32]) {
        let mut raw = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut raw));
            zip.start_file(entry_name(), zip::write::SimpleFileOptions::default())
                .expect("start");
            zip.write_all(content).expect("write");
            zip.finish().expect("finish");
        }
        let key = SigningKey::from_bytes(&[seed; 32]);
        let public = key.verifying_key().to_bytes();
        let mut out = std::io::Cursor::new(Vec::new());
        zipsign_api::sign::copy_and_sign_zip(
            &mut std::io::Cursor::new(raw),
            &mut out,
            &[key],
            None,
        )
        .expect("sign");
        (out.into_inner(), public)
    }

    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// A loopback release channel: `info` answers latest_info, `bundle` answers the download.
    pub(crate) async fn channel(info: String, bundle: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let (info, bundle) = (info.clone(), bundle.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]).to_string();
                    let body = if head.contains("latest_info") {
                        info.into_bytes()
                    } else {
                        bundle
                    };
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                });
            }
        });
        base
    }

    pub(crate) fn info(version: &str, sha256: &str) -> String {
        json!({"result": {"status": "success", "version": version, "sha256": sha256}}).to_string()
    }

    // Tests [@ANCHOR: radioctld_update_entry_name]
    #[test]
    fn the_bundle_entry_is_the_platforms_program_name() {
        assert_eq!(entry_name().ends_with(".exe"), cfg!(windows));
        assert!(entry_name().starts_with("radioctl-selftest"));
    }

    // Tests [@ANCHOR: radioctld_update_platform]
    #[test]
    fn this_build_has_a_release_platform_name_on_the_supported_systems() {
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(platform(), Some("linux-x86_64"));
        }
    }

    // Tests [@ANCHOR: radioctld_update_is_newer]
    #[test]
    fn versions_compare_by_number_and_junk_is_an_error() {
        assert_eq!(is_newer("0.10.0", "0.9.9"), Ok(true));
        assert_eq!(is_newer("0.3.3", "0.3.3"), Ok(false));
        assert_eq!(is_newer("0.3.2", "0.3.3"), Ok(false));
        assert!(is_newer("latest", "0.3.3").is_err());
    }

    // Tests [@ANCHOR: radioctld_update_fetch_latest]
    // Tests [@ANCHOR: radioctld_update_download]
    #[tokio::test]
    async fn the_channel_answers_the_version_and_the_bundle_and_offline_is_an_error() {
        let base = channel(info("0.4.0", "ab"), b"BUNDLE".to_vec()).await;
        let latest = fetch_latest(&base, "linux-x86_64").await.expect("latest");
        assert_eq!(
            latest,
            Latest {
                version: "0.4.0".into(),
                sha256: "ab".into()
            }
        );
        assert_eq!(
            download(&base, "linux-x86_64").await.expect("bundle"),
            b"BUNDLE"
        );
        let none = channel("{\"result\": {\"status\": \"error\"}}".into(), vec![]).await;
        assert!(fetch_latest(&none, "x").await.is_err());
        let closed = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let gone = format!("http://{}", closed.local_addr().expect("addr"));
        drop(closed);
        assert!(fetch_latest(&gone, "x").await.is_err());
        assert!(download(&gone, "x").await.is_err());
    }

    // Tests [@ANCHOR: radioctld_update_verify_and_extract]
    #[test]
    fn only_a_matching_hash_and_a_good_signature_give_the_program() {
        let (bundle, public) = signed_bundle(7, b"NEW PROGRAM");
        assert_eq!(
            verify_and_extract(&bundle, &sha(&bundle), &[public]).expect("ok"),
            b"NEW PROGRAM"
        );
        assert!(verify_and_extract(&bundle, &sha(b"other"), &[public]).is_err());
        let (_, other_key) = signed_bundle(8, b"x");
        assert!(verify_and_extract(&bundle, &sha(&bundle), &[other_key]).is_err());
        let mut tampered = bundle.clone();
        tampered[40] ^= 0xff;
        assert!(verify_and_extract(&tampered, &sha(&tampered), &[public]).is_err());
        let unsigned = {
            let mut raw = Vec::new();
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut raw));
            zip.start_file(entry_name(), zip::write::SimpleFileOptions::default())
                .expect("start");
            zip.finish().expect("finish");
            raw
        };
        assert!(verify_and_extract(&unsigned, &sha(&unsigned), &[public]).is_err());
    }

    // Tests [@ANCHOR: radioctld_update_replace_exe]
    #[test]
    fn replacing_keeps_the_old_program_as_prev_and_failure_leaves_it_alone() {
        let dir = std::env::temp_dir().join(format!("update-replace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let exe = dir.join("radioctl-selftest");
        std::fs::write(&exe, b"OLD").expect("old");
        let prev = replace_exe(&exe, b"NEW").expect("replace");
        assert_eq!(std::fs::read(&exe).expect("exe"), b"NEW");
        assert_eq!(std::fs::read(&prev).expect("prev"), b"OLD");
        assert!(!dir.join("radioctl-selftest.new").exists());
        assert!(replace_exe(&dir.join("missing/none"), b"X").is_err());
        assert_eq!(std::fs::read(&exe).expect("exe"), b"NEW");
        std::fs::remove_dir_all(&dir).ok();
    }

    // Tests [@ANCHOR: radioctld_update_restart]
    #[test]
    fn a_program_that_cannot_start_reports_failure_not_a_crash() {
        // Only the failure path is run here: a successful restart would replace this test process.
        #[cfg(not(unix))]
        assert_eq!(restart(Path::new("/nonexistent/radioctl-selftest")), 1);
        #[cfg(unix)]
        assert_eq!(restart(Path::new("/nonexistent/radioctl-selftest")), 1);
    }
}
