//! A single procedure used by direct calls, HTTP and OpenAPI generation.

use loams_web::{ApiError, Ctx, Procedure, WireType};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize, WireType)]
#[serde(deny_unknown_fields)]
pub struct Bill {
    #[wire(min = 0.0)]
    pub total: f64,
    #[wire(min = 0, max = 100)]
    pub tip: u32,
    #[wire(min = 1, max = 50)]
    pub people: u32,
}

#[derive(Debug, Serialize, Deserialize, WireType)]
pub struct Share {
    pub each: f64,
}

#[loams_web::procedure(name = "bill.split", method = "POST", path = "/split")]
async fn split_handler(_ctx: Ctx, bill: Bill) -> Result<Share, ApiError> {
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
}

pub fn split() -> Procedure<Bill, Share> {
    split_handler_procedure_builder()
        .summary("Split a bill including a percentage tip")
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
    let procedure = Arc::new(split());
    let rpc = loams_web::rpc::BillSplitService::new(
        procedure.clone(),
        |message| {
            Ok(Bill {
                total: message.total,
                tip: message.tip,
                people: message.people,
            })
        },
        |share| {
            Ok(
                loams_web::rpc::proto::loams::web::bills::v1::SplitResponse {
                    each: share.each,
                    ..Default::default()
                },
            )
        },
    )
    .into_router();
    loams_web::http::HttpAdapter::new()
        .shared_procedure(procedure)
        .expect("the split route is unique")
        .router()
        .expect("the HTTP limits and routes are valid")
        .route(
            "/openapi.json",
            axum::routing::get(move || async move { axum::Json(document) }),
        )
        .fallback_service(loams_web::rpc::service(rpc))
}
