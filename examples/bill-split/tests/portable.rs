//! Shared portable cases are checked through both invocation paths.

use http::{Request, StatusCode};
use http_body_util::BodyExt;
use loams_web::Ctx;
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn portable_split_cases() {
    let corpus: Value =
        serde_json::from_str(include_str!("../../../conformance/cases/portable.json")).unwrap();
    let mut count = 0;
    for case in corpus["cases"].as_array().unwrap() {
        if case["kind"] != "split" {
            continue;
        }
        count += 1;
        let id = case["id"].as_str().unwrap();
        let input = case["input"].clone();
        let result = bill_split::split()
            .invoke_json(Ctx::new(id), input.clone())
            .await;
        let response = bill_split::router()
            .oneshot(
                Request::post("/split")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        if let Some(expected) = case["expected"]["each"].as_f64() {
            let output = result.unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(output["each"].as_f64().unwrap(), expected, "{id}");
            assert_eq!(response.status(), StatusCode::OK, "{id}");
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap(),
                output,
                "{id}"
            );
        } else {
            assert!(
                case["expected"]["invalid_fields"].is_array(),
                "{id}: missing expected outcome"
            );
            let error = result.expect_err(id);
            assert_eq!(error.code, loams_web::RpcCode::InvalidArgument, "{id}");
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{id}");
        }
    }
    assert!(
        count >= 6,
        "fixture selection must keep the representative split corpus"
    );
}
