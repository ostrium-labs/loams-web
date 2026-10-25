use super::*;
use loams_web_core::{HttpMethod, Procedure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower::ServiceExt;

#[derive(Serialize, Deserialize, JsonSchema)]
struct Input {
    #[schemars(range(min = 1, max = 50))]
    people: u32,
}
fn procedure() -> Procedure<Input, u32> {
    Procedure::builder("split")
        .http(HttpMethod::Post, "/split")
        .handler(|_, input: Input| async move { Ok(input.people) })
        .build()
        .unwrap()
}
fn request(path: &str, body: &str) -> Request {
    Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}
async fn payload(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap()
}

#[tokio::test]
async fn happy_invalid_and_unroutable_requests() {
    let app = HttpAdapter::new()
        .procedure(procedure())
        .unwrap()
        .router()
        .unwrap();
    let response = app
        .clone()
        .oneshot(request("/split", r#"{"people":2}"#))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("x-request-id"));
    assert_eq!(payload(response).await, json!(2));
    for body in [
        r#"{"people":0}"#,
        r#"{"people":51}"#,
        r#"{"people":"SECRET_TOKEN"}"#,
        "SECRET_TOKEN",
    ] {
        let response = app.clone().oneshot(request("/split", body)).await.unwrap();
        assert!(response.status().is_client_error());
        assert!(!payload(response).await.to_string().contains("SECRET_TOKEN"));
    }
    assert_eq!(
        app.clone()
            .oneshot(request("/missing", "{}"))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let request = Request::builder()
        .uri("/split")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[tokio::test]
async fn content_type_size_and_output_limits() {
    let app = HttpAdapter::new()
        .procedure(procedure())
        .unwrap()
        .limits(HttpLimits {
            max_body_bytes: 20,
            ..HttpLimits::default()
        })
        .router()
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(request("/split", &" ".repeat(21)))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let missing_type = Request::builder()
        .method("POST")
        .uri("/split")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        app.oneshot(missing_type).await.unwrap().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    let app = HttpAdapter::new()
        .procedure(procedure())
        .unwrap()
        .limits(HttpLimits {
            max_response_bytes: 1,
            ..HttpLimits::default()
        })
        .router()
        .unwrap();
    assert_eq!(
        app.oneshot(request("/split", r#"{"people":20}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[tokio::test]
async fn errors_project_status_and_undeclared_errors_are_redacted() {
    let declared = Procedure::<(), ()>::builder("refusal")
        .http(HttpMethod::Post, "/refusal")
        .error(RpcCode::PermissionDenied, "closed", "Closed")
        .handler(|_, ()| async {
            Err(ApiError::new(RpcCode::PermissionDenied, "closed", "Closed"))
        })
        .build()
        .unwrap();
    let secret = Procedure::<(), ()>::builder("secret")
        .http(HttpMethod::Post, "/secret")
        .handler(|_, ()| async {
            Err(ApiError::new(
                RpcCode::Internal,
                "database",
                "SECRET_PASSWORD",
            ))
        })
        .build()
        .unwrap();
    let app = HttpAdapter::new()
        .procedure(declared)
        .unwrap()
        .procedure(secret)
        .unwrap()
        .router()
        .unwrap();
    let response = app
        .clone()
        .oneshot(request("/refusal", "null"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(payload(response).await["reason"], "closed");
    let response = app.oneshot(request("/secret", "null")).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        !payload(response)
            .await
            .to_string()
            .contains("SECRET_PASSWORD")
    );
}

#[test]
fn invalid_definitions_fail_as_values() {
    assert!(matches!(
        HttpAdapter::new()
            .procedure(procedure())
            .unwrap()
            .procedure(procedure()),
        Err(BuildError::DuplicateName(_))
    ));
    let other = Procedure::<(), ()>::builder("other")
        .http(HttpMethod::Post, "/split")
        .handler(|_, ()| async { Ok(()) })
        .build()
        .unwrap();
    assert!(matches!(
        HttpAdapter::new()
            .procedure(procedure())
            .unwrap()
            .procedure(other),
        Err(BuildError::DuplicateRoute(_))
    ));
    let dynamic = Procedure::<(), ()>::builder("dynamic")
        .http(HttpMethod::Post, "/users/{id}")
        .handler(|_, ()| async { Ok(()) })
        .build()
        .unwrap();
    assert!(matches!(
        HttpAdapter::new().procedure(dynamic),
        Err(BuildError::InvalidPath(_))
    ));
    let get = Procedure::<Input, ()>::builder("get")
        .http(HttpMethod::Get, "/get")
        .handler(|_, _| async { Ok(()) })
        .build()
        .unwrap();
    assert!(matches!(
        HttpAdapter::new().procedure(get),
        Err(BuildError::InvalidInputBinding)
    ));
    assert!(matches!(
        HttpAdapter::new()
            .limits(HttpLimits {
                max_in_flight: 0,
                ..HttpLimits::default()
            })
            .router(),
        Err(BuildError::InvalidLimits)
    ));
}

#[tokio::test(start_paused = true)]
async fn deadline_covers_pending_handler() {
    let p = Procedure::<(), ()>::builder("pending")
        .http(HttpMethod::Post, "/pending")
        .handler(|_, ()| async { std::future::pending().await })
        .build()
        .unwrap();
    let app = HttpAdapter::new().procedure(p).unwrap().router().unwrap();
    let response = app.oneshot(request("/pending", "null")).await.unwrap();
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(payload(response).await["code"], "deadline_exceeded");
}

#[tokio::test]
async fn overload_rejects_without_waiting_and_releases_capacity() {
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let p = Procedure::<(), ()>::builder("held")
        .http(HttpMethod::Post, "/held")
        .handler({
            let started = started.clone();
            let release = release.clone();
            move |_, ()| {
                let started = started.clone();
                let release = release.clone();
                async move {
                    started.notify_one();
                    release.notified().await;
                    Ok(())
                }
            }
        })
        .build()
        .unwrap();
    let app = HttpAdapter::new()
        .procedure(p)
        .unwrap()
        .limits(HttpLimits {
            max_in_flight: 1,
            ..HttpLimits::default()
        })
        .router()
        .unwrap();
    let first = tokio::spawn(app.clone().oneshot(request("/held", "null")));
    started.notified().await;
    assert_eq!(
        app.clone()
            .oneshot(request("/held", "null"))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    release.notify_one();
    assert_eq!(first.await.unwrap().unwrap().status(), StatusCode::OK);
    release.notify_one();
    assert_eq!(
        app.oneshot(request("/held", "null"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn unit_get_head_and_body_contract() {
    let p = Procedure::<(), u32>::builder("get")
        .http(HttpMethod::Get, "/get")
        .handler(|_, ()| async { Ok(42) })
        .build()
        .unwrap();
    let app = HttpAdapter::new().procedure(p).unwrap().router().unwrap();
    let get = Request::builder().uri("/get").body(Body::empty()).unwrap();
    assert_eq!(
        payload(app.clone().oneshot(get).await.unwrap()).await,
        json!(42)
    );
    let head = Request::builder()
        .method("HEAD")
        .uri("/get")
        .body(Body::empty())
        .unwrap();
    assert!(
        to_bytes(app.clone().oneshot(head).await.unwrap().into_body(), 100)
            .await
            .unwrap()
            .is_empty()
    );
    let body = Request::builder()
        .uri("/get")
        .body(Body::from("null"))
        .unwrap();
    assert_eq!(
        app.oneshot(body).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn declared_error_payloads_are_bounded() {
    let p = Procedure::<(), ()>::builder("large")
        .http(HttpMethod::Post, "/large")
        .error(RpcCode::PermissionDenied, "large", "Large refusal")
        .handler(|_, ()| async {
            Err(ApiError::new(
                RpcCode::PermissionDenied,
                "large",
                "x".repeat(4096),
            ))
        })
        .build()
        .unwrap();
    let app = HttpAdapter::new()
        .procedure(p)
        .unwrap()
        .limits(HttpLimits {
            max_response_bytes: 256,
            ..HttpLimits::default()
        })
        .router()
        .unwrap();
    let response = app.oneshot(request("/large", "null")).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(payload(response).await["reason"], "response_limit");
}
