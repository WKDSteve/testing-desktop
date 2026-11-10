//! The requests of the tests wait for a slow answer: a call of the real engine may take more than
//! five seconds while the other tests start their Neovims (the empty replies of the engine tests).

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

mod common;

/// A server that answers after `delay`, once.
async fn slow_server(delay: Duration) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut head = [0u8; 1024];
        let _ = socket.read(&mut head).await.unwrap();
        tokio::time::sleep(delay).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .await
            .unwrap();
    });
    addr
}

#[tokio::test]
async fn a_request_waits_longer_than_five_seconds_for_its_answer() {
    let addr = slow_server(Duration::from_secs(6)).await;
    let bytes = appkit_testkit::wire::request("GET", "/", &[("Host", "127.0.0.1")], b"");
    let reply = common::send(addr, bytes).await;
    assert_eq!(reply.status, 200, "{reply:?}");
    assert_eq!(reply.json(), serde_json::json!({}));
}

#[test]
fn the_patience_outlasts_the_time_a_request_may_take_in_the_hub() {
    assert!(common::patience() > appkit_hub::Limits::default().request_timeout);
}
