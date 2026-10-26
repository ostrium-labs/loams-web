//! HTTP/1 and HTTP/2 server policy with bounded admission and shutdown.
//!
//! Hyper owns framing and header parsing. This helper does not yet implement the
//! reference server's minimum body rate, idle/read/write progress timeouts, TLS,
//! dual-family binding, or HTTP/2 per-header progress deadlines. Supply an
//! already-bound listener explicitly.

use std::{
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use axum::Router;
use hyper_util::{
    rt::{TokioExecutor, TokioIo, TokioTimer},
    server::conn::auto,
    service::TowerToHyperService,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpListener,
    sync::{Semaphore, watch},
    task::JoinSet,
};

/// Connection-level policy, independent of the adapter's per-request limits.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Maximum admitted TCP connections; excess accepted sockets are closed.
    pub max_connections: usize,
    /// Hyper's deadline for reading each HTTP/1 request header.
    pub header_timeout: Duration,
    /// Maximum time to finish admitted connections after shutdown is requested.
    /// Zero skips draining and cancels every remaining connection immediately.
    pub drain_timeout: Duration,
    /// Maximum number of header fields parsed by hyper per request.
    pub max_headers: usize,
    /// Hyper's HTTP/1 read-buffer bound; this is not an exact header-byte limit.
    /// Must lie between 8 KiB and 64 MiB, inclusive.
    pub max_header_buffer_bytes: usize,
    /// Maximum concurrent streams on each HTTP/2 connection, from 1 to 65535.
    pub max_http2_streams: u32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_connections: 512,
            header_timeout: Duration::from_secs(10),
            drain_timeout: Duration::from_secs(5),
            max_headers: 100,
            max_header_buffer_bytes: 16 * 1024,
            max_http2_streams: 128,
        }
    }
}

/// Invalid configuration is rejected before starting any connection task.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("max_connections must be between 1 and the semaphore capacity")]
    Connections,
    #[error("header_timeout must be greater than zero and at most one day")]
    HeaderTimeout,
    #[error("drain_timeout must be at most one day")]
    DrainTimeout,
    #[error("max_headers must be between 1 and 65536")]
    Headers,
    #[error("max_header_buffer_bytes must be between 8192 and 67108864")]
    HeaderBuffer,
    #[error("max_http2_streams must be between 1 and 65535")]
    Http2Streams,
}

/// Connection accounting for one run, without request data or error messages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShutdownReport {
    /// Connections admitted and assigned a connection task.
    pub accepted_connections: u64,
    /// Accepted sockets closed immediately because admission was exhausted.
    pub rejected_connections: u64,
    /// Admitted connections ending in a hyper error or task panic.
    pub failed_connections: u64,
    /// Connection tasks cancelled when the drain deadline expired.
    /// These are counted separately from failed connections.
    pub forced_connections: u64,
}

/// A validated HTTP server configuration. Libraries install no global hooks.
pub struct Server {
    config: ServerConfig,
}

impl Server {
    pub fn new(config: ServerConfig) -> Result<Self, ConfigError> {
        if config.max_connections == 0 || config.max_connections > Semaphore::MAX_PERMITS {
            return Err(ConfigError::Connections);
        }
        if config.header_timeout.is_zero() || config.header_timeout > Duration::from_secs(86400) {
            return Err(ConfigError::HeaderTimeout);
        }
        if config.drain_timeout > Duration::from_secs(86400) {
            return Err(ConfigError::DrainTimeout);
        }
        if config.max_headers == 0 || config.max_headers > 65536 {
            return Err(ConfigError::Headers);
        }
        if !(8192..=64 * 1024 * 1024).contains(&config.max_header_buffer_bytes) {
            return Err(ConfigError::HeaderBuffer);
        }
        if !(1..=65535).contains(&config.max_http2_streams) {
            return Err(ConfigError::Http2Streams);
        }
        Ok(Self { config })
    }

    /// Serve until `shutdown` completes or accepting a connection fails.
    ///
    /// Stop accepting first, ask each connection to finish its current request,
    /// then cancel tasks still pending at the drain deadline. Listener errors
    /// follow the same drain policy before being returned to the caller. A
    /// caller dropping this future aborts connection tasks without a drain.
    pub async fn serve(
        self,
        listener: TcpListener,
        router: Router,
        shutdown: impl Future<Output = ()>,
    ) -> io::Result<ShutdownReport> {
        let admission = std::sync::Arc::new(Semaphore::new(self.config.max_connections));
        let (stop, stop_rx) = watch::channel(false);
        let mut connections = JoinSet::new();
        let mut report = ShutdownReport::default();
        tokio::pin!(shutdown);
        let accept_result = loop {
            tokio::select! {
                biased;
                () = &mut shutdown => break Ok(()),
                finished = connections.join_next(), if !connections.is_empty() => {
                    record_result(finished.expect("nonempty task set"), &mut report);
                }
                accepted = listener.accept() => {
                    let (socket, _) = match accepted {
                        Ok(connection) => connection,
                        Err(error) => break Err(error),
                    };
                    let Ok(permit) = admission.clone().try_acquire_owned() else {
                        report.rejected_connections += 1;
                        drop(socket);
                        continue;
                    };
                    report.accepted_connections += 1;
                    let config = self.config.clone();
                    let router = router.clone();
                    let stop_rx = stop_rx.clone();
                    connections.spawn(async move {
                        let _permit = permit;
                        connection(socket, router, config, stop_rx).await
                    });
                }
            }
        };
        // Drop the listening socket before any drain wait.
        drop(listener);
        let _ = stop.send(true);
        drain(&mut connections, &mut report, self.config.drain_timeout).await;
        accept_result.map(|()| report)
    }
}

type ConnectionResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

async fn connection<I>(
    io: I,
    router: Router,
    config: ServerConfig,
    mut stop: watch::Receiver<bool>,
) -> ConnectionResult
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = auto::Builder::new(TokioExecutor::new());
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(config.header_timeout)
        .max_headers(config.max_headers)
        .max_buf_size(config.max_header_buffer_bytes);
    builder
        .http2()
        .timer(TokioTimer::new())
        .max_concurrent_streams(config.max_http2_streams)
        .max_header_list_size(config.max_header_buffer_bytes as u32)
        .max_send_buf_size(1024 * 1024);
    let io = DetectionIo::new(io, config.header_timeout);
    let connection = builder.serve_connection(TokioIo::new(io), TowerToHyperService::new(router));
    tokio::pin!(connection);
    // A just-spawned connection may first run after the stop notification.
    if *stop.borrow() {
        connection.as_mut().graceful_shutdown();
        return connection.await;
    }
    tokio::select! {
        result = &mut connection => result,
        _ = stop.changed() => {
            connection.as_mut().graceful_shutdown();
            connection.await
        }
    }
}

fn record_result(
    result: Result<ConnectionResult, tokio::task::JoinError>,
    report: &mut ShutdownReport,
) {
    if !matches!(result, Ok(Ok(()))) {
        report.failed_connections += 1;
    }
}

async fn drain(
    connections: &mut JoinSet<ConnectionResult>,
    report: &mut ShutdownReport,
    deadline: Duration,
) {
    let finished = tokio::time::timeout(deadline, async {
        while let Some(result) = connections.join_next().await {
            record_result(result, report);
        }
    })
    .await;
    if finished.is_err() {
        connections.abort_all();
        // Observe task destruction and release sockets/permits before returning.
        while let Some(result) = connections.join_next().await {
            if matches!(&result, Err(error) if error.is_cancelled()) {
                report.forced_connections += 1;
            } else {
                record_result(result, report);
            }
        }
    }
}

// The auto builder reads a possible HTTP/2 preface before starting HTTP/1's
// header timer. Bound that negotiation too, including clients sending nothing.
struct DetectionIo<I> {
    inner: I,
    timeout: Pin<Box<tokio::time::Sleep>>,
    matched: usize,
    detected: bool,
}
impl<I> DetectionIo<I> {
    fn new(inner: I, timeout: Duration) -> Self {
        Self {
            inner,
            timeout: Box::pin(tokio::time::sleep(timeout)),
            matched: 0,
            detected: false,
        }
    }
}
impl<I: AsyncRead + Unpin> AsyncRead for DetectionIo<I> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.detected && self.timeout.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "HTTP protocol detection deadline exceeded",
            )));
        }
        let before = buffer.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buffer);
        if matches!(&result, Poll::Ready(Ok(()))) && !self.detected {
            const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
            for byte in &buffer.filled()[before..] {
                if *byte != PREFACE[self.matched] {
                    self.detected = true;
                    break;
                }
                self.matched += 1;
                if self.matched == PREFACE.len() {
                    self.detected = true;
                    break;
                }
            }
        }
        result
    }
}
impl<I: AsyncWrite + Unpin> AsyncWrite for DetectionIo<I> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, bytes)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests;
