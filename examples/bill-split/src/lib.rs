//! A single procedure used by direct calls, HTTP and OpenAPI generation.

use loams_web::{ApiError, HttpMethod, Procedure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bill {
    #[schemars(range(min = 0.0))]
    pub total: f64,
    #[schemars(range(min = 0, max = 100))]
    pub tip: u32,
    #[schemars(range(min = 1, max = 50))]
    pub people: u32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Share {
    pub each: f64,
}

pub fn split() -> Procedure<Bill, Share> {
    Procedure::<Bill, Share>::builder("bill.split")
        .http(HttpMethod::Post, "/split")
        .summary("Split a bill including a percentage tip")
        .handler(|_ctx, bill| async move {
            let total = bill.total * f64::from(100 + bill.tip) / 100.0;
            let each = total / f64::from(bill.people);
            if each.is_finite() {
                Ok(Share { each })
            } else {
                Err(ApiError::invalid_argument(
                    "invalid_input",
                    "The bill exceeds the supported range",
                ))
            }
        })
        .build()
        .expect("the split procedure has a valid static definition")
}

pub fn openapi() -> serde_json::Value {
    let procedure = split();
    loams_web::openapi::generate([procedure.descriptor()], "Bill split", "0.1.0")
        .expect("the split descriptor has a valid schema")
}

pub fn router() -> axum::Router {
    let document = openapi();
    loams_web::http::HttpAdapter::new()
        .procedure(split())
        .expect("the split route is unique")
        .router()
        .expect("the HTTP limits and routes are valid")
        .route(
            "/openapi.json",
            axum::routing::get(move || async move { axum::Json(document) }),
        )
}
