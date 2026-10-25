//! Offline descriptor-driven TypeScript generation and artifact freshness checks.
//! Unsupported schema shapes fail generation instead of silently becoming `any`.

use loams_web_core::{HttpMethod, ProcedureDescriptor};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    path::Path,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationError(pub String);
impl fmt::Display for GenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for GenerationError {}
fn fail(message: impl Into<String>) -> GenerationError {
    GenerationError(message.into())
}
fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization cannot fail")
}
fn identifier(value: &str) -> String {
    let mut out = String::from("T_");
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() {
            out.push(byte as char);
        } else {
            out.push('_');
            out.push_str(&format!("{byte:02x}"));
        }
    }
    out
}

struct SchemaTypes<'a> {
    root: &'a Value,
    prefix: String,
    definitions: BTreeMap<String, String>,
}
impl<'a> SchemaTypes<'a> {
    fn new(root: &'a Value, prefix: String) -> Result<Self, GenerationError> {
        let mut definitions = BTreeMap::new();
        if let Some(defs) = root.get("$defs") {
            for key in defs
                .as_object()
                .ok_or_else(|| fail("$defs must be an object"))?
                .keys()
            {
                definitions.insert(key.clone(), format!("{prefix}_{}", identifier(key)));
            }
        }
        Ok(Self {
            root,
            prefix,
            definitions,
        })
    }
    fn declarations(&self) -> Result<String, GenerationError> {
        let mut text = format!(
            "export type {} = {};\n",
            self.prefix,
            self.render(self.root)?
        );
        for (name, alias) in &self.definitions {
            text.push_str(&format!(
                "type {alias} = {};\n",
                self.render(&self.root["$defs"][name])?
            ));
        }
        Ok(text)
    }
    fn render(&self, schema: &Value) -> Result<String, GenerationError> {
        match schema {
            Value::Bool(true) => return Ok("unknown".into()),
            Value::Bool(false) => return Ok("never".into()),
            _ => {}
        }
        let object = schema
            .as_object()
            .ok_or_else(|| fail("Schema must be a JSON object or boolean"))?;
        for key in object.keys() {
            if ![
                "$schema",
                "$defs",
                "$ref",
                "$id",
                "title",
                "description",
                "default",
                "examples",
                "deprecated",
                "readOnly",
                "writeOnly",
                "type",
                "properties",
                "required",
                "additionalProperties",
                "items",
                "enum",
                "const",
                "anyOf",
                "oneOf",
                "allOf",
                "minimum",
                "maximum",
                "exclusiveMinimum",
                "exclusiveMaximum",
                "multipleOf",
                "minLength",
                "maxLength",
                "pattern",
                "format",
                "minItems",
                "maxItems",
                "uniqueItems",
                "minProperties",
                "maxProperties",
            ]
            .contains(&key.as_str())
            {
                return Err(fail(format!("Unsupported JSON Schema keyword: {key}")));
            }
        }
        if let Some(reference) = object.get("$ref") {
            let reference = reference
                .as_str()
                .ok_or_else(|| fail("$ref must be a string"))?;
            if object.keys().any(|key| {
                [
                    "type",
                    "properties",
                    "items",
                    "enum",
                    "const",
                    "anyOf",
                    "oneOf",
                    "allOf",
                    "additionalProperties",
                    "required",
                ]
                .contains(&key.as_str())
            }) {
                return Err(fail("Validation siblings of $ref are unsupported"));
            }
            if reference == "#" {
                return Ok(self.prefix.clone());
            }
            let key = reference
                .strip_prefix("#/$defs/")
                .ok_or_else(|| fail("Only local $defs references are supported"))?
                .replace("~1", "/")
                .replace("~0", "~");
            return self
                .definitions
                .get(&key)
                .cloned()
                .ok_or_else(|| fail(format!("Missing schema definition: {key}")));
        }
        if let Some(value) = object.get("const") {
            return scalar_literal(value);
        }
        if let Some(values) = object.get("enum") {
            let values = values
                .as_array()
                .ok_or_else(|| fail("enum must be an array"))?;
            if values.is_empty() {
                return Ok("never".into());
            }
            return values
                .iter()
                .map(scalar_literal)
                .collect::<Result<Vec<_>, _>>()
                .map(|values| values.join(" | "));
        }
        for (keyword, join) in [("anyOf", " | "), ("oneOf", " | "), ("allOf", " & ")] {
            if let Some(variants) = object.get(keyword) {
                if object.keys().any(|key| {
                    [
                        "type",
                        "properties",
                        "items",
                        "enum",
                        "const",
                        "additionalProperties",
                        "required",
                        "anyOf",
                        "oneOf",
                        "allOf",
                    ]
                    .contains(&key.as_str())
                        && key != keyword
                }) {
                    return Err(fail("Combined schema composition is unsupported"));
                }
                let variants = variants
                    .as_array()
                    .ok_or_else(|| fail(format!("{keyword} must be an array")))?;
                if variants.is_empty() {
                    return Err(fail("Empty schema composition is unsupported"));
                }
                return variants
                    .iter()
                    .map(|schema| self.render(schema).map(|value| format!("({value})")))
                    .collect::<Result<Vec<_>, _>>()
                    .map(|values| values.join(join));
            }
        }
        match object.get("type") {
            Some(Value::Array(types)) => {
                if types.is_empty() || types.iter().any(|kind| !kind.is_string()) {
                    return Err(fail("type arrays must contain supported string types"));
                }
                types
                    .iter()
                    .map(|kind| {
                        let mut schema = object.clone();
                        schema.insert("type".into(), kind.clone());
                        self.render(&Value::Object(schema))
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map(|values| values.join(" | "))
            }
            Some(Value::String(kind)) => match kind.as_str() {
                "null" => Ok("null".into()),
                "boolean" => Ok("boolean".into()),
                "string" => Ok("string".into()),
                "number" | "integer" => Ok("number".into()),
                "array" => {
                    let item = object
                        .get("items")
                        .ok_or_else(|| fail("Array schemas require items"))?;
                    Ok(format!("Array<{}>", self.render(item)?))
                }
                "object" => self.object(object),
                _ => Err(fail(format!("Unsupported JSON Schema type: {kind}"))),
            },
            None if object.keys().all(|key| {
                [
                    "$schema",
                    "$defs",
                    "$id",
                    "title",
                    "description",
                    "default",
                    "examples",
                ]
                .contains(&key.as_str())
            }) =>
            {
                Ok("unknown".into())
            }
            _ => Err(fail("Schema requires a supported explicit type")),
        }
    }
    fn object(&self, schema: &Map<String, Value>) -> Result<String, GenerationError> {
        let properties = schema
            .get("properties")
            .map(|properties| {
                properties
                    .as_object()
                    .ok_or_else(|| fail("properties must be an object"))
            })
            .transpose()?;
        let mut required = BTreeSet::new();
        if let Some(value) = schema.get("required") {
            for key in value
                .as_array()
                .ok_or_else(|| fail("required must be an array"))?
            {
                required.insert(
                    key.as_str()
                        .ok_or_else(|| fail("required entries must be strings"))?
                        .to_owned(),
                );
            }
        }
        let mut keys = required.clone();
        if let Some(properties) = properties {
            keys.extend(properties.keys().cloned());
        }
        let mut fields = String::from("{ ");
        for key in keys {
            let schema = properties.and_then(|props| props.get(&key));
            fields.push_str(&format!(
                "{}{}: {}; ",
                quoted(&key),
                if required.contains(&key) { "" } else { "?" },
                schema
                    .map(|schema| self.render(schema))
                    .transpose()?
                    .unwrap_or_else(|| "unknown".into())
            ));
        }
        match schema.get("additionalProperties") {
            Some(Value::Bool(false)) => {
                fields.push('}');
                Ok(fields)
            }
            None | Some(Value::Bool(true)) => {
                fields.push_str("[key: string]: unknown; }");
                Ok(fields)
            }
            Some(additional) => {
                if properties.is_some_and(|properties| !properties.is_empty())
                    || !required.is_empty()
                {
                    return Err(fail(
                        "Typed additionalProperties mixed with named fields is unsupported",
                    ));
                }
                fields.push('}');
                Ok(format!(
                    "({fields}) & Record<string, {}>",
                    self.render(additional)?
                ))
            }
        }
    }
}
fn scalar_literal(value: &Value) -> Result<String, GenerationError> {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(value.to_string()),
        _ => Err(fail("Only scalar const/enum values are supported")),
    }
}

/// Emit a stable fetch client. Generation needs no server or network connection.
/// Numeric wire types use JavaScript numbers; callers must respect safe integers.
/// Schema constraints remain enforced by the server, rather than duplicated here.
pub fn generate_typescript(descriptors: &[ProcedureDescriptor]) -> Result<String, GenerationError> {
    let mut procedures = descriptors.iter().collect::<Vec<_>>();
    procedures.sort_by(|a, b| a.name.cmp(&b.name));
    let mut names = BTreeSet::new();
    let mut bindings = BTreeSet::new();
    let mut text = String::from("// Generated by loams-web-cli. Do not edit.\n\n");
    for descriptor in &procedures {
        if descriptor.name.is_empty() || !names.insert(&descriptor.name) {
            return Err(fail("Procedure names must be nonempty and unique"));
        }
        let binding = descriptor
            .http
            .as_ref()
            .ok_or_else(|| fail(format!("{} has no HTTP binding", descriptor.name)))?;
        if matches!(
            binding.method,
            HttpMethod::Get | HttpMethod::Head | HttpMethod::Options
        ) {
            return Err(fail(format!(
                "{} requires a JSON-body HTTP binding",
                descriptor.name
            )));
        }
        if !binding.path.starts_with('/')
            || binding.path.starts_with("//")
            || binding.path.contains(['?', '#', '{', '}', ':', '*', '\\'])
            || binding.path.chars().any(char::is_control)
        {
            return Err(fail(format!(
                "{} requires a static absolute HTTP path",
                descriptor.name
            )));
        }
        if !bindings.insert((binding.method, &binding.path)) {
            return Err(fail("Duplicate HTTP binding"));
        }
        let prefix = identifier(&descriptor.name);
        text.push_str(
            &SchemaTypes::new(&descriptor.input_schema, format!("{prefix}_Input"))?
                .declarations()?,
        );
        text.push_str(
            &SchemaTypes::new(&descriptor.output_schema, format!("{prefix}_Output"))?
                .declarations()?,
        );
        text.push('\n');
    }
    text.push_str("export class ApiClientError extends Error {\n  constructor(public readonly status: number, public readonly body: unknown) {\n    super(`Request failed with HTTP ${status}`);\n    this.name = \"ApiClientError\";\n  }\n}\n\nexport function createClient(baseUrl: string, fetchImpl: typeof fetch = fetch) {\n  const base = baseUrl.replace(/\\/$/, \"\");\n  return {\n");
    for descriptor in procedures {
        let prefix = identifier(&descriptor.name);
        let binding = descriptor.http.as_ref().expect("bindings validated above");
        text.push_str(&format!(r#"    [{}]: async (input: {prefix}_Input, options: {{ signal?: AbortSignal; headers?: HeadersInit }} = {{}}): Promise<{prefix}_Output> => {{
      const headers = new Headers(options.headers);
      headers.set("content-type", "application/json");
      const response = await fetchImpl(base + {}, {{
        method: {},
        headers,
        body: JSON.stringify(input),
        signal: options.signal,
      }});
      const bodyText = await response.text();
      let body: unknown;
      try {{ body = JSON.parse(bodyText); }} catch {{
        if (response.ok) throw new Error("Server returned invalid JSON");
        body = bodyText;
      }}
      if (!response.ok) throw new ApiClientError(response.status, body);
      return body as {prefix}_Output;
    }},
"#, quoted(&descriptor.name), quoted(&binding.path), quoted(binding.method.as_str())));
    }
    text.push_str("  };\n}\n");
    Ok(text)
}

#[derive(Debug)]
pub enum ArtifactError {
    Io(std::io::Error),
    Stale,
}
impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Stale => f.write_str("Generated artifact is stale; regenerate it with --write"),
        }
    }
}
impl std::error::Error for ArtifactError {}
pub fn write_client(path: impl AsRef<Path>, source: &str) -> Result<(), ArtifactError> {
    fs::write(path, source).map_err(ArtifactError::Io)
}
pub fn check_client(path: impl AsRef<Path>, source: &str) -> Result<(), ArtifactError> {
    if fs::read_to_string(path).map_err(ArtifactError::Io)? == source {
        Ok(())
    } else {
        Err(ArtifactError::Stale)
    }
}
