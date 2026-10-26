use super::*;
use axum::{Router, body::Body};
use base64::Engine;
use buffa::Message;
use buffa::{HasMessageView, view::OwnedView};
use connectrpc::{
    CodecFormat, Protocol, SpecOrigin,
    client::{
        CallOptions, ClientConfig, ClientTransport, ServiceTransport, UnaryResponse, call_unary,
    },
};
use http_body_util::BodyExt;
use serde::Deserialize;
use std::sync::atomic::AtomicUsize;
use tower::ServiceExt;

#[derive(Serialize, Deserialize, JsonSchema)]
struct Bill {
    #[schemars(range(min = 0.0))]
    total: f64,
    #[schemars(range(min = 0, max = 100))]
    tip: u32,
    #[schemars(range(min = 1, max = 50))]
    people: u32,
}
#[derive(Serialize, JsonSchema)]
struct Share {
    each: f64,
}
type ResponseView = <SplitResponse as HasMessageView>::View<'static>;

fn procedure(calls: Arc<AtomicUsize>) -> Arc<Procedure<Bill, Share>> {
    Arc::new(
        Procedure::<Bill, Share>::builder("bill.split")
            .http(loams_web_core::HttpMethod::Post, "/split")
            .error(RpcCode::FailedPrecondition, "closed", "Bill is closed")
            .handler(move |_, bill| {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    if bill.total == 13.0 {
                        return Err(ApiError::new(
                            RpcCode::FailedPrecondition,
                            "closed",
                            "Bill is closed",
                        )
                        .with_details(serde_json::json!({"closed":true}))
                        .with_retry(100));
                    }
                    if bill.total == 17.0 {
                        return Err(ApiError::new(
                            RpcCode::Internal,
                            "database",
                            "SECRET_DATABASE_PASSWORD",
                        ));
                    }
                    Ok(Share {
                        each: bill.total * (1.0 + f64::from(bill.tip) / 100.0)
                            / f64::from(bill.people),
                    })
                }
            })
            .build()
            .unwrap(),
    )
}
fn rpc(procedure: Arc<Procedure<Bill, Share>>) -> connectrpc::Router {
    BillSplitService::new(
        procedure,
        |request| {
            Ok(Bill {
                total: request.total,
                tip: request.tip,
                people: request.people,
            })
        },
        |share| {
            Ok(SplitResponse {
                each: share.each,
                ..Default::default()
            })
        },
    )
    .into_router()
}
fn request(total: f64, people: u32) -> SplitRequest {
    SplitRequest {
        total,
        tip: 10,
        people,
        ..Default::default()
    }
}
async fn call<T: ClientTransport>(
    transport: T,
    origin: &str,
    protocol: Protocol,
    format: CodecFormat,
    input: SplitRequest,
) -> Result<UnaryResponse<OwnedView<ResponseView>>, ConnectError>
where
    <T::ResponseBody as connectrpc::http_body::Body>::Error: std::fmt::Display,
{
    call_unary(
        &transport,
        &ClientConfig::new(origin.parse().unwrap())
            .with_protocol(protocol)
            .with_codec_format(format),
        bills::BILL_SPLIT_SERVICE_SPLIT_SPEC.with_origin(SpecOrigin::Client),
        input,
        CallOptions::default(),
    )
    .await
}
fn detail(error: &ConnectError) -> ProcedureErrorDetail {
    let raw = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(error.details[0].value.as_ref().unwrap())
        .unwrap();
    ProcedureErrorDetail::decode_from_slice(&raw).unwrap()
}

#[tokio::test]
async fn runtime_transports_share_validation_handler_and_typed_errors() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new().fallback_service(service(rpc(procedure(calls.clone()))));
    for protocol in [Protocol::Connect, Protocol::Grpc, Protocol::GrpcWeb] {
        for format in [CodecFormat::Proto, CodecFormat::Json] {
            let transport = ServiceTransport::new(app.clone());
            let response = call(
                transport.clone(),
                "http://local",
                protocol,
                format,
                request(90.0, 3),
            )
            .await
            .unwrap();
            assert!(response.headers().contains_key("x-request-id"));
            assert!((response.into_owned().each - 33.0).abs() < 1e-12);
            let invalid = call(
                transport.clone(),
                "http://local",
                protocol,
                format,
                request(90.0, 0),
            )
            .await
            .unwrap_err();
            assert_eq!(invalid.code, ErrorCode::InvalidArgument);
            assert_eq!(detail(&invalid).reason, "invalid_input");
            let closed = call(
                transport.clone(),
                "http://local",
                protocol,
                format,
                request(13.0, 1),
            )
            .await
            .unwrap_err();
            assert_eq!(closed.code, ErrorCode::FailedPrecondition);
            assert_eq!(detail(&closed).reason, "closed");
            assert_eq!(detail(&closed).retry_after_millis, Some(100));
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&detail(&closed).details_json).unwrap(),
                serde_json::json!({"closed":true})
            );
            let secret = call(
                transport,
                "http://local",
                protocol,
                format,
                request(17.0, 1),
            )
            .await
            .unwrap_err();
            assert_eq!(secret.code, ErrorCode::Internal);
            assert!(!secret.to_json().windows(6).any(|chunk| chunk == b"SECRET"));
        }
    }
    // Six protocol/codec pairs each invoke happy, declared and undeclared paths;
    // invalid input never reaches the shared handler.
    assert_eq!(calls.load(Ordering::SeqCst), 18);
}

#[tokio::test]
async fn malformed_and_oversize_payloads_are_rejected_by_runtime() {
    let app =
        Router::new().fallback_service(service(rpc(procedure(Arc::new(AtomicUsize::new(0))))));
    for bytes in [vec![0xff, 0xff, 0xff], vec![0; 1024 * 1024 + 1]] {
        let req = http::Request::builder()
            .method("POST")
            .uri("/loams.web.bills.v1.BillSplitService/Split")
            .header("content-type", "application/proto")
            .header("connect-protocol-version", "1")
            .body(Body::from(bytes))
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert!(response.status().is_client_error());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let error: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(matches!(
            error["code"].as_str(),
            Some("invalid_argument" | "resource_exhausted")
        ));
    }
}

#[tokio::test(start_paused = true)]
async fn runtime_deadline_bounds_pending_handler() {
    let p = Arc::new(
        Procedure::<Bill, Share>::builder("pending")
            .handler(|_, _| async { std::future::pending().await })
            .build()
            .unwrap(),
    );
    let runtime = service(rpc(p));
    assert_eq!(
        runtime.deadline_policy().default_timeout(),
        Some(Duration::from_secs(30))
    );
    assert_eq!(
        runtime.deadline_policy().max(),
        Some(Duration::from_secs(30))
    );
    let app = Router::new().fallback_service(runtime);
    let error = call(
        ServiceTransport::new(app),
        "http://local",
        Protocol::Connect,
        CodecFormat::Proto,
        request(90.0, 3),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::DeadlineExceeded);
}

#[test]
fn errors_preserve_canonical_codes_and_bound_public_metadata() {
    assert_eq!(
        project_error(ApiError::new(RpcCode::Cancelled, "cancelled", "Cancelled")).code,
        ErrorCode::Canceled
    );
    assert_eq!(
        project_error(ApiError::new(
            RpcCode::PermissionDenied,
            "closed",
            "x".repeat(5000)
        ))
        .code,
        ErrorCode::Internal
    );
    assert!(!FILE_DESCRIPTOR_SET.is_empty());
}

#[tokio::test]
async fn one_tcp_port_serves_rest_connect_grpc_and_grpc_web() {
    use connectrpc::client::{Http2Connection, HttpClient};
    use loams_web_http::{
        HttpAdapter,
        server::{Server, ServerConfig},
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let calls = Arc::new(AtomicUsize::new(0));
    let procedure = procedure(calls.clone());
    let app = HttpAdapter::new()
        .shared_procedure(procedure.clone())
        .unwrap()
        .router()
        .unwrap()
        .fallback_service(service(rpc(procedure)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = format!("http://{address}");
    let (stop, stop_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(Server::new(ServerConfig::default()).unwrap().serve(
        listener,
        app,
        async {
            let _ = stop_rx.await;
        },
    ));
    for protocol in [Protocol::Connect, Protocol::GrpcWeb] {
        let response = call(
            HttpClient::plaintext(),
            &origin,
            protocol,
            CodecFormat::Proto,
            request(90.0, 3),
        )
        .await
        .unwrap();
        assert!((response.into_owned().each - 33.0).abs() < 1e-12);
    }
    let h2 = Http2Connection::connect_plaintext(origin.parse().unwrap())
        .await
        .unwrap()
        .shared(8);
    let response = call(
        h2.clone(),
        &origin,
        Protocol::Grpc,
        CodecFormat::Proto,
        request(90.0, 3),
    )
    .await
    .unwrap();
    assert_eq!(response.trailers().get("grpc-status").unwrap(), "0");
    assert!((response.into_owned().each - 33.0).abs() < 1e-12);
    let mut rest = tokio::net::TcpStream::connect(address).await.unwrap();
    let body = r#"{"total":90,"tip":10,"people":3}"#;
    rest.write_all(format!("POST /split HTTP/1.1\r\nHost: local\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
    let mut response = String::new();
    rest.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    drop(h2);
    stop.send(()).unwrap();
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.forced_connections, 0);
}
