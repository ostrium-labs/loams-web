//! Transport-independent typed procedures, schemas, errors, and request context.
//!
//! Input and output JSON Schemas are generated from their respective serde wire
//! shapes. The same schemas enforce bounds during invocation and describe the API.

use schemars::{JsonSchema, generate::SchemaSettings};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    any::Any,
    collections::{BTreeSet, HashMap, HashSet},
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::OnceCell;

/// Canonical RPC codes; transport adapters project these to their protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcCode {
    Cancelled,
    Unknown,
    InvalidArgument,
    DeadlineExceeded,
    NotFound,
    AlreadyExists,
    PermissionDenied,
    ResourceExhausted,
    FailedPrecondition,
    Aborted,
    OutOfRange,
    Unimplemented,
    Internal,
    Unavailable,
    DataLoss,
    Unauthenticated,
}

impl RpcCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
            Self::InvalidArgument => "invalid_argument",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::NotFound => "not_found",
            Self::AlreadyExists => "already_exists",
            Self::PermissionDenied => "permission_denied",
            Self::ResourceExhausted => "resource_exhausted",
            Self::FailedPrecondition => "failed_precondition",
            Self::Aborted => "aborted",
            Self::OutOfRange => "out_of_range",
            Self::Unimplemented => "unimplemented",
            Self::Internal => "internal",
            Self::Unavailable => "unavailable",
            Self::DataLoss => "data_loss",
            Self::Unauthenticated => "unauthenticated",
        }
    }
    pub const fn http_status(self) -> u16 {
        match self {
            Self::Cancelled => 499,
            Self::InvalidArgument | Self::FailedPrecondition | Self::OutOfRange => 400,
            Self::DeadlineExceeded => 504,
            Self::NotFound => 404,
            Self::AlreadyExists | Self::Aborted => 409,
            Self::PermissionDenied => 403,
            Self::ResourceExhausted => 429,
            Self::Unimplemented => 501,
            Self::Unavailable => 503,
            Self::Unauthenticated => 401,
            Self::Unknown | Self::Internal | Self::DataLoss => 500,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryMetadata {
    pub after_millis: u64,
}

/// An intentionally public error. Messages/details must be safe for the caller.
/// Framework failures use fixed messages and never forward serde/handler errors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: RpcCode,
    pub reason: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub details: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<Box<RetryMetadata>>,
}
impl ApiError {
    pub fn new(code: RpcCode, reason: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
            message: message.into(),
            details: Value::Null,
            request_id: None,
            retry: None,
        }
    }
    pub fn invalid_argument(reason: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(RpcCode::InvalidArgument, reason, message)
    }
    pub fn internal() -> Self {
        Self::new(RpcCode::Internal, "internal", "An internal error occurred")
    }
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }
    pub fn with_retry(mut self, after_millis: u64) -> Self {
        self.retry = Some(Box::new(RetryMetadata { after_millis }));
        self
    }
}
impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.reason, self.message)
    }
}
impl std::error::Error for ApiError {}

/// A bounded, safe description of an invalid input. `at` is an RFC 6901 JSON
/// pointer, with an empty string denoting the root. Dynamic object keys are
/// omitted: only declared field names and array indices appear in pointers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct ValidationProblem {
    #[schemars(length(max = MAX_VALIDATION_POINTER_BYTES))]
    pub at: String,
    #[schemars(length(max = 32))]
    pub code: String,
    #[schemars(length(max = 96))]
    pub message: String,
}

/// Alias retained for consumers that call one issue an input problem.
pub type InputProblem = ValidationProblem;

/// Structured input error details. `truncated` signals a count, pointer or schema
/// traversal budget; dynamic map keys are always collapsed to their safe parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationDetails {
    #[schemars(length(max = MAX_VALIDATION_PROBLEMS))]
    pub problems: Vec<ValidationProblem>,
    pub truncated: bool,
}

pub const MAX_VALIDATION_PROBLEMS: usize = 32;
pub const MAX_VALIDATION_ERRORS: usize = 256;
pub const MAX_VALIDATION_POINTER_BYTES: usize = 256;
pub const MAX_VALIDATION_POINTER_DEPTH: usize = 16;
const MAX_SCHEMA_SEARCH: usize = 64;

fn input_error(problems: Vec<InputProblem>, truncated: bool) -> ApiError {
    ApiError::invalid_argument("invalid_input", "Input does not match the procedure schema")
        .with_details(serde_json::json!(ValidationDetails {
            problems,
            truncated
        }))
}

fn root_input_error(code: &str, message: &str) -> ApiError {
    let mut error = input_error(
        vec![InputProblem {
            at: String::new(),
            code: code.into(),
            message: message.into(),
        }],
        false,
    );
    error.message = match code {
        "encoding" => "Input cannot be represented as JSON",
        "decode" => "Input cannot be decoded",
        _ => "Input does not match the procedure schema",
    }
    .into();
    error
}

// Never format ValidationError: its instance, schema values and parser wording
// can contain credentials, rejected values or request-selected property names.
fn problem_kind(kind: &jsonschema::error::ValidationErrorKind) -> (&'static str, &'static str) {
    use jsonschema::error::ValidationErrorKind as Kind;
    match kind {
        Kind::Required { .. } => ("required", "A required field is missing."),
        Kind::Type { .. } => ("type", "Value has the wrong type."),
        Kind::Minimum { .. } => ("minimum", "Must be at least the minimum."),
        Kind::Maximum { .. } => ("maximum", "Must be at most the maximum."),
        Kind::ExclusiveMinimum { .. } => ("exclusive_minimum", "Must be greater than the minimum."),
        Kind::ExclusiveMaximum { .. } => ("exclusive_maximum", "Must be less than the maximum."),
        Kind::MultipleOf { .. } => ("multiple_of", "Must use an allowed numeric increment."),
        Kind::MinLength { .. } => ("min_length", "Text is too short."),
        Kind::MaxLength { .. } => ("max_length", "Text is too long."),
        Kind::MinItems { .. } => ("min_items", "There are too few items."),
        Kind::MaxItems { .. } | Kind::AdditionalItems { .. } | Kind::UnevaluatedItems { .. } => {
            ("max_items", "There are too many items.")
        }
        Kind::MinProperties { .. } => ("min_properties", "There are too few fields."),
        Kind::MaxProperties { .. } => ("max_properties", "There are too many fields."),
        Kind::AdditionalProperties { .. } | Kind::UnevaluatedProperties { .. } => {
            ("unknown_field", "An undeclared field is present.")
        }
        Kind::Enum { .. } | Kind::Constant { .. } => ("choice", "Value is not an allowed choice."),
        Kind::Format { .. } | Kind::Pattern { .. } => ("format", "Value has an invalid format."),
        Kind::UniqueItems => ("unique_items", "Items must be unique."),
        Kind::PropertyNames { .. } => ("field_name", "A field name is invalid."),
        _ => ("invalid", "Value does not match the required shape."),
    }
}

// Expand only local references and composition branches, with a fixed work
// budget. Recursive schemas are safe because each node is visited once here.
fn schema_candidates<'a>(root: &'a Value, seeds: &[&'a Value]) -> (Vec<&'a Value>, bool) {
    let mut stack = seeds
        .iter()
        .copied()
        .take(MAX_SCHEMA_SEARCH)
        .collect::<Vec<_>>();
    let mut limited = seeds.len() > MAX_SCHEMA_SEARCH;
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    while let Some(schema) = stack.pop() {
        if !seen.insert(std::ptr::from_ref(schema)) {
            continue;
        }
        if seen.len() > MAX_SCHEMA_SEARCH {
            limited = true;
            break;
        }
        result.push(schema);
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            if reference == "#" {
                stack.push(root);
            } else if let Some(target) = reference
                .strip_prefix('#')
                .and_then(|pointer| root.pointer(pointer))
            {
                stack.push(target);
            }
        }
        for keyword in ["allOf", "anyOf", "oneOf"] {
            if let Some(branches) = schema.get(keyword).and_then(Value::as_array) {
                let available = MAX_SCHEMA_SEARCH.saturating_sub(stack.len());
                limited |= branches.len() > available;
                stack.extend(branches.iter().take(available));
            }
        }
    }
    (result, limited)
}

fn safe_pointer(
    root: &Value,
    instance: &Value,
    pointer: &str,
    required: Option<&str>,
) -> (String, bool) {
    let mut schemas = vec![root];
    let mut current = Some(instance);
    let mut at = String::new();
    let mut limited = false;
    // Required properties originate in the schema, but still must pass the same
    // declared-field checks; raw instance pointer segments are always untrusted.
    let segments = pointer
        .split('/')
        .skip(1)
        .map(|part| (part, true))
        .chain(required.map(|part| (part, false)));
    for (depth, (raw, escaped)) in segments.enumerate() {
        if depth >= MAX_VALIDATION_POINTER_DEPTH || raw.len() > MAX_VALIDATION_POINTER_BYTES {
            limited = true;
            break;
        }
        let segment = if escaped {
            raw.replace("~1", "/").replace("~0", "~")
        } else {
            raw.to_owned()
        };
        let (candidates, bounded) = schema_candidates(root, &schemas);
        limited |= bounded;
        let mut next = Vec::new();
        let declared = if current.is_some_and(Value::is_array) {
            if let Ok(index) = segment.parse::<usize>() {
                for schema in &candidates {
                    if let Some(item) = schema
                        .get("prefixItems")
                        .and_then(Value::as_array)
                        .and_then(|items| items.get(index))
                        .or_else(|| schema.get("items"))
                    {
                        next.push(item);
                    }
                }
                !next.is_empty()
            } else {
                false
            }
        } else {
            let mut declared = false;
            for schema in &candidates {
                if let Some(property) = schema
                    .get("properties")
                    .and_then(|properties| properties.get(&segment))
                {
                    declared = true;
                    next.push(property);
                }
                if schema
                    .get("required")
                    .and_then(Value::as_array)
                    .is_some_and(|required| {
                        required.iter().any(|name| name.as_str() == Some(&segment))
                    })
                {
                    declared = true;
                }
            }
            declared
        };
        if !declared {
            break;
        }
        let encoded = segment.replace('~', "~0").replace('/', "~1");
        if at.len() + encoded.len() + 1 > MAX_VALIDATION_POINTER_BYTES {
            limited = true;
            break;
        }
        at.push('/');
        at.push_str(&encoded);
        current = current.and_then(|value| match value {
            Value::Array(items) => segment
                .parse::<usize>()
                .ok()
                .and_then(|index| items.get(index)),
            Value::Object(fields) => fields.get(&segment),
            _ => None,
        });
        schemas = next;
    }
    (at, limited)
}

fn validate_input(
    validator: &jsonschema::Validator,
    schema: &Value,
    instance: &Value,
) -> Result<(), ApiError> {
    let mut problems = BTreeSet::new();
    let mut truncated = false;
    for (index, error) in validator.iter_errors(instance).enumerate() {
        if index == MAX_VALIDATION_ERRORS {
            truncated = true;
            break;
        }
        let required = match &error.kind {
            jsonschema::error::ValidationErrorKind::Required { property } => property.as_str(),
            _ => None,
        };
        let (at, limited) = safe_pointer(schema, instance, error.instance_path.as_str(), required);
        truncated |= limited;
        let (code, message) = problem_kind(&error.kind);
        problems.insert(InputProblem {
            at,
            code: code.into(),
            message: message.into(),
        });
        if problems.len() > MAX_VALIDATION_PROBLEMS {
            problems.pop_last();
            truncated = true;
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(input_error(problems.into_iter().collect(), truncated))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Principal {
    pub namespace: String,
    pub subject: String,
    pub scopes: Vec<String>,
}

type Cached = Arc<dyn Any + Send + Sync>;
type CacheCell = Arc<OnceCell<Result<Cached, ApiError>>>;
/// Request state. Clones share only this request's dependency cache.
#[derive(Clone)]
pub struct Ctx {
    pub request_id: String,
    pub principal: Option<Principal>,
    dependencies: Arc<Mutex<HashMap<u64, CacheCell>>>,
}
impl Ctx {
    pub fn new(request_id: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            principal: None,
            dependencies: Arc::default(),
        }
    }
    pub fn with_principal(mut self, principal: Principal) -> Self {
        self.principal = Some(principal);
        self
    }
}
impl fmt::Debug for Ctx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ctx")
            .field("request_id", &self.request_id)
            .finish_non_exhaustive()
    }
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
type DependencyFn<T> = dyn Fn(Ctx) -> BoxFuture<Result<T, ApiError>> + Send + Sync;
static NEXT_DEPENDENCY: AtomicU64 = AtomicU64::new(1);
/// An identity-bearing dependency; success and refusal are shared per request.
pub struct Dependency<T> {
    id: u64,
    run: Arc<DependencyFn<T>>,
}
impl<T> Clone for Dependency<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            run: self.run.clone(),
        }
    }
}
impl<T: Send + Sync + 'static> Dependency<T> {
    pub fn new<F, Fut>(run: F) -> Self
    where
        F: Fn(Ctx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, ApiError>> + Send + 'static,
    {
        Self {
            id: NEXT_DEPENDENCY.fetch_add(1, Ordering::Relaxed),
            run: Arc::new(move |ctx| Box::pin(run(ctx))),
        }
    }
    /// Concurrent callers share initialization. A cancelled initializer may retry.
    /// Dependencies must form an acyclic graph; recursive resolution waits forever.
    pub async fn resolve(&self, ctx: &Ctx) -> Result<Arc<T>, ApiError> {
        let cell = ctx
            .dependencies
            .lock()
            .map_err(|_| ApiError::internal())?
            .entry(self.id)
            .or_default()
            .clone();
        let result = cell
            .get_or_init(|| async {
                (self.run)(ctx.clone())
                    .await
                    .map(|value| Arc::new(value) as Cached)
            })
            .await
            .clone();
        result.and_then(|value| value.downcast::<T>().map_err(|_| ApiError::internal()))
    }
    pub async fn uncached(&self, ctx: &Ctx) -> Result<T, ApiError> {
        (self.run)(ctx.clone()).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}
impl HttpMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
        }
    }
}
impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpBinding {
    pub method: HttpMethod,
    pub path: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorDescriptor {
    pub code: RpcCode,
    pub reason: String,
    pub description: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureDescriptor {
    pub name: String,
    pub summary: Option<String>,
    pub http: Option<HttpBinding>,
    pub input_schema: Value,
    pub output_schema: Value,
    pub errors: Vec<ErrorDescriptor>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionError(pub String);
impl fmt::Display for DefinitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for DefinitionError {}

type Handler<I, O> = dyn Fn(Ctx, I) -> BoxFuture<Result<O, ApiError>> + Send + Sync;
type InputValidator<I> = dyn Fn(&I) -> Result<(), ApiError> + Send + Sync;
struct OfflineSchemas;
impl jsonschema::Retrieve for OfflineSchemas {
    fn retrieve(
        &self,
        _: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err("External schema references are disabled".into())
    }
}
fn compile_schema(schema: &Value) -> Result<jsonschema::Validator, DefinitionError> {
    jsonschema::options()
        .with_retriever(OfflineSchemas)
        .build(schema)
        .map_err(|_| DefinitionError("Invalid or externally referenced JSON Schema".into()))
}
/// One handler and one descriptor, reusable by each transport.
pub struct Procedure<I, O> {
    descriptor: ProcedureDescriptor,
    handler: Arc<Handler<I, O>>,
    input_validator: Option<Arc<InputValidator<I>>>,
    input_schema: jsonschema::Validator,
    typed_input_schema: jsonschema::Validator,
    typed_input_description: Value,
    output_schema: jsonschema::Validator,
}
impl<I, O> Procedure<I, O>
where
    I: Serialize + DeserializeOwned + JsonSchema + Send + 'static,
    O: Serialize + JsonSchema + Send + 'static,
{
    pub fn builder(name: impl Into<String>) -> ProcedureBuilder<I, O> {
        ProcedureBuilder {
            name: name.into(),
            summary: None,
            http: None,
            errors: vec![],
            handler: None,
            input_validator: None,
        }
    }
    pub fn descriptor(&self) -> &ProcedureDescriptor {
        &self.descriptor
    }
    pub async fn invoke(&self, ctx: Ctx, input: I) -> Result<O, ApiError> {
        let request_id = ctx.request_id.clone();
        let result = async {
            let value = serde_json::to_value(&input).map_err(|_| {
                root_input_error("encoding", "Input must be representable as JSON.")
            })?;
            validate_input(
                &self.typed_input_schema,
                &self.typed_input_description,
                &value,
            )?;
            self.invoke_validated(ctx, input).await
        }
        .await;
        result.map_err(|error| self.declared_error(error).with_request_id(request_id))
    }
    fn check_input(&self, value: &Value) -> Result<(), ApiError> {
        validate_input(&self.input_schema, &self.descriptor.input_schema, value)
    }
    fn declared_error(&self, error: ApiError) -> ApiError {
        if self
            .descriptor
            .errors
            .iter()
            .any(|declared| declared.code == error.code && declared.reason == error.reason)
        {
            error
        } else {
            ApiError::internal()
        }
    }
    async fn invoke_validated(&self, ctx: Ctx, input: I) -> Result<O, ApiError> {
        if let Some(validate) = &self.input_validator {
            validate(&input)?;
        }
        let output = (self.handler)(ctx, input).await?;
        let value = serde_json::to_value(&output).map_err(|_| ApiError::internal())?;
        if !self.output_schema.is_valid(&value) {
            return Err(ApiError::internal());
        }
        Ok(output)
    }
    pub async fn invoke_json(&self, ctx: Ctx, input: Value) -> Result<Value, ApiError> {
        let request_id = ctx.request_id.clone();
        let result = async {
            self.check_input(&input)?;
            let input = serde_json::from_value(input)
                .map_err(|_| root_input_error("decode", "Input cannot be decoded."))?;
            let output = self.invoke_validated(ctx, input).await?;
            serde_json::to_value(output).map_err(|_| ApiError::internal())
        }
        .await;
        result.map_err(|error| self.declared_error(error).with_request_id(request_id))
    }
}
pub struct ProcedureBuilder<I, O> {
    name: String,
    summary: Option<String>,
    http: Option<HttpBinding>,
    errors: Vec<ErrorDescriptor>,
    handler: Option<Arc<Handler<I, O>>>,
    input_validator: Option<Arc<InputValidator<I>>>,
}
impl<I, O> ProcedureBuilder<I, O>
where
    I: Serialize + DeserializeOwned + JsonSchema + Send + 'static,
    O: Serialize + JsonSchema + Send + 'static,
{
    pub fn http(mut self, method: HttpMethod, path: impl Into<String>) -> Self {
        self.http = Some(HttpBinding {
            method,
            path: path.into(),
        });
        self
    }
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }
    pub fn error(
        mut self,
        code: RpcCode,
        reason: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.errors.push(ErrorDescriptor {
            code,
            reason: reason.into(),
            description: description.into(),
        });
        self
    }
    /// Extra business validation. Wire bounds belong on the schema itself.
    pub fn input_validator<F>(mut self, validate: F) -> Self
    where
        F: Fn(&I) -> Result<(), ApiError> + Send + Sync + 'static,
    {
        self.input_validator = Some(Arc::new(validate));
        self
    }
    pub fn handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(Ctx, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, ApiError>> + Send + 'static,
    {
        self.handler = Some(Arc::new(move |ctx, input| Box::pin(handler(ctx, input))));
        self
    }
    pub fn build(mut self) -> Result<Procedure<I, O>, DefinitionError> {
        if self.name.is_empty()
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(DefinitionError(
                "Procedure name must contain ASCII letters, digits, dot, underscore or hyphen"
                    .into(),
            ));
        }
        let mut reasons = std::collections::HashSet::new();
        for error in &self.errors {
            if error.reason.is_empty() || !reasons.insert(error.reason.clone()) {
                return Err(DefinitionError(
                    "Error reasons must be nonempty and unique".into(),
                ));
            }
            if matches!(error.reason.as_str(), "internal" | "invalid_input") {
                return Err(DefinitionError(
                    "Framework error reasons are reserved".into(),
                ));
            }
        }
        let handler = self
            .handler
            .ok_or_else(|| DefinitionError("Procedure needs a handler".into()))?;
        let input_schema = SchemaSettings::draft2020_12()
            .for_deserialize()
            .into_generator()
            .into_root_schema_for::<I>()
            .to_value();
        let typed_input_schema = SchemaSettings::draft2020_12()
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<I>()
            .to_value();
        let output_schema = SchemaSettings::draft2020_12()
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<O>()
            .to_value();
        let input_validator = compile_schema(&input_schema)?;
        let typed_input_validator = compile_schema(&typed_input_schema)?;
        let output_validator = compile_schema(&output_schema)?;
        for (code, reason, description) in [
            (
                RpcCode::InvalidArgument,
                "invalid_input",
                "Input does not match the procedure schema",
            ),
            (RpcCode::Internal, "internal", "An internal error occurred"),
        ] {
            if !self.errors.iter().any(|error| error.reason == reason) {
                self.errors.push(ErrorDescriptor {
                    code,
                    reason: reason.into(),
                    description: description.into(),
                });
            }
        }
        Ok(Procedure {
            descriptor: ProcedureDescriptor {
                name: self.name,
                summary: self.summary,
                http: self.http,
                input_schema,
                output_schema,
                errors: self.errors,
            },
            handler,
            input_validator: self.input_validator,
            input_schema: input_validator,
            typed_input_schema: typed_input_validator,
            typed_input_description: typed_input_schema,
            output_schema: output_validator,
        })
    }
}
/// Object-safe procedure interface for heterogeneous transport registries.
pub trait ErasedProcedure: Send + Sync {
    fn descriptor(&self) -> &ProcedureDescriptor;
    fn invoke_json(
        &self,
        ctx: Ctx,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, ApiError>> + Send + '_>>;
}
impl<I, O> ErasedProcedure for Procedure<I, O>
where
    I: Serialize + DeserializeOwned + JsonSchema + Send + 'static,
    O: Serialize + JsonSchema + Send + 'static,
{
    fn descriptor(&self) -> &ProcedureDescriptor {
        &self.descriptor
    }
    fn invoke_json(
        &self,
        ctx: Ctx,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, ApiError>> + Send + '_>> {
        Box::pin(Procedure::invoke_json(self, ctx, input))
    }
}
