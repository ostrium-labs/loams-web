use loams_web_core::{ApiError, Ctx, Dependency, HttpMethod, Procedure, RpcCode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Bill {
    #[schemars(range(min = 0.0))]
    total: f64,
    #[schemars(range(min = 1, max = 50))]
    people: u32,
}

fn split() -> Procedure<Bill, f64> {
    Procedure::builder("bill.split")
        .http(HttpMethod::Post, "/split")
        .handler(|_, bill: Bill| async move { Ok(bill.total / bill.people as f64) })
        .build()
        .unwrap()
}

#[tokio::test]
async fn schema_bounds_apply_to_json_and_typed_invocations() {
    let procedure = split();
    assert_eq!(
        procedure
            .invoke_json(Ctx::new("ok"), json!({"total":30,"people":3}))
            .await
            .unwrap(),
        json!(10.0)
    );
    assert_eq!(
        procedure.descriptor().input_schema["properties"]["people"]["maximum"],
        50
    );
    for input in [
        json!({"total":1,"people":0}),
        json!({"total":-1,"people":1}),
        json!({"total":1,"people":51}),
        json!({"total":1,"people":1,"unknown":"secret"}),
    ] {
        let error = procedure
            .invoke_json(Ctx::new("request"), input)
            .await
            .unwrap_err();
        assert_eq!(error.code, RpcCode::InvalidArgument);
        assert_eq!(error.request_id.as_deref(), Some("request"));
        assert!(!serde_json::to_string(&error).unwrap().contains("secret"));
    }
    for total in [-1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            procedure
                .invoke(Ctx::new("typed"), Bill { total, people: 1 })
                .await
                .unwrap_err()
                .code,
            RpcCode::InvalidArgument
        );
    }
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct Renamed {
    #[serde(rename(serialize = "outgoing", deserialize = "incoming"))]
    value: String,
    #[serde(default, skip_serializing)]
    optional: bool,
}

#[tokio::test]
async fn input_and_output_preserve_directional_serde_shapes() {
    let procedure = Procedure::<Renamed, Renamed>::builder("rename")
        .handler(|_, value| async { Ok(value) })
        .build()
        .unwrap();
    assert!(
        procedure.descriptor().input_schema["properties"]
            .get("incoming")
            .is_some()
    );
    assert!(
        procedure.descriptor().output_schema["properties"]
            .get("outgoing")
            .is_some()
    );
    assert_eq!(
        procedure
            .invoke_json(Ctx::new("json"), json!({"incoming":"hello"}))
            .await
            .unwrap(),
        json!({"outgoing":"hello"})
    );
    let typed = procedure
        .invoke(
            Ctx::new("typed"),
            Renamed {
                value: "hello".into(),
                optional: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(typed.value, "hello");
    assert!(!typed.optional);
}

#[derive(Serialize, JsonSchema)]
struct Output {
    #[schemars(range(min = 1))]
    count: u32,
}
#[tokio::test]
async fn output_contract_failures_use_safe_internal_error() {
    let procedure = Procedure::<(), Output>::builder("bad.output")
        .handler(|_, ()| async { Ok(Output { count: 0 }) })
        .build()
        .unwrap();
    let error = procedure
        .invoke_json(Ctx::new("safe"), json!(null))
        .await
        .unwrap_err();
    assert_eq!(error, ApiError::internal().with_request_id("safe"));
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct Node {
    #[schemars(range(min = 1))]
    value: u32,
    next: Option<Box<Node>>,
}

#[tokio::test]
async fn nested_recursive_bounds_and_nullable_fields_validate() {
    let procedure = Procedure::<Node, Node>::builder("recursive")
        .handler(|_, node| async { Ok(node) })
        .build()
        .unwrap();
    let valid = json!({"value":1,"next":{"value":2,"next":null}});
    assert_eq!(
        procedure
            .invoke_json(Ctx::new("ok"), valid.clone())
            .await
            .unwrap(),
        valid
    );
    let invalid = json!({"value":1,"next":{"value":0,"next":null}});
    assert_eq!(
        procedure
            .invoke_json(Ctx::new("bad"), invalid)
            .await
            .unwrap_err()
            .code,
        RpcCode::InvalidArgument
    );
}

#[derive(Serialize, Deserialize)]
struct External;
impl JsonSchema for External {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "External".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"$ref":"https://example.invalid/never-fetch.json"})
    }
}

#[test]
fn external_schema_references_are_rejected_during_build() {
    assert!(
        Procedure::<External, ()>::builder("external")
            .handler(|_, _| async { Ok(()) })
            .build()
            .is_err()
    );
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SplitBill {
    #[schemars(range(min = 0.0))]
    total: f64,
    #[schemars(range(min = 0, max = 100))]
    tip: u32,
    #[schemars(range(min = 1, max = 50))]
    people: u32,
}

#[tokio::test]
async fn all_invalid_bill_fields_are_reported_in_pointer_order_for_json_and_typed_calls() {
    let procedure = Procedure::<SplitBill, ()>::builder("split.problems")
        .handler(|_, _| async { panic!("Invalid input must not invoke handler") })
        .build()
        .unwrap();
    let json = procedure
        .invoke_json(Ctx::new("json"), json!({"total":-1,"tip":101,"people":0}))
        .await
        .unwrap_err();
    let typed = procedure
        .invoke(
            Ctx::new("typed"),
            SplitBill {
                total: -1.0,
                tip: 101,
                people: 0,
            },
        )
        .await
        .unwrap_err();
    let expected = json!({"problems":[
        {"at":"/people","code":"minimum","message":"Must be at least the minimum."},
        {"at":"/tip","code":"maximum","message":"Must be at most the maximum."},
        {"at":"/total","code":"minimum","message":"Must be at least the minimum."}
    ],"truncated":false});
    assert_eq!(json.details, expected);
    assert_eq!(typed.details, expected);
    assert_eq!(json.reason, "invalid_input");
    assert_eq!(json.request_id.as_deref(), Some("json"));
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Address {
    #[serde(rename = "street/name~")]
    street: String,
    #[schemars(range(min = 1))]
    number: u32,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Addresses {
    address: Address,
    others: Vec<Address>,
    attributes: std::collections::BTreeMap<String, u32>,
}

#[tokio::test]
async fn nested_required_type_bounds_and_escaped_pointer_fields_are_safe() {
    let procedure = Procedure::<Addresses, ()>::builder("nested.problems")
        .handler(|_, _| async { Ok(()) })
        .build()
        .unwrap();
    let error = procedure
        .invoke_json(
            Ctx::new("nested"),
            json!({
                "address":{"number":0},
                "others":[{"street/name~":123,"number":"secret-rejected-number"}],
                "attributes":{"secret-dynamic-key":"secret-rejected-value"},
                "secret-unknown-key":"secret-unknown-value"
            }),
        )
        .await
        .unwrap_err();
    let problems = error.details["problems"].as_array().unwrap();
    let locations = problems
        .iter()
        .map(|problem| {
            (
                problem["at"].as_str().unwrap(),
                problem["code"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        locations,
        vec![
            ("", "unknown_field"),
            ("/address/number", "minimum"),
            ("/address/street~1name~0", "required"),
            ("/attributes", "type"),
            ("/others/0/number", "type"),
            ("/others/0/street~1name~0", "type")
        ]
    );
    let rendered = serde_json::to_string(&error).unwrap();
    assert!(!rendered.contains("secret"));
    assert_eq!(error.details["truncated"], false);
}

#[tokio::test]
async fn invalid_root_type_and_missing_declared_fields_have_fixed_problems() {
    let procedure = split();
    let missing = procedure
        .invoke_json(Ctx::new("missing"), json!({}))
        .await
        .unwrap_err();
    assert_eq!(
        missing.details["problems"],
        json!([
            {"at":"/people","code":"required","message":"A required field is missing."},
            {"at":"/total","code":"required","message":"A required field is missing."}
        ])
    );
    let wrong_type = procedure
        .invoke_json(Ctx::new("type"), json!("secret-body"))
        .await
        .unwrap_err();
    assert_eq!(
        wrong_type.details["problems"],
        json!([{"at":"","code":"type","message":"Value has the wrong type."}])
    );
    assert!(!wrong_type.to_string().contains("secret"));
}

#[tokio::test]
async fn validation_problem_count_is_bounded_and_dynamic_keys_are_deduplicated() {
    let procedure = Procedure::<Vec<Bill>, ()>::builder("many.problems")
        .handler(|_, _| async { Ok(()) })
        .build()
        .unwrap();
    let values = (0..1000)
        .map(|_| json!({"total":-1,"people":0}))
        .collect::<Vec<_>>();
    let error = procedure
        .invoke_json(Ctx::new("many"), json!(values))
        .await
        .unwrap_err();
    assert_eq!(error.details["problems"].as_array().unwrap().len(), 32);
    assert_eq!(error.details["truncated"], true);
    assert!(serde_json::to_string(&error).unwrap().len() < 6000);

    let maps = Procedure::<std::collections::BTreeMap<String, u32>, ()>::builder("map.problems")
        .handler(|_, _| async { Ok(()) })
        .build()
        .unwrap();
    let values = (0..100)
        .map(|i| (format!("secret-key-{i}"), json!("secret-value")))
        .collect::<serde_json::Map<_, _>>();
    let error = maps
        .invoke_json(Ctx::new("map"), Value::Object(values))
        .await
        .unwrap_err();
    assert_eq!(
        error.details["problems"],
        json!([{"at":"","code":"type","message":"Value has the wrong type."}])
    );
    assert!(!serde_json::to_string(&error).unwrap().contains("secret"));
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct RequiredRecursive {
    #[schemars(range(min = 1))]
    value: u32,
    next: Box<RequiredRecursive>,
}

#[tokio::test]
async fn deep_problem_locations_are_capped_and_marked_truncated() {
    let procedure = Procedure::<RequiredRecursive, ()>::builder("deep.problems")
        .handler(|_, _| async { Ok(()) })
        .build()
        .unwrap();
    let mut value = json!(null);
    for _ in 0..24 {
        value = json!({"value":0,"next":value});
    }
    let error = procedure
        .invoke_json(Ctx::new("deep"), value)
        .await
        .unwrap_err();
    assert_eq!(error.details["truncated"], true);
    for problem in error.details["problems"].as_array().unwrap() {
        let at = problem["at"].as_str().unwrap();
        assert!(at.len() <= loams_web_core::MAX_VALIDATION_POINTER_BYTES);
        assert!(at.matches('/').count() <= loams_web_core::MAX_VALIDATION_POINTER_DEPTH);
    }
}

#[derive(Serialize, Deserialize)]
struct LongDeclaredField(Value);
impl JsonSchema for LongDeclaredField {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "LongDeclaredField".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let name = "x".repeat(300);
        json!({"type":"object","properties":{name.clone():{"type":"integer"}},"required":[name]})
            .try_into()
            .unwrap()
    }
}

#[tokio::test]
async fn long_declared_field_names_do_not_amplify_output_pointers() {
    let procedure = Procedure::<LongDeclaredField, ()>::builder("long.problem")
        .handler(|_, _| async { Ok(()) })
        .build()
        .unwrap();
    let error = procedure
        .invoke_json(Ctx::new("long"), json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.details["problems"][0]["at"], "");
    assert_eq!(error.details["truncated"], true);
}

#[derive(Serialize, JsonSchema)]
struct DecodeFailure {}
impl<'de> Deserialize<'de> for DecodeFailure {
    fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Err(serde::de::Error::custom("secret-decoder-message"))
    }
}

#[tokio::test]
async fn decoder_errors_keep_top_level_message_and_add_safe_root_problem() {
    let procedure = Procedure::<DecodeFailure, ()>::builder("decode.problem")
        .handler(|_, _| async { Ok(()) })
        .build()
        .unwrap();
    let error = procedure
        .invoke_json(Ctx::new("decode"), json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.message, "Input cannot be decoded");
    assert_eq!(
        error.details["problems"],
        json!([{"at":"","code":"decode","message":"Input cannot be decoded."}])
    );
    assert!(!serde_json::to_string(&error).unwrap().contains("secret"));
}

#[tokio::test]
async fn dependencies_cache_concurrent_success_and_refusal_per_request() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let dependency = Dependency::new(move |_| {
        let counter = counter.clone();
        async move {
            tokio::task::yield_now().await;
            Ok(counter.fetch_add(1, Ordering::SeqCst))
        }
    });
    let ctx = Ctx::new("first");
    let cloned = ctx.clone();
    let (first, second) = tokio::join!(dependency.resolve(&ctx), dependency.resolve(&cloned));
    assert!(Arc::ptr_eq(&first.unwrap(), &second.unwrap()));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    dependency.resolve(&Ctx::new("second")).await.unwrap();
    dependency.uncached(&ctx).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);

    let failures = Arc::new(AtomicUsize::new(0));
    let count = failures.clone();
    let refused = Dependency::<()>::new(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        async {
            Err(ApiError::new(
                RpcCode::Unauthenticated,
                "sign_in",
                "Sign in required",
            ))
        }
    });
    assert!(refused.resolve(&ctx).await.is_err());
    assert!(refused.resolve(&ctx).await.is_err());
    assert_eq!(failures.load(Ordering::SeqCst), 1);
}

#[test]
fn definitions_require_stable_names_and_handlers() {
    assert!(Procedure::<(), ()>::builder("missing").build().is_err());
    assert!(
        Procedure::<(), ()>::builder("has spaces")
            .handler(|_, _| async { Ok(()) })
            .build()
            .is_err()
    );
}

#[tokio::test]
async fn undeclared_errors_are_masked_and_declared_errors_preserved() {
    let hidden = Procedure::<(), ()>::builder("hidden")
        .handler(|_, _| async {
            Err(ApiError::new(
                RpcCode::PermissionDenied,
                "secret",
                "Internal secret",
            ))
        })
        .build()
        .unwrap();
    assert_eq!(
        hidden.invoke(Ctx::new("id"), ()).await.unwrap_err(),
        ApiError::internal().with_request_id("id")
    );
    let declared = Procedure::<(), ()>::builder("declared")
        .error(RpcCode::PermissionDenied, "no_access", "Access denied")
        .handler(|_, _| async {
            Err(ApiError::new(
                RpcCode::PermissionDenied,
                "no_access",
                "Access denied",
            ))
        })
        .build()
        .unwrap();
    assert_eq!(
        declared
            .invoke(Ctx::new("id"), ())
            .await
            .unwrap_err()
            .reason,
        "no_access"
    );
    assert!(
        Procedure::<(), ()>::builder("override")
            .error(RpcCode::NotFound, "internal", "Override")
            .handler(|_, _| async { Ok(()) })
            .build()
            .is_err()
    );
}
