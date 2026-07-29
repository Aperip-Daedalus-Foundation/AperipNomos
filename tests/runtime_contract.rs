use std::sync::Arc;
use std::time::Duration;

use aperip_nomos::server::serve_until;
use axum::{Router, routing::get};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Notify,
    sync::oneshot,
};

async fn get_body(address: std::net::SocketAddr) -> String {
    let mut stream = TcpStream::connect(address).await.expect("connect");
    stream
        .write_all(b"GET /identity HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("write request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("read response");
    response
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn graceful_shutdown_waits_for_in_flight_request() {
    let public_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let public_address = public_listener.local_addr().unwrap();
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let handler_entered = Arc::clone(&entered);
    let handler_release = Arc::clone(&release);
    let public = Router::new().route(
        "/identity",
        get(move || {
            let entered = Arc::clone(&handler_entered);
            let release = Arc::clone(&handler_release);
            async move {
                entered.notify_one();
                release.notified().await;
                "drained"
            }
        }),
    );
    let admin = Router::new().route("/identity", get(|| async { "admin" }));
    let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();
    let mut server = tokio::spawn(serve_until(
        public_listener,
        public,
        admin_listener,
        admin,
        async move {
            let _ = shutdown_receiver.await;
        },
    ));
    let request = tokio::spawn(get_body(public_address));

    entered.notified().await;
    shutdown_sender.send(()).expect("send shutdown");
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut server)
            .await
            .is_err(),
        "server exited before the in-flight handler drained"
    );
    release.notify_one();
    assert_eq!(request.await.expect("request task"), "drained");
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("servers stop after drain")
        .expect("join server")
        .expect("server result");
}

#[tokio::test]
async fn serves_distinct_routers_and_stops_both_together() {
    let public_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let public_address = public_listener.local_addr().unwrap();
    let admin_address = admin_listener.local_addr().unwrap();
    let public = Router::new().route("/identity", get(|| async { "public" }));
    let admin = Router::new().route("/identity", get(|| async { "admin" }));
    let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();

    let server = tokio::spawn(serve_until(
        public_listener,
        public,
        admin_listener,
        admin,
        async move {
            let _ = shutdown_receiver.await;
        },
    ));

    assert_eq!(get_body(public_address).await, "public");
    assert_eq!(get_body(admin_address).await, "admin");
    shutdown_sender.send(()).expect("send shutdown");
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("servers stop promptly")
        .expect("join server")
        .expect("server result");
}
