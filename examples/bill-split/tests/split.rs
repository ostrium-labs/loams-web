use http::{Request, StatusCode};
use http_body_util::BodyExt;
use loams_web::Ctx;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn same_procedure_serves_in_process_and_http() {
    let direct = bill_split::split()
        .invoke(
            Ctx::new("direct"),
            bill_split::Bill {
                total: 90.0,
                tip: 10,
                people: 3,
            },
        )
        .await
        .unwrap();
    let response = bill_split::router()
        .oneshot(
            Request::post("/split")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"total":90,"tip":10,"people":3}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let wire: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(wire["each"].as_f64().unwrap(), direct.each);
    assert_eq!(direct.each, 33.0);
}

#[tokio::test]
async fn rejects_unknown_missing_and_out_of_range_input() {
    for input in [
        json!({"total":90,"tip":10,"people":0}),
        json!({"total":-1,"tip":10,"people":3}),
        json!({"total":90,"tip":101,"people":3}),
        json!({"total":90,"tip":10,"people":51}),
        json!({"total":90,"tip":10}),
        json!({"total":90,"tip":10,"people":3,"secret":"sensitive-canary"}),
    ] {
        let response = bill_split::router()
            .oneshot(
                Request::post("/split")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{input}");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(!String::from_utf8_lossy(&bytes).contains("sensitive-canary"));
    }
}

#[tokio::test]
async fn documents_exact_bounds_and_serves_same_document() {
    let expected = bill_split::openapi();
    assert_eq!(expected["openapi"], "3.2.0");
    let response = bill_split::router()
        .oneshot(
            Request::get("/openapi.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), expected);
    let schema = &bill_split::split().descriptor().input_schema.clone();
    assert_eq!(schema["properties"]["people"]["minimum"], 1);
    assert_eq!(schema["properties"]["people"]["maximum"], 50);
}
