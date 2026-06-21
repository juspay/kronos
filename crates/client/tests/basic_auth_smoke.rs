//! Smoke tests that verify the actual transport-level `Authorization`
//! header produced by [`Config::with_basic`] and [`Config::with_bearer`].
//!
//! These tests exist because of a defect found post-`a33a9a1`: the smithy
//! orchestrator resolves identity BEFORE `modify_before_signing` runs and
//! the bearer signer fires AFTER it. The original `with_basic` interceptor
//! hooked `modify_before_signing` and therefore:
//!
//!   1. failed with `NoMatchingAuthSchemeError` when used alone (no
//!      identity resolver matched the bearer auth scheme), and
//!   2. was overwritten by the bearer signer when both basic and bearer
//!      were configured.
//!
//! The fix moves the interceptor to `modify_before_transmit` (which runs
//! AFTER signing) and installs a placeholder bearer token so identity
//! resolution succeeds. These tests assert the on-the-wire header is what
//! we expect for each builder.
//!
//! Approach: spin up a tiny hand-rolled TCP server that reads the raw
//! HTTP request, captures the `Authorization` header line, then returns a
//! minimal HTTP/1.1 response. We don't care if the SDK parses the body —
//! we only want to observe the bytes the SDK puts on the wire.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use kronos_sdk::config::BehaviorVersion;
use kronos_sdk::{Client, Config};

/// Run a single-shot HTTP "server" on a fresh ephemeral port. Returns
/// `(endpoint_url, captured_authorization_header)`.
///
/// The server accepts ONE connection, reads the request line + headers
/// (until CRLF CRLF), captures the `Authorization:` line, drains any
/// declared `Content-Length` body, and writes back a fixed 200 response
/// with an empty JSON body. Anything past that is ignored.
fn spawn_capture_server() -> (String, mpsc::Receiver<Option<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
    listener
        .set_nonblocking(false)
        .expect("listener blocking mode");
    let addr = listener.local_addr().expect("local_addr");
    let endpoint_url = format!("http://{}", addr);

    let (tx, rx) = mpsc::channel::<Option<String>>();

    thread::spawn(move || {
        // Single-shot accept. accept() blocks until a connection arrives.
        let (mut stream, _peer) = match listener.accept() {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("capture server accept failed: {e}");
                let _ = tx.send(None);
                return;
            }
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

        let captured = read_request_capture_authorization(&mut stream);
        // Send a minimal HTTP/1.1 response. We don't care if the SDK
        // successfully deserializes the body; the test asserts on the
        // header we just captured.
        let resp = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
        let _ = stream.write_all(resp);
        let _ = stream.flush();
        // Best-effort shutdown so the client sees EOF promptly.
        let _ = stream.shutdown(std::net::Shutdown::Write);

        let _ = tx.send(captured);
    });

    (endpoint_url, rx)
}

fn read_request_capture_authorization(stream: &mut TcpStream) -> Option<String> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut authorization: Option<String> = None;
    let mut content_length: usize = 0;

    // Read the request line.
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }

    // Read headers until blank line.
    loop {
        let mut header_line = String::new();
        let n = reader.read_line(&mut header_line).ok()?;
        if n == 0 {
            break;
        }
        let trimmed = header_line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            let name_lc = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name_lc == "authorization" {
                authorization = Some(value);
            } else if name_lc == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
        }
    }

    // Drain the body, if any, so the client's write completes cleanly.
    if content_length > 0 {
        let mut body = vec![0u8; content_length];
        let _ = reader.read_exact(&mut body);
    }

    authorization
}

fn build_config(endpoint_url: &str) -> Config {
    Config::builder()
        .endpoint_url(endpoint_url)
        .behavior_version(BehaviorVersion::latest())
        .build()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_basic_sends_basic_authorization_header() {
    let (endpoint_url, rx) = spawn_capture_server();

    let config = build_config(&endpoint_url).with_basic("svc-1", "s3cret");
    let client = Client::from_conf(config);

    // We don't care whether send() succeeds — we only care what the SDK
    // put on the wire. Use list_jobs with minimal required inputs.
    let _ = client
        .list_jobs()
        .org_id("test-org")
        .workspace_id("test-ws")
        .send()
        .await;

    // The capture server runs on its own OS thread; spawn_blocking lets us
    // join it without stalling the async runtime.
    let captured = tokio::task::spawn_blocking(move || {
        rx.recv_timeout(Duration::from_secs(10))
            .ok()
            .and_then(|opt| opt)
    })
    .await
    .expect("join capture thread");

    let auth = captured.expect(
        "capture server saw no Authorization header — the SDK either failed before \
         transmit (the original `NoMatchingAuthSchemeError` regression) or sent the \
         request without an Authorization header",
    );

    // base64("svc-1:s3cret") == "c3ZjLTE6czNjcmV0"
    assert_eq!(
        auth, "Basic c3ZjLTE6czNjcmV0",
        "with_basic must produce a transport-level `Authorization: Basic <b64(id:secret)>` header"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_bearer_sends_bearer_authorization_header() {
    let (endpoint_url, rx) = spawn_capture_server();

    let config = build_config(&endpoint_url).with_bearer("abc");
    let client = Client::from_conf(config);

    let _ = client
        .list_jobs()
        .org_id("test-org")
        .workspace_id("test-ws")
        .send()
        .await;

    let captured = tokio::task::spawn_blocking(move || {
        rx.recv_timeout(Duration::from_secs(10))
            .ok()
            .and_then(|opt| opt)
    })
    .await
    .expect("join capture thread");

    let auth = captured.expect("capture server saw no Authorization header for with_bearer");

    assert_eq!(
        auth, "Bearer abc",
        "with_bearer must produce a transport-level `Authorization: Bearer <token>` header"
    );
}
