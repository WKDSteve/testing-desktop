//! What the tests of the hub share: requests as raw bytes and a session.

#![allow(dead_code)]

use std::time::{Duration, Instant};

use appkit_core::authn::Secret;
use appkit_hub::RunningServer;
use appkit_testkit::wire::{request, send, Reply};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// An open event stream, read as it comes.
pub struct Stream {
    socket: tokio::net::TcpStream,
    pub seen: String,
}

impl Stream {
    pub async fn open(server: &RunningServer, path: &str, token: &str) -> Stream {
        let mut socket = tokio::net::TcpStream::connect(server.addr()).await.unwrap();
        let host = format!("127.0.0.1:{}", server.addr().port());
        let bearer = format!("Bearer {token}");
        let bytes = request(
            "GET",
            path,
            &[("Host", host.as_str()), ("Authorization", bearer.as_str())],
            b"",
        );
        socket.write_all(&bytes).await.unwrap();
        Stream {
            socket,
            seen: String::new(),
        }
    }

    /// Read until the text holds `needle`, for as long as `patience`; the whole text read so far.
    pub async fn until_within(&mut self, needle: &str, patience: Duration) -> String {
        let end = Instant::now() + patience;
        while !self.seen.contains(needle) {
            let left = end.saturating_duration_since(Instant::now());
            let mut buf = [0u8; 4096];
            let n = tokio::time::timeout(left, self.socket.read(&mut buf))
                .await
                .unwrap_or_else(|_| panic!("no {needle:?} in {:?}", self.seen))
                .unwrap();
            assert!(n > 0, "the stream ended before {needle:?}: {:?}", self.seen);
            self.seen.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        self.seen.clone()
    }

    pub async fn until(&mut self, needle: &str) -> String {
        self.until_within(needle, Duration::from_secs(15)).await
    }
}

pub async fn get_with(
    server: &RunningServer,
    path: &str,
    auth: Option<&str>,
    extra: &[(&str, &str)],
) -> Reply {
    let host = format!("127.0.0.1:{}", server.addr().port());
    let bearer = auth.map(|t| format!("Bearer {t}"));
    let mut headers = vec![("Host", host.as_str())];
    if let Some(b) = &bearer {
        headers.push(("Authorization", b.as_str()));
    }
    headers.extend_from_slice(extra);
    send(server.addr(), request("GET", path, &headers, b"")).await
}

pub async fn get(server: &RunningServer, path: &str, auth: Option<&str>) -> Reply {
    get_with(server, path, auth, &[]).await
}

pub async fn post(server: &RunningServer, path: &str, auth: Option<&str>, body: &str) -> Reply {
    let host = format!("127.0.0.1:{}", server.addr().port());
    let origin = format!("http://{host}");
    let len = body.len().to_string();
    let bearer = auth.map(|t| format!("Bearer {t}"));
    let mut headers = vec![
        ("Host", host.as_str()),
        ("Origin", origin.as_str()),
        ("Content-Type", "application/json"),
        ("Content-Length", len.as_str()),
    ];
    if let Some(b) = &bearer {
        headers.push(("Authorization", b.as_str()));
    }
    send(
        server.addr(),
        request("POST", path, &headers, body.as_bytes()),
    )
    .await
}

/// Pair with the control secret and exchange the code: the token of a new session.
pub async fn session(server: &RunningServer, control: &Secret) -> String {
    let code = post(server, "/api/control/pair", Some(&control.to_hex()), "{}").await;
    assert_eq!(code.status, 200, "{code:?}");
    let body = format!(r#"{{"code":"{}"}}"#, code.json()["code"].as_str().unwrap());
    let token = post(server, "/auth/exchange", None, &body).await;
    assert_eq!(token.status, 200, "{token:?}");
    token.json()["token"].as_str().unwrap().to_owned()
}
