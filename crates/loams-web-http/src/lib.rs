//! Bounded JSON REST adapter for transport-independent Loams Web procedures.
//!
//! This first slice supports literal paths. [`server`] adds connection admission,
//! HTTP/1 header-read deadlines, HTTP/2 stream bounds, and bounded shutdown. Path/query extractors,
//! authentication, streaming, and progress-based I/O limits remain future work.

pub mod server;

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{DefaultBodyLimit, FromRequest, Request},
    response::{IntoResponse, Response},
    routing::{MethodFilter, on},
};
use http::{Method, StatusCode, header};
use loams_web_core::{ApiError, Ctx, ErasedProcedure, ProcedureDescriptor, RpcCode};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    error::Error as _,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;

/// Limits enforced before decoding and while invoking each procedure.
#[derive(Clone, Debug)]
pub struct HttpLimits {
    pub max_body_bytes: usize,
    pub max_response_bytes: usize,
    pub max_in_flight: usize,
    pub request_timeout: Duration,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 1024 * 1024,
            max_response_bytes: 1024 * 1024,
            max_in_flight: 128,
            request_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("procedure {0} has no HTTP binding")]
    MissingBinding(String),
    #[error("invalid literal route path: {0}")]
    InvalidPath(String),
    #[error("duplicate procedure name: {0}")]
    DuplicateName(String),
    #[error("duplicate HTTP route: {0}")]
    DuplicateRoute(String),
    #[error("unsupported HTTP method: {0}")]
    UnsupportedMethod(String),
    #[error(
        "HTTP limits must be greater than zero, concurrency within semaphore capacity, and timeout at most one day"
    )]
    InvalidLimits,
    #[error("GET, HEAD and OPTIONS procedures must accept unit (JSON null)")]
    InvalidInputBinding,
}

/// Registers the same erased procedure used by in-process calls and generators.
#[derive(Default)]
pub struct HttpAdapter {
    procedures: Vec<Arc<dyn ErasedProcedure>>,
    names: BTreeSet<String>,
    routes: BTreeSet<(String, String)>,
    limits: HttpLimits,
}

impl HttpAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn procedure(self, procedure: impl ErasedProcedure + 'static) -> Result<Self, BuildError> {
        self.shared_procedure(Arc::new(procedure))
    }

    /// Register an existing procedure shared with another transport adapter.
    pub fn shared_procedure(
        mut self,
        procedure: Arc<dyn ErasedProcedure>,
    ) -> Result<Self, BuildError> {
        let descriptor = procedure.descriptor();
        let binding = descriptor
            .http
            .as_ref()
            .ok_or_else(|| BuildError::MissingBinding(descriptor.name.clone()))?;
        validate_path(&binding.path)?;
        let method = binding.method.as_str().to_string();
        parse_method(&method)?;
        if matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS")
            && descriptor.input_schema.get("type").and_then(Value::as_str) != Some("null")
        {
            return Err(BuildError::InvalidInputBinding);
        }
        if self.names.contains(&descriptor.name) {
            return Err(BuildError::DuplicateName(descriptor.name.clone()));
        }
        let key = (method.clone(), binding.path.clone());
        // axum automatically mounts HEAD for GET; reject an overlapping explicit HEAD.
        let counterpart = match method.as_str() {
            "GET" => Some("HEAD"),
            "HEAD" => Some("GET"),
            _ => None,
        };
        if self.routes.contains(&key)
            || counterpart.is_some_and(|m| self.routes.contains(&(m.into(), binding.path.clone())))
        {
            return Err(BuildError::DuplicateRoute(format!(
                "{method} {}",
                binding.path
            )));
        }
        self.names.insert(descriptor.name.clone());
        self.routes.insert(key);
        self.procedures.push(procedure);
        Ok(self)
    }

    pub fn limits(mut self, limits: HttpLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn descriptors(&self) -> Vec<&ProcedureDescriptor> {
        self.procedures.iter().map(|p| p.descriptor()).collect()
    }

    pub fn router(self) -> Result<Router, BuildError> {
        let limits = self.limits;
        if limits.max_body_bytes == 0
            || limits.max_response_bytes == 0
            || limits.max_in_flight == 0
            || limits.max_in_flight > Semaphore::MAX_PERMITS
            || limits.request_timeout.is_zero()
            || limits.request_timeout > Duration::from_secs(86400)
        {
            return Err(BuildError::InvalidLimits);
        }
        let gate = Arc::new(Semaphore::new(limits.max_in_flight));
        let mut router = Router::new();
        for procedure in self.procedures {
            let binding = procedure
                .descriptor()
                .http
                .as_ref()
                .expect("validated at registration");
            let path = binding.path.clone();
            let method = parse_method(binding.method.as_str())?;
            let filter = MethodFilter::try_from(method)
                .map_err(|e| BuildError::UnsupportedMethod(e.to_string()))?;
            let limits = limits.clone();
            let gate = gate.clone();
            router = router.route(
                &path,
                on(filter, move |request: Request| {
                    serve(request, procedure.clone(), limits.clone(), gate.clone())
                }),
            );
        }
        Ok(router
            .fallback(|| async {
                framework_error(StatusCode::NOT_FOUND, "not_found", "Route not found", None)
            })
            .method_not_allowed_fallback(|| async {
                framework_error(
                    StatusCode::METHOD_NOT_ALLOWED,
                    "method_not_allowed",
                    "Method not allowed",
                    None,
                )
            }))
    }
}

fn validate_path(path: &str) -> Result<(), BuildError> {
    if !path.starts_with('/')
        || path.contains(['{', '}', '*', '?', '#'])
        || path.split('/').any(|segment| segment.starts_with(':'))
        || path.chars().any(char::is_control)
    {
        return Err(BuildError::InvalidPath(path.into()));
    }
    Ok(())
}

fn parse_method(method: &str) -> Result<Method, BuildError> {
    let parsed = Method::from_bytes(method.as_bytes())
        .map_err(|_| BuildError::UnsupportedMethod(method.into()))?;
    if !matches!(
        parsed,
        Method::GET
            | Method::HEAD
            | Method::POST
            | Method::PUT
            | Method::PATCH
            | Method::DELETE
            | Method::OPTIONS
    ) {
        return Err(BuildError::UnsupportedMethod(method.into()));
    }
    Ok(parsed)
}

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

async fn serve(
    request: Request,
    procedure: Arc<dyn ErasedProcedure>,
    limits: HttpLimits,
    gate: Arc<Semaphore>,
) -> Response {
    let request_id = format!("lw-{}", NEXT_REQUEST.fetch_add(1, Ordering::Relaxed));
    let Ok(_permit) = gate.try_acquire_owned() else {
        return framework_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "overloaded",
            "Server is at capacity",
            Some(&request_id),
        );
    };
    let result = tokio::time::timeout(limits.request_timeout, async {
        let (mut parts, body) = request.into_parts();
        // Bound every body, including bodies sent on GET/HEAD.
        let bytes = match to_bytes(body, limits.max_body_bytes).await {
            Ok(bytes) => bytes,
            Err(error) => {
                return if error
                    .source()
                    .is_some_and(|source| source.is::<http_body_util::LengthLimitError>())
                {
                    framework_error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "body_limit",
                        "Request body exceeds the configured limit",
                        Some(&request_id),
                    )
                } else {
                    framework_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_body",
                        "Request body could not be read",
                        Some(&request_id),
                    )
                };
            }
        };
        let input = if matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS) {
            if !bytes.is_empty() {
                return framework_error(
                    StatusCode::BAD_REQUEST,
                    "unexpected_body",
                    "This operation does not accept a request body",
                    Some(&request_id),
                );
            }
            Value::Null
        } else {
            parts
                .extensions
                .insert(DefaultBodyLimit::max(limits.max_body_bytes));
            let reconstructed = Request::from_parts(parts, Body::from(bytes));
            match Json::<Value>::from_request(reconstructed, &()).await {
                Ok(Json(value)) => value,
                Err(rejection) => {
                    let status = rejection.status();
                    let message = if status == StatusCode::UNSUPPORTED_MEDIA_TYPE {
                        "Expected application/json"
                    } else {
                        "Invalid JSON request body"
                    };
                    return framework_error(status, "invalid_json", message, Some(&request_id));
                }
            }
        };
        match procedure
            .invoke_json(Ctx::new(request_id.clone()), input)
            .await
        {
            Ok(value) => match serde_json::to_vec(&value) {
                Ok(bytes) if bytes.len() <= limits.max_response_bytes => (
                    [
                        (header::CONTENT_TYPE, "application/json"),
                        (
                            header::HeaderName::from_static("x-request-id"),
                            request_id.as_str(),
                        ),
                    ],
                    bytes,
                )
                    .into_response(),
                _ => framework_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "response_limit",
                    "Response could not be encoded within the configured limit",
                    Some(&request_id),
                ),
            },
            Err(mut error) => {
                error.request_id = Some(request_id.clone());
                let status = StatusCode::from_u16(error.code.http_status())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                match serde_json::to_vec(&error) {
                    Ok(bytes) if bytes.len() <= limits.max_response_bytes => (
                        status,
                        [
                            (header::CONTENT_TYPE, "application/json"),
                            (
                                header::HeaderName::from_static("x-request-id"),
                                request_id.as_str(),
                            ),
                        ],
                        bytes,
                    )
                        .into_response(),
                    _ => framework_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "response_limit",
                        "Response could not be encoded within the configured limit",
                        Some(&request_id),
                    ),
                }
            }
        }
    })
    .await;
    match result {
        Ok(response) => response,
        Err(_) => framework_error(
            StatusCode::GATEWAY_TIMEOUT,
            "deadline_exceeded",
            "Request deadline exceeded",
            Some(&request_id),
        ),
    }
}

fn framework_error(
    status: StatusCode,
    reason: &str,
    message: &str,
    request_id: Option<&str>,
) -> Response {
    // Only fixed framework messages are exposed; extractor internals never cross the boundary.
    let code = match status {
        StatusCode::NOT_FOUND => RpcCode::NotFound,
        StatusCode::SERVICE_UNAVAILABLE => RpcCode::Unavailable,
        StatusCode::GATEWAY_TIMEOUT => RpcCode::DeadlineExceeded,
        status if status.is_server_error() => RpcCode::Internal,
        _ => RpcCode::InvalidArgument,
    };
    let mut error = ApiError::new(code, reason, message);
    error.request_id = request_id.map(str::to_string);
    let mut response = (status, Json(error)).into_response();
    if let Some(id) = request_id.and_then(|id| header::HeaderValue::from_str(id).ok()) {
        response.headers_mut().insert("x-request-id", id);
    }
    response
}

#[cfg(test)]
mod tests;
