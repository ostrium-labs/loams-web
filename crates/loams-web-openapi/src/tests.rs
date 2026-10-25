use super::*;
use loams_web_core::{HttpMethod, Procedure, RpcCode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema)]
struct Bill {
    #[schemars(range(min = 1, max = 50))]
    people: u32,
    child: Option<Box<Bill>>,
}

#[test]
fn deterministic_generation_keeps_bounds_errors_and_resolves_recursion_without_running_handlers() {
    let p = Procedure::<Bill, Bill>::builder("split")
        .http(HttpMethod::Post, "/split")
        .error(RpcCode::PermissionDenied, "closed", "Bill is closed")
        .handler(|_, _| async { panic!("Schema generation must never invoke a handler") })
        .build()
        .unwrap();
    let q = Procedure::<(), ()>::builder("health")
        .http(HttpMethod::Get, "/health")
        .handler(|_, ()| async { Ok(()) })
        .build()
        .unwrap();
    let first = generate([p.descriptor(), q.descriptor()], "Bills", "1").unwrap();
    let second = generate([q.descriptor(), p.descriptor()], "Bills", "1").unwrap();
    assert_eq!(first, second);
    assert_eq!(first["openapi"], "3.2.0");
    assert_eq!(
        first["paths"]["/split"]["post"]["responses"]["403"]["description"],
        "Bill is closed"
    );
    assert!(
        first["paths"]["/health"]["get"]
            .get("requestBody")
            .is_none()
    );
    assert!(first.to_string().contains("\"maximum\":50"));
    fn references_resolve(value: &Value, document: &Value) {
        match value {
            Value::Object(object) => {
                if let Some(Value::String(reference)) = object.get("$ref") {
                    assert!(
                        document
                            .pointer(reference.strip_prefix('#').unwrap())
                            .is_some(),
                        "Unresolved {reference}"
                    );
                }
                for value in object.values() {
                    references_resolve(value, document);
                }
            }
            Value::Array(array) => {
                for value in array {
                    references_resolve(value, document);
                }
            }
            _ => {}
        }
    }
    references_resolve(&first, &first);
    let required = first["components"]["schemas"]["LoamsApiError"]["required"]
        .as_array()
        .unwrap();
    assert!(!required.contains(&json!("details")));
    assert_eq!(
        first["components"]["schemas"]["LoamsValidationDetails"]["properties"]["problems"]["maxItems"],
        loams_web_core::MAX_VALIDATION_PROBLEMS
    );
    assert_eq!(
        first["paths"]["/split"]["post"]["x-loams-validation-error"]["$ref"],
        "#/components/schemas/LoamsValidationError"
    );
}

#[test]
fn duplicate_operations_and_names_are_rejected() {
    let p = Procedure::<(), ()>::builder("one")
        .http(HttpMethod::Post, "/same")
        .handler(|_, ()| async { Ok(()) })
        .build()
        .unwrap();
    let q = Procedure::<(), ()>::builder("two")
        .http(HttpMethod::Post, "/same")
        .handler(|_, ()| async { Ok(()) })
        .build()
        .unwrap();
    assert!(matches!(
        generate([p.descriptor(), q.descriptor()], "X", "1"),
        Err(OpenApiError::DuplicateOperation(_))
    ));
    assert!(matches!(
        generate([p.descriptor(), p.descriptor()], "X", "1"),
        Err(OpenApiError::DuplicateName(_))
    ));
    let get = Procedure::<(), ()>::builder("get")
        .http(HttpMethod::Get, "/same")
        .handler(|_, ()| async { Ok(()) })
        .build()
        .unwrap();
    let head = Procedure::<(), ()>::builder("head")
        .http(HttpMethod::Head, "/same")
        .handler(|_, ()| async { Ok(()) })
        .build()
        .unwrap();
    assert!(matches!(
        generate([get.descriptor(), head.descriptor()], "X", "1"),
        Err(OpenApiError::DuplicateOperation(_))
    ));
}
