extern crate loams_web_core as loams_web;

use loams_web::{ApiError, Ctx};
use loams_web_derive::{WireType, procedure};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, Deserialize, WireType)]
struct Bill {
    #[wire(min = 0.0)]
    total: f64,
    #[wire(min = 0, max = 100)]
    tip: u32,
    #[wire(min = 1, max = 50)]
    people: u32,
}
#[derive(Serialize, Deserialize, WireType)]
struct Share {
    per_person: f64,
}

#[procedure(name = "bill.split", method = "POST", path = "/split")]
async fn split(_ctx: Ctx, bill: Bill) -> Result<Share, ApiError> {
    Ok(Share {
        per_person: bill.total * (1.0 + bill.tip as f64 / 100.0) / bill.people as f64,
    })
}

#[tokio::test]
async fn generated_factory_invokes_same_original_handler_and_validates_schema_bounds() {
    fn requires_wire<T: loams_web::WireType>() {}
    requires_wire::<Bill>();
    let procedure = split_procedure().unwrap();
    assert_eq!(procedure.descriptor().name, "bill.split");
    assert_eq!(procedure.descriptor().http.as_ref().unwrap().path, "/split");
    assert_eq!(
        procedure.descriptor().input_schema["properties"]["people"]["minimum"],
        1
    );
    let output = procedure
        .invoke_json(
            Ctx::new("schema"),
            json!({"total":60.0,"tip":10,"people":3}),
        )
        .await
        .unwrap();
    assert_eq!(output, json!({"per_person":22.0}));
    let output = split(
        Ctx::new("direct"),
        Bill {
            total: 60.0,
            tip: 10,
            people: 3,
        },
    )
    .await
    .unwrap();
    assert_eq!(output.per_person, 22.0);
    let error = procedure
        .invoke_json(Ctx::new("bad"), json!({"total":-1,"tip":101,"people":0}))
        .await
        .unwrap_err();
    assert_eq!(error.details["problems"].as_array().unwrap().len(), 3);
}

#[derive(Serialize, Deserialize, WireType)]
struct Shapes<T> {
    #[serde(rename(serialize = "sent", deserialize = "received"))]
    value: T,
    #[serde(default)]
    optional: Option<String>,
    #[serde(skip)]
    ignored: bool,
    #[wire(min_length = 1, max_length = 4)]
    names: Vec<String>,
}

#[tokio::test]
async fn generic_directional_serde_shapes_defaults_and_lengths_are_preserved() {
    let procedure = loams_web::Procedure::<Shapes<u32>, Shapes<u32>>::builder("shapes")
        .handler(|_, shape| async { Ok(shape) })
        .build()
        .unwrap();
    assert!(
        procedure.descriptor().input_schema["properties"]
            .get("received")
            .is_some()
    );
    assert!(
        procedure.descriptor().output_schema["properties"]
            .get("sent")
            .is_some()
    );
    assert!(
        procedure.descriptor().output_schema["properties"]
            .get("ignored")
            .is_none()
    );
    let value = procedure
        .invoke_json(Ctx::new("ok"), json!({"received":7,"names":["test"]}))
        .await
        .unwrap();
    assert_eq!(value, json!({"sent":7,"optional":null,"names":["test"]}));
    assert!(
        procedure
            .invoke_json(Ctx::new("bad"), json!({"received":7,"names":[]}))
            .await
            .is_err()
    );
    let typed = Shapes::<u32> {
        value: 7,
        optional: None,
        ignored: false,
        names: vec!["ok".into()],
    };
    assert!(!typed.ignored);
}

#[derive(Serialize, Deserialize, WireType)]
#[serde(default)]
struct Configuration {
    level: u32,
    label: String,
}
impl Default for Configuration {
    fn default() -> Self {
        Self {
            level: 7,
            label: "default label".into(),
        }
    }
}

#[tokio::test]
async fn container_default_uses_original_custom_default_values() {
    let procedure = loams_web::Procedure::<Configuration, Configuration>::builder("defaults")
        .handler(|_, value| async { Ok(value) })
        .build()
        .unwrap();
    let output = procedure
        .invoke_json(Ctx::new("default"), json!({}))
        .await
        .unwrap();
    assert_eq!(output, json!({"level":7,"label":"default label"}));
    assert_eq!(
        procedure.descriptor().input_schema["properties"]["level"]["default"],
        7
    );
}

#[derive(Serialize, Deserialize, WireType)]
struct Tree {
    #[wire(min = 1)]
    value: u32,
    next: Option<Box<Self>>,
}
#[derive(Serialize, Deserialize, WireType)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Command {
    Update {
        tree: Tree,
    },
    Remove {
        #[wire(min_length = 1)]
        name: String,
    },
}

#[tokio::test]
async fn recursive_self_types_and_tagged_enums_build_and_validate() {
    let procedure = loams_web::Procedure::<Command, Command>::builder("commands")
        .handler(|_, value| async { Ok(value) })
        .build()
        .unwrap();
    let input = json!({"kind":"update","tree":{"value":1,"next":{"value":2,"next":null}}});
    assert_eq!(
        procedure
            .invoke_json(Ctx::new("tree"), input.clone())
            .await
            .unwrap(),
        input
    );
    assert!(
        procedure
            .invoke_json(Ctx::new("bad"), json!({"kind":"remove","name":""}))
            .await
            .is_err()
    );
}

#[derive(Serialize, Deserialize, WireType)]
#[serde(transparent)]
struct Maybe(Option<u32>);
#[derive(Serialize, Deserialize, WireType)]
struct Details {
    enabled: bool,
}
#[derive(Serialize, Deserialize, WireType)]
struct Flattened {
    #[serde(flatten)]
    details: Details,
    maybe: Maybe,
}

#[tokio::test]
async fn flattened_and_transparent_optional_types_preserve_schema_and_missing_behavior() {
    let procedure = loams_web::Procedure::<Flattened, Flattened>::builder("flattened")
        .handler(|_, value| async { Ok(value) })
        .build()
        .unwrap();
    assert_eq!(
        procedure
            .invoke_json(Ctx::new("flattened"), json!({"enabled":true}))
            .await
            .unwrap(),
        json!({"enabled":true,"maybe":null})
    );
    let required = procedure.descriptor().input_schema["required"]
        .as_array()
        .unwrap();
    assert!(!required.contains(&json!("maybe")));
}

#[procedure(name = "restricted", method = "POST", path = "/restricted")]
async fn restricted(_ctx: Ctx, (): ()) -> Result<(), ApiError> {
    Err(ApiError::new(
        loams_web::RpcCode::PermissionDenied,
        "no_access",
        "Access denied",
    ))
}

#[tokio::test]
async fn generated_builder_extends_declared_errors_and_summary() {
    let procedure = restricted_procedure_builder()
        .summary("Restricted operation")
        .error(
            loams_web::RpcCode::PermissionDenied,
            "no_access",
            "Access denied",
        )
        .build()
        .unwrap();
    assert_eq!(
        procedure.descriptor().summary.as_deref(),
        Some("Restricted operation")
    );
    assert_eq!(
        procedure
            .invoke(Ctx::new("denied"), ())
            .await
            .unwrap_err()
            .reason,
        "no_access"
    );
    assert_eq!(
        restricted_procedure()
            .unwrap()
            .invoke(Ctx::new("hidden"), ())
            .await
            .unwrap_err()
            .reason,
        "internal"
    );
}
