//! Deterministic OpenAPI 3.2 generation from procedure metadata alone.
//!
//! No server, handler invocation, or database is needed to generate a document.

use loams_web_core::ProcedureDescriptor;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, thiserror::Error)]
pub enum OpenApiError {
    #[error("duplicate procedure name: {0}")]
    DuplicateName(String),
    #[error("duplicate operation: {0}")]
    DuplicateOperation(String),
    #[error("unsupported HTTP path: {0}")]
    UnsupportedPath(String),
    #[error("unsupported HTTP method: {0}")]
    UnsupportedMethod(String),
    #[error("unsupported external schema reference: {0}")]
    ExternalReference(String),
    #[error("GET, HEAD and OPTIONS procedures must accept unit (JSON null)")]
    InvalidInputBinding,
}

/// Generate an OpenAPI 3.2 document. Procedures without REST bindings are omitted.
///
/// This slice describes literal paths and JSON request/response bodies. Schemars
/// definitions are relocated into components with all local references rewritten.
pub fn generate<'a>(
    descriptors: impl IntoIterator<Item = &'a ProcedureDescriptor>,
    title: impl Into<String>,
    version: impl Into<String>,
) -> Result<Value, OpenApiError> {
    let mut descriptors: Vec<_> = descriptors.into_iter().collect();
    descriptors.sort_by(|a, b| a.name.cmp(&b.name));
    let mut paths = Map::new();
    let mut schemas = Map::new();
    let mut names = BTreeSet::new();
    let mut operations = BTreeSet::new();
    schemas.insert(
        "LoamsApiError".into(),
        json!({
            "type":"object", "required":["code","reason","message"],
            "properties": {
                "code":{"type":"string"}, "reason":{"type":"string"}, "message":{"type":"string"},
                "details":{}, "request_id":{"type":["string","null"]}, "retry":{}
            }
        }),
    );
    for descriptor in descriptors {
        if !names.insert(descriptor.name.clone()) {
            return Err(OpenApiError::DuplicateName(descriptor.name.clone()));
        }
        let Some(binding) = &descriptor.http else {
            continue;
        };
        if !binding.path.starts_with('/')
            || binding.path.contains(['{', '}', '*', '?', '#'])
            || binding
                .path
                .split('/')
                .any(|segment| segment.starts_with(':'))
            || binding.path.chars().any(char::is_control)
        {
            return Err(OpenApiError::UnsupportedPath(binding.path.clone()));
        }
        let method = binding.method.as_str().to_ascii_lowercase();
        if matches!(method.as_str(), "get" | "head" | "options")
            && descriptor.input_schema.get("type").and_then(Value::as_str) != Some("null")
        {
            return Err(OpenApiError::InvalidInputBinding);
        }
        if !matches!(
            method.as_str(),
            "get" | "head" | "post" | "put" | "patch" | "delete" | "options"
        ) {
            return Err(OpenApiError::UnsupportedMethod(method));
        }
        let counterpart = match method.as_str() {
            "get" => Some("head"),
            "head" => Some("get"),
            _ => None,
        };
        if counterpart
            .is_some_and(|method| operations.contains(&(binding.path.clone(), method.to_string())))
            || !operations.insert((binding.path.clone(), method.clone()))
        {
            return Err(OpenApiError::DuplicateOperation(format!(
                "{} {}",
                method, binding.path
            )));
        }
        let prefix = component_name(&descriptor.name);
        let input_ref = add_schema(
            &format!("{prefix}.Input"),
            &descriptor.input_schema,
            &mut schemas,
        )?;
        let output_ref = add_schema(
            &format!("{prefix}.Output"),
            &descriptor.output_schema,
            &mut schemas,
        )?;
        let mut responses = BTreeMap::new();
        responses.insert("200".to_string(), if method == "head" { json!({"description":"Successful response"}) } else { json!({"description":"Successful response", "content":{"application/json":{"schema":{"$ref":output_ref}}}}) });
        for (status, description) in [
            (400, "Invalid request"),
            (413, "Request body too large"),
            (415, "Unsupported media type"),
            (422, "Invalid JSON"),
            (500, "Internal error"),
            (503, "Server at capacity"),
            (504, "Request deadline exceeded"),
        ] {
            responses.insert(status.to_string(), error_response(description));
        }
        let mut declared = Vec::new();
        for error in &descriptor.errors {
            responses.insert(
                error.code.http_status().to_string(),
                error_response(&error.description),
            );
            declared.push(json!({"code":error.code.as_str(), "reason":error.reason, "description":error.description}));
        }
        let mut operation = json!({"operationId":descriptor.name, "responses":responses, "x-loams-errors":declared});
        if let Some(summary) = &descriptor.summary {
            operation["summary"] = Value::String(summary.clone());
        }
        if !matches!(method.as_str(), "get" | "head" | "options") {
            operation["requestBody"] = json!({"required":true,"content":{"application/json":{"schema":{"$ref":input_ref}}}});
        }
        let entry = paths
            .entry(binding.path.clone())
            .or_insert_with(|| json!({}));
        entry
            .as_object_mut()
            .expect("path object")
            .insert(method, operation);
    }
    Ok(
        json!({"openapi":"3.2.0", "jsonSchemaDialect":"https://json-schema.org/draft/2020-12/schema", "info":{"title":title.into(),"version":version.into()}, "paths":paths,"components":{"schemas":schemas}}),
    )
}

#[cfg(test)]
mod tests;

fn error_response(description: &str) -> Value {
    json!({"description":description,"content":{"application/json":{"schema":{"$ref":"#/components/schemas/LoamsApiError"}}}})
}

fn component_name(name: &str) -> String {
    // Hex encoding is injective, so arbitrary stable names cannot collide after sanitization.
    let mut result = String::from("Procedure");
    for byte in name.bytes() {
        result.push_str(&format!("{byte:02x}"));
    }
    result
}

fn add_schema(
    name: &str,
    schema: &Value,
    schemas: &mut Map<String, Value>,
) -> Result<String, OpenApiError> {
    let mut schema = schema.clone();
    let definitions = schema.as_object_mut().and_then(|object| {
        object.remove("$schema");
        object.remove("$defs")
    });
    rewrite_references(&mut schema, name)?;
    schemas.insert(name.into(), schema);
    if let Some(Value::Object(definitions)) = definitions {
        for (key, mut value) in definitions {
            rewrite_references(&mut value, name)?;
            // Preserve escaped JSON pointer suffixes in refs; component names themselves
            // use raw definition names and are escaped when linked below.
            schemas.insert(format!("{name}.defs.{key}"), value);
        }
    }
    Ok(format!("#/components/schemas/{}", pointer_escape(name)))
}

fn pointer_escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn rewrite_references(value: &mut Value, root: &str) -> Result<(), OpenApiError> {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(reference)) = object.get_mut("$ref") {
                if reference == "#" {
                    *reference = format!("#/components/schemas/{}", pointer_escape(root));
                } else if let Some(suffix) = reference.strip_prefix("#/$defs/") {
                    *reference = format!(
                        "#/components/schemas/{}.defs.{suffix}",
                        pointer_escape(root)
                    );
                } else {
                    return Err(OpenApiError::ExternalReference(reference.clone()));
                }
            }
            for value in object.values_mut() {
                rewrite_references(value, root)?;
            }
        }
        Value::Array(array) => {
            for value in array {
                rewrite_references(value, root)?;
            }
        }
        _ => {}
    }
    Ok(())
}
