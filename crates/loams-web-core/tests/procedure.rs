use loams_web_core::{ApiError, Ctx, Dependency, HttpMethod, Procedure, RpcCode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
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
