use super::*;
use axum::routing::get;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn configuration_is_validated_without_panicking() {
    assert!(Server::new(ServerConfig::default()).is_ok());
    for max_connections in [0, usize::MAX] {
        assert!(matches!(
            Server::new(ServerConfig {
                max_connections,
                ..ServerConfig::default()
            }),
            Err(ConfigError::Connections)
        ));
    }
    for header_timeout in [Duration::ZERO, Duration::MAX] {
        assert!(matches!(
            Server::new(ServerConfig {
                header_timeout,
                ..ServerConfig::default()
            }),
            Err(ConfigError::HeaderTimeout)
        ));
    }
    assert!(matches!(
        Server::new(ServerConfig {
            drain_timeout: Duration::MAX,
            ..ServerConfig::default()
        }),
        Err(ConfigError::DrainTimeout)
    ));
    assert!(
        Server::new(ServerConfig {
            drain_timeout: Duration::ZERO,
            ..ServerConfig::default()
        })
        .is_ok()
    );
    for max_headers in [0, usize::MAX] {
        assert!(matches!(
            Server::new(ServerConfig {
                max_headers,
                ..ServerConfig::default()
            }),
            Err(ConfigError::Headers)
        ));
    }
    for max_header_buffer_bytes in [0, 8191, usize::MAX] {
        assert!(matches!(
            Server::new(ServerConfig {
                max_header_buffer_bytes,
                ..ServerConfig::default()
            }),
            Err(ConfigError::HeaderBuffer)
        ));
    }
}

#[tokio::test(start_paused = true)]
async fn partial_header_expires_on_virtual_time() {
    let (mut client, server) = tokio::io::duplex(1024);
    let (_stop, stop_rx) = watch::channel(false);
    let task = tokio::spawn(connection(
        server,
        Router::new(),
        ServerConfig::default(),
        stop_rx,
    ));
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: unfinished")
        .await
        .unwrap();
    let started = tokio::time::Instant::now();
    let error = task.await.unwrap().unwrap_err();
    assert!(error.is_timeout());
    assert_eq!(started.elapsed(), Duration::from_secs(10));
}

#[tokio::test]
async fn graceful_connection_finishes_active_response_and_closes_keep_alive() {
    let (mut client, server) = tokio::io::duplex(4096);
    let (stop, stop_rx) = watch::channel(false);
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let app = Router::new().route(
        "/",
        get({
            let started = started.clone();
            let release = release.clone();
            move || {
                let started = started.clone();
                let release = release.clone();
                async move {
                    started.notify_one();
                    release.notified().await;
                    "finished"
                }
            }
        }),
    );
    let task = tokio::spawn(connection(server, app, ServerConfig::default(), stop_rx));
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: local\r\n\r\n")
        .await
        .unwrap();
    started.notified().await;
    stop.send(true).unwrap();
    release.notify_one();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200"));
    assert!(response.ends_with(b"finished"));
    assert!(task.await.unwrap().is_ok());
}

struct DropFlag(Arc<AtomicBool>);
impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn drain_deadline_cancels_pending_tasks_and_observes_cleanup() {
    let dropped = Arc::new(AtomicBool::new(false));
    let mut tasks = JoinSet::new();
    let (started, ready) = tokio::sync::oneshot::channel();
    let flag = dropped.clone();
    tasks.spawn(async move {
        let _drop = DropFlag(flag);
        let _ = started.send(());
        std::future::pending::<Result<(), hyper::Error>>().await
    });
    ready.await.unwrap();
    let mut report = ShutdownReport::default();
    let started = tokio::time::Instant::now();
    drain(&mut tasks, &mut report, Duration::from_secs(5)).await;
    assert_eq!(started.elapsed(), Duration::from_secs(5));
    assert_eq!(report.forced_connections, 1);
    assert_eq!(report.failed_connections, 0);
    assert!(tasks.is_empty());
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn tcp_smoke_and_connection_overload() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let app = Router::new().route(
        "/",
        get({
            let started = started.clone();
            let release = release.clone();
            move || {
                let started = started.clone();
                let release = release.clone();
                async move {
                    started.notify_one();
                    release.notified().await;
                    "ok"
                }
            }
        }),
    );
    let (stop, stop_rx) = tokio::sync::oneshot::channel();
    let server = Server::new(ServerConfig {
        max_connections: 1,
        ..ServerConfig::default()
    })
    .unwrap();
    let task = tokio::spawn(server.serve(listener, app, async {
        let _ = stop_rx.await;
    }));
    let mut first = tokio::net::TcpStream::connect(address).await.unwrap();
    first
        .write_all(b"GET / HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    // A handler notification proves the first socket holds the admission slot.
    started.notified().await;
    let mut second = tokio::net::TcpStream::connect(address).await.unwrap();
    let _ = second
        .write_all(b"GET / HTTP/1.1\r\nHost: local\r\n\r\n")
        .await;
    let mut rejected = Vec::new();
    match second.read_to_end(&mut rejected).await {
        Ok(_) => assert!(rejected.is_empty()),
        Err(error) => assert!(matches!(
            error.kind(),
            io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
        )),
    }
    release.notify_one();
    let mut response = Vec::new();
    first.read_to_end(&mut response).await.unwrap();
    assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200"));
    assert!(response.ends_with(b"ok"));
    stop.send(()).unwrap();
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.accepted_connections, 1);
    assert_eq!(report.rejected_connections, 1);
    assert_eq!(report.forced_connections, 0);
    assert_eq!(report.failed_connections, 0);
}

#[tokio::test]
async fn failed_and_panicked_tasks_are_counted() {
    let mut tasks = JoinSet::new();
    tasks.spawn(async {
        panic!("controlled test panic");
        #[allow(unreachable_code)]
        Ok(())
    });
    let mut report = ShutdownReport::default();
    drain(&mut tasks, &mut report, Duration::from_secs(5)).await;
    assert_eq!(report.failed_connections, 1);
    assert_eq!(report.forced_connections, 0);
}
