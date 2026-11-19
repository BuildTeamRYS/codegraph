//! `codegraph update` acceptance (phase 0.5):
//! - latest release tag resolved from the /releases/latest redirect
//! - binary + checksums download, SHA-256 verified before any install
//! - a tampered checksum aborts and nothing is installed
//! - no releases yet -> clean error, never a panic
//! - replace_executable swaps the binary (content + mode)

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread::JoinHandle;

use codegraph_cli::update::{
    download_verified, latest_tag, parse_checksums, replace_executable, sha256_hex,
};

type Response = (u16, Vec<(String, String)>, Vec<u8>);
type Route = (String, Response);

/// Minimal one-shot fake release server. Serves `routes` (path → body),
/// status 200, with `Connection: close`. Anything else gets 404.
struct FakeRelease {
    base: String,
    handle: Option<JoinHandle<()>>,
}

impl FakeRelease {
    fn serve(routes: Vec<Route>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let map: HashMap<String, Response> = routes.into_iter().collect();
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                // Read until end of headers; these requests have no body.
                loop {
                    let n = match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&buf).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, headers, body) = map.get(&path).cloned().unwrap_or((
                    404,
                    vec![("content-type".to_string(), "text/plain".to_string())],
                    b"not found".to_vec(),
                ));
                let mut response = format!(
                    "HTTP/1.1 {status} X\r\nconnection: close\r\ncontent-length: {}\r\n",
                    body.len()
                );
                for (k, v) in &headers {
                    response.push_str(&format!("{k}: {v}\r\n"));
                }
                response.push_str("\r\n");
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        Self {
            base: format!("http://127.0.0.1:{port}"),
            handle: Some(handle),
        }
    }
}

impl Drop for FakeRelease {
    fn drop(&mut self) {
        // Daemon-ish listeners die with the process; detach the thread.
        drop(self.handle.take());
    }
}

fn ok(body: &[u8]) -> Response {
    (200, vec![], body.to_vec())
}

fn redirect(location: &str) -> Response {
    (
        302,
        vec![("location".to_string(), location.to_string())],
        vec![],
    )
}

fn checksums_for(asset: &str, content: &[u8]) -> Vec<u8> {
    format!("{}  {}\n", sha256_hex(content), asset).into_bytes()
}

#[test]
fn latest_tag_resolves_from_redirect() {
    let server = FakeRelease::serve(vec![(
        "/releases/latest".to_string(),
        redirect("/releases/tag/v99.0.0"),
    )]);
    let tag = latest_tag(&server.base).unwrap();
    assert_eq!(tag, "v99.0.0");
}

#[test]
fn latest_tag_404_is_a_clean_error() {
    let server = FakeRelease::serve(vec![]);
    let err = latest_tag(&server.base).unwrap_err();
    assert!(err.to_string().contains("no releases found"), "got: {err}");
}

#[test]
fn download_verified_happy_path() {
    let asset = "codegraph-x86_64-unknown-linux-musl";
    let binary = b"fake-binary-bytes";
    let server = FakeRelease::serve(vec![
        (format!("/releases/download/v99.0.0/{asset}"), ok(binary)),
        (
            "/releases/download/v99.0.0/checksums.txt".to_string(),
            ok(&checksums_for(asset, binary)),
        ),
    ]);
    let bytes = download_verified(&server.base, "v99.0.0", asset).unwrap();
    assert_eq!(bytes, binary);
}

#[test]
fn download_verified_aborts_on_tampered_binary() {
    let asset = "codegraph-x86_64-unknown-linux-musl";
    let real = b"real-binary";
    let tampered = b"EVIL-binary";
    let server = FakeRelease::serve(vec![
        (format!("/releases/download/v99.0.0/{asset}"), ok(tampered)),
        (
            "/releases/download/v99.0.0/checksums.txt".to_string(),
            ok(&checksums_for(asset, real)),
        ),
    ]);
    let err = download_verified(&server.base, "v99.0.0", asset).unwrap_err();
    assert!(err.to_string().contains("checksum mismatch"), "got: {err}");
}

#[test]
fn download_verified_aborts_when_asset_missing_from_checksums() {
    let asset = "codegraph-x86_64-unknown-linux-musl";
    let server = FakeRelease::serve(vec![
        (format!("/releases/download/v99.0.0/{asset}"), ok(b"bin")),
        (
            "/releases/download/v99.0.0/checksums.txt".to_string(),
            ok(b"abcd1234  some-other-asset\n"),
        ),
    ]);
    let err = download_verified(&server.base, "v99.0.0", asset).unwrap_err();
    assert!(err.to_string().contains("no entry"), "got: {err}");
}

#[test]
fn parse_checksums_handles_sha256sum_shapes() {
    let map =
        parse_checksums("deadbeef  codegraph-linux\ncafebabe *codegraph-windows.exe\n\n").unwrap();
    assert_eq!(map["codegraph-linux"], "deadbeef");
    assert_eq!(map["codegraph-windows.exe"], "cafebabe");
    assert!(parse_checksums("garbage").is_none());
}

#[test]
fn replace_executable_swaps_bytes_and_mode() {
    let dir = tempfile::tempdir().unwrap();
    let exe: PathBuf = dir.path().join("codegraph");
    std::fs::write(&exe, b"old-binary").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    replace_executable(&exe, b"new-binary").unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), b"new-binary");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
}
