//! Opt-in protobuf RPC adapters invoking the same typed core procedures as REST.
//!
//! The pinned Connect runtime owns Connect, gRPC, and gRPC-Web framing. This
//! crate supplies explicit message conversions and canonical application errors.

pub mod proto {
    connectrpc::include_generated!();
}

pub use connectrpc;

/// Compiled schema for external code generation and gRPC tools.
pub const FILE_DESCRIPTOR_SET: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/bills.descriptor.bin"));

use connectrpc::{
    ConnectError, ErrorCode, ErrorDetail, RequestContext, Response, ServiceRequest, ServiceResult,
};
use loams_web_core::{ApiError, Ctx, Procedure, RpcCode};
use proto::loams::web::bills::v1::{
    self as bills, ProcedureErrorDetail, SplitRequest, SplitResponse,
};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

type Decode<I> = dyn Fn(SplitRequest) -> Result<I, ApiError> + Send + Sync;
type Encode<O> = dyn Fn(O) -> Result<SplitResponse, ApiError> + Send + Sync;

/// Explicit adapter between a typed procedure and the opt-in bills protobuf API.
/// Conversions are application code; serde shapes never acquire automatic tags.
pub struct BillSplitService<I, O> {
    procedure: Arc<Procedure<I, O>>,
    decode: Arc<Decode<I>>,
    encode: Arc<Encode<O>>,
    admission: tokio::sync::Semaphore,
}

impl<I, O> BillSplitService<I, O>
where
    I: Serialize + DeserializeOwned + JsonSchema + Send + 'static,
    O: Serialize + JsonSchema + Send + 'static,
{
    pub fn new(
        procedure: Arc<Procedure<I, O>>,
        decode: impl Fn(SplitRequest) -> Result<I, ApiError> + Send + Sync + 'static,
        encode: impl Fn(O) -> Result<SplitResponse, ApiError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            procedure,
            decode: Arc::new(decode),
            encode: Arc::new(encode),
            admission: tokio::sync::Semaphore::new(128),
        }
    }

    /// Register generated service metadata and dispatch glue into the runtime.
    pub fn into_router(self) -> connectrpc::Router {
        connectrpc::Router::new().add_service(Arc::new(self))
    }
}

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

impl<I, O> bills::BillSplitService for BillSplitService<I, O>
where
    I: Serialize + DeserializeOwned + JsonSchema + Send + 'static,
    O: Serialize + JsonSchema + Send + 'static,
{
    async fn split<'a>(
        &'a self,
        context: RequestContext,
        request: ServiceRequest<'_, SplitRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<SplitResponse> + Send + use<'a, I, O>> {
        let _permit = self
            .admission
            .try_acquire()
            .map_err(|_| ConnectError::resource_exhausted("RPC handler is at capacity"))?;
        // Auth middleware may supply a validated context as an extension. Never
        // construct an identity from a caller-controlled header here.
        let ctx = context
            .extensions()
            .get::<Ctx>()
            .cloned()
            .unwrap_or_else(|| {
                Ctx::new(format!(
                    "lw-rpc-{}",
                    NEXT_REQUEST.fetch_add(1, Ordering::Relaxed)
                ))
            });
        let request_id = ctx.request_id.clone();
        let input = (self.decode)(request.to_owned_message())
            .map_err(|_| ConnectError::invalid_argument("RPC input conversion failed"))?;
        let output = self
            .procedure
            .invoke(ctx, input)
            .await
            .map_err(project_error)?;
        let output = (self.encode)(output)
            .map_err(|_| ConnectError::internal("RPC output conversion failed"))?;
        let mut response = Response::new(output);
        if let Ok(id) = http::HeaderValue::from_str(&request_id) {
            response.headers.insert("x-request-id", id);
        }
        Ok(response)
    }
}

/// Apply the preview's bounded unary policy to a registered runtime router.
/// Protocol decoding, compression, deadlines and trailers remain runtime-owned.
pub fn service(router: connectrpc::Router) -> connectrpc::ConnectRpcService {
    connectrpc::ConnectRpcService::new(router)
        .with_limits(
            connectrpc::Limits::default()
                .with_max_request_body_size(1024 * 1024)
                .with_max_message_size(1024 * 1024),
        )
        .with_deadline_policy(
            connectrpc::DeadlinePolicy::new()
                .with_default_timeout(Duration::from_secs(30))
                .with_max(Duration::from_secs(30)),
        )
}

/// Project canonical application failures to protocol status and typed details.
pub fn project_error(mut error: ApiError) -> ConnectError {
    let details = serde_json::to_vec(&error.details).unwrap_or_default();
    if error.message.len() > 4096
        || error.reason.len() > 1024
        || details.len() > 65536
        || error.request_id.as_ref().is_some_and(|id| id.len() > 1024)
    {
        error = ApiError::internal();
    }
    let code = match error.code {
        RpcCode::Cancelled => ErrorCode::Canceled,
        RpcCode::Unknown => ErrorCode::Unknown,
        RpcCode::InvalidArgument => ErrorCode::InvalidArgument,
        RpcCode::DeadlineExceeded => ErrorCode::DeadlineExceeded,
        RpcCode::NotFound => ErrorCode::NotFound,
        RpcCode::AlreadyExists => ErrorCode::AlreadyExists,
        RpcCode::PermissionDenied => ErrorCode::PermissionDenied,
        RpcCode::ResourceExhausted => ErrorCode::ResourceExhausted,
        RpcCode::FailedPrecondition => ErrorCode::FailedPrecondition,
        RpcCode::Aborted => ErrorCode::Aborted,
        RpcCode::OutOfRange => ErrorCode::OutOfRange,
        RpcCode::Unimplemented => ErrorCode::Unimplemented,
        RpcCode::Internal => ErrorCode::Internal,
        RpcCode::Unavailable => ErrorCode::Unavailable,
        RpcCode::DataLoss => ErrorCode::DataLoss,
        RpcCode::Unauthenticated => ErrorCode::Unauthenticated,
    };
    let detail = ProcedureErrorDetail {
        reason: error.reason,
        request_id: error.request_id.clone().unwrap_or_default(),
        details_json: serde_json::to_vec(&error.details).unwrap_or_default(),
        retry_after_millis: error.retry.map(|retry| retry.after_millis),
        ..Default::default()
    };
    let mut result = ConnectError::new(code, error.message).with_detail(ErrorDetail::from_message(
        "loams.web.bills.v1.ProcedureErrorDetail",
        &detail,
    ));
    if let Some(id) = error
        .request_id
        .and_then(|id| http::HeaderValue::from_str(&id).ok())
    {
        result.response_headers_mut().insert("x-request-id", id);
    }
    result
}

#[cfg(test)]
mod tests;
