//! Request-scoped integration with the generated Loams Rust SDK.
//!
//! This crate delegates credentials, retries, generated messages and consistency
//! merging to the SDK. Authentication verification belongs to a supported host
//! provider; no identity header or locally invented token verifier is supplied.

pub use loams_sdk as sdk;

use connectrpc::client::HttpClient;
use loams_web_core::{ApiError, Ctx, Dependency, Principal, RpcCode};
use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
};

/// The engine credential flow a host provider actually verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialSource {
    ApiKey,
    Oidc,
    Tailscale,
    AuthDisabled,
}

/// Result of trusted host verification, not a claim parsed from request headers.
/// Construct this only after verifying credentials through the engine's supported
/// auth boundary, or applying its explicitly configured auth-disabled policy.
pub struct VerifiedIdentity {
    principal: Principal,
    source: CredentialSource,
    expires_unix_seconds: Option<u64>,
    outbound_auth: Option<Arc<dyn sdk::TokenSource>>,
}
impl VerifiedIdentity {
    pub fn new(principal: Principal, source: CredentialSource) -> Self {
        Self {
            principal,
            source,
            expires_unix_seconds: None,
            outbound_auth: None,
        }
    }
    pub fn with_expiry(mut self, expires_unix_seconds: u64) -> Self {
        self.expires_unix_seconds = Some(expires_unix_seconds);
        self
    }
    /// Credentials verified/authorized for outbound engine calls. They are never
    /// installed on the shared platform client or copied into core context.
    pub fn with_outbound_auth(mut self, source: Arc<dyn sdk::TokenSource>) -> Self {
        self.outbound_auth = Some(source);
        self
    }
}
impl fmt::Debug for VerifiedIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedIdentity")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

pub type VerificationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<VerifiedIdentity, ApiError>> + Send + 'a>>;
/// The host owns `Request`, including trusted connection/ingress evidence. There
/// is intentionally no permissive implementation or client identity-header path.
pub trait AuthProvider<Request>: Send + Sync {
    fn verify<'a>(&'a self, request: &'a Request) -> VerificationFuture<'a>;
}

fn context_marker() -> &'static Dependency<()> {
    static MARKER: OnceLock<Dependency<()>> = OnceLock::new();
    MARKER.get_or_init(|| Dependency::new(|_| async { Ok(()) }))
}

/// A core context bound to one verified request. Clones retain its identity;
/// constructing a new request with identical text IDs does not share it.
#[derive(Clone)]
pub struct VerifiedCtx {
    ctx: Ctx,
    marker: Arc<()>,
    identity: Arc<VerifiedIdentity>,
}
impl VerifiedCtx {
    pub async fn from_verified(
        identity: VerifiedIdentity,
        request_id: impl Into<String>,
        now_unix_seconds: u64,
    ) -> Result<Self, ApiError> {
        if identity.principal.namespace.is_empty() || identity.principal.subject.is_empty() {
            return Err(ApiError::new(
                RpcCode::Unauthenticated,
                "invalid_identity",
                "A verified namespace and subject are required",
            ));
        }
        if identity
            .expires_unix_seconds
            .is_some_and(|expiry| expiry <= now_unix_seconds)
        {
            return Err(ApiError::new(
                RpcCode::Unauthenticated,
                "identity_expired",
                "Verified identity has expired",
            ));
        }
        let ctx = Ctx::new(request_id).with_principal(identity.principal.clone());
        let marker = context_marker().resolve(&ctx).await?;
        Ok(Self {
            ctx,
            marker,
            identity: Arc::new(identity),
        })
    }
    pub fn ctx(&self) -> &Ctx {
        &self.ctx
    }
    pub fn principal(&self) -> &Principal {
        &self.identity.principal
    }
    pub fn source(&self) -> CredentialSource {
        self.identity.source
    }
    async fn matches(&self, ctx: &Ctx) -> Result<(), ApiError> {
        let marker = context_marker().resolve(ctx).await?;
        if !Arc::ptr_eq(&self.marker, &marker)
            || ctx.principal.as_ref() != Some(self.principal())
            || ctx.request_id != self.ctx.request_id
        {
            return Err(ApiError::new(
                RpcCode::FailedPrecondition,
                "request_scope_mismatch",
                "Platform state belongs to another verified request",
            ));
        }
        Ok(())
    }
}
impl fmt::Debug for VerifiedCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedCtx")
            .field("ctx", &self.ctx)
            .field("source", &self.identity.source)
            .finish_non_exhaustive()
    }
}

pub async fn authenticate<P, R>(
    provider: &P,
    request: &R,
    request_id: impl Into<String>,
    now_unix_seconds: u64,
) -> Result<VerifiedCtx, ApiError>
where
    P: AuthProvider<R> + ?Sized,
{
    VerifiedCtx::from_verified(
        provider.verify(request).await?,
        request_id,
        now_unix_seconds,
    )
    .await
}

/// App service-package scopes from SDK runtime contract R4/R11. A graph token
/// never becomes an implicit collection/SQL consistency or a Live resume cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ServicePackage {
    Collection,
    Sql,
    Graph,
    Stream,
}
impl ServicePackage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Collection => "loams.collection.v1",
            Self::Sql => "loams.sql.v1",
            Self::Graph => "loams.graph.v1",
            Self::Stream => "loams.stream.v1",
        }
    }
}

/// An inbound token accepted by the SDK's exact integer parser. Header parsing
/// and deciding which package a header represents remain explicit host policy.
#[derive(Clone, PartialEq, Eq)]
pub struct ScopedToken {
    package: ServicePackage,
    token: sdk::ConsistencyToken,
}
impl ScopedToken {
    pub fn parse(package: ServicePackage, text: &str) -> Result<Self, ApiError> {
        sdk::ConsistencyToken::parse(text)
            .map(|token| Self { package, token })
            .map_err(|_| {
                ApiError::invalid_argument(
                    "invalid_consistency_token",
                    "Consistency token is invalid",
                )
            })
    }
    pub fn package(&self) -> ServicePackage {
        self.package
    }
    pub fn token(&self) -> &sdk::ConsistencyToken {
        &self.token
    }
}
impl fmt::Debug for ScopedToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScopedToken")
            .field("package", &self.package)
            .finish_non_exhaustive()
    }
}

/// A shared transport with pristine SDK configuration: no credentials, namespace
/// or process-global consistency store. Per-request clients own those settings.
#[derive(Clone)]
pub struct Platform<T> {
    sdk: sdk::Loams<T>,
}
impl Platform<HttpClient> {
    pub fn connect(options: sdk::TransportOptions) -> Result<Self, ApiError> {
        sdk::Loams::connect(options)
            .map(|sdk| Self { sdk })
            .map_err(|_| {
                ApiError::invalid_argument(
                    "invalid_engine_endpoint",
                    "Engine endpoint configuration is invalid",
                )
            })
    }
}
impl<T: Clone> Platform<T> {
    pub fn with_transport(transport: T, options: sdk::TransportOptions) -> Result<Self, ApiError> {
        sdk::Loams::with_transport(transport, options)
            .map(|sdk| Self { sdk })
            .map_err(|_| {
                ApiError::invalid_argument(
                    "invalid_engine_endpoint",
                    "Engine endpoint configuration is invalid",
                )
            })
    }
    pub fn request(
        &self,
        ctx: &VerifiedCtx,
        seeds: impl IntoIterator<Item = ScopedToken>,
    ) -> RequestClient<T> {
        let session = Arc::new(sdk::ConsistencySession::new());
        for seed in seeds {
            session.record_in(Some(&seed.token.to_string()), seed.package.as_str());
        }
        let mut client = self
            .sdk
            .clone()
            .session_consistency(false)
            .with_namespace(ctx.principal().namespace.clone());
        if let Some(source) = &ctx.identity.outbound_auth {
            client = client.with_auth(source.clone());
        }
        RequestClient {
            ctx: ctx.clone(),
            sdk: client,
            session,
        }
    }
}
impl<T> fmt::Debug for Platform<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Platform").finish_non_exhaustive()
    }
}

/// One request's SDK client and package-scoped session. SDK response recording
/// merges tokens, including successful writes followed by a later safe error.
#[derive(Clone)]
pub struct RequestClient<T> {
    ctx: VerifiedCtx,
    sdk: sdk::Loams<T>,
    session: Arc<sdk::ConsistencySession>,
}
impl<T> RequestClient<T> {
    /// Keep explicit consistency unchanged and install this request's session.
    /// Supplying another session is rejected, rather than sharing caller state.
    pub async fn options(
        &self,
        ctx: &Ctx,
        options: sdk::CallOptionsOverrides,
    ) -> Result<sdk::CallOptionsOverrides, ApiError> {
        if options.consistency.is_some() {
            return Err(ApiError::new(
                RpcCode::FailedPrecondition,
                "explicit_consistency_requires_call",
                "Use call to preserve explicit consistency and response tokens",
            ));
        }
        self.prepare_options(ctx, options).await
    }
    async fn prepare_options(
        &self,
        ctx: &Ctx,
        mut options: sdk::CallOptionsOverrides,
    ) -> Result<sdk::CallOptionsOverrides, ApiError> {
        self.ctx.matches(ctx).await?;
        if options.session.is_some() {
            return Err(ApiError::new(
                RpcCode::FailedPrecondition,
                "session_override_not_allowed",
                "Use the verified request's consistency session",
            ));
        }
        if let Some(sdk::Consistency::AtLeast(token)) = &options.consistency {
            sdk::ConsistencyToken::parse(token).map_err(|_| {
                ApiError::invalid_argument(
                    "invalid_consistency_token",
                    "Consistency token is invalid",
                )
            })?;
        }
        options.session = Some(self.session.clone());
        Ok(options)
    }
    /// Select the single package represented by an ordinary response header.
    /// Call after all outbound work, even when the handler returns a safe error.
    /// Streaming callers must carry later changes in their own typed events.
    pub async fn response_token(
        &self,
        ctx: &Ctx,
        package: ServicePackage,
    ) -> Result<Option<String>, ApiError> {
        self.ctx.matches(ctx).await?;
        Ok(self.session.current_in(package.as_str()))
    }
    pub fn rejected_response_tokens(&self) -> u64 {
        self.session.rejected()
    }
}
impl<T: Clone + Send + Sync + 'static> RequestClient<T> {
    /// Invoke a generated SDK app method after checking its request namespace.
    /// The SDK fills omitted consistency, applies explicit precedence, excludes
    /// graph implicit consistency, and records package-scoped response tokens.
    pub async fn call<R, O, F, Fut>(
        &self,
        ctx: &Ctx,
        mut request: R,
        options: sdk::CallOptionsOverrides,
        invoke: F,
    ) -> Result<O, ApiError>
    where
        R: sdk::AppRequest + Send + 'static,
        O: Send + 'static,
        F: FnOnce(sdk::Loams<T>, R, sdk::CallOptionsOverrides) -> Fut + Send + 'static,
        Fut: Future<Output = Result<O, sdk::LoamsError>> + 'static,
    {
        let explicit = options.consistency.is_some();
        let mut options = self.prepare_options(ctx, options).await?;
        if let Some(namespace) = request.namespace_mut() {
            if namespace.is_empty() {
                self.ctx.principal().namespace.clone_into(namespace);
            } else if namespace != &self.ctx.principal().namespace {
                return Err(ApiError::new(
                    RpcCode::PermissionDenied,
                    "namespace_scope_mismatch",
                    "Request namespace does not match the verified principal",
                ));
            }
        }
        let call_session = explicit.then(|| Arc::new(sdk::ConsistencySession::new()));
        if let Some(session) = &call_session {
            options.session = Some(session.clone());
        }
        let client = self.sdk.clone();
        let destination = self.session.clone();
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| ApiError::internal())?;
        // SDK 0.1.2's borrowed app-module futures do not currently satisfy Send
        // when composed inside a server handler. Create and drive the future on
        // one blocking worker; only owned inputs and the completed result cross
        // the thread boundary. Transport IO still uses the original Tokio runtime.
        tokio::task::spawn_blocking(move || {
            runtime.block_on(async move {
                let result = invoke(client, request, options).await;
                if let Some(session) = call_session {
                    for package in [
                        ServicePackage::Collection,
                        ServicePackage::Sql,
                        ServicePackage::Graph,
                        ServicePackage::Stream,
                    ] {
                        destination.record_in(
                            session.current_in(package.as_str()).as_deref(),
                            package.as_str(),
                        );
                    }
                }
                result.map_err(map_sdk_error)
            })
        })
        .await
        .map_err(|_| ApiError::internal())?
    }
}
impl<T> fmt::Debug for RequestClient<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestClient")
            .field("ctx", &self.ctx)
            .finish_non_exhaustive()
    }
}

/// Keep canonical code and bounded stable reasons, while dropping upstream
/// exception messages, hints and metadata that may contain application secrets.
pub fn map_sdk_error(error: sdk::LoamsError) -> ApiError {
    use sdk::Code as C;
    let code = match error.code {
        C::Canceled => RpcCode::Cancelled,
        C::Unknown => RpcCode::Unknown,
        C::InvalidArgument => RpcCode::InvalidArgument,
        C::DeadlineExceeded => RpcCode::DeadlineExceeded,
        C::NotFound => RpcCode::NotFound,
        C::AlreadyExists => RpcCode::AlreadyExists,
        C::PermissionDenied => RpcCode::PermissionDenied,
        C::ResourceExhausted => RpcCode::ResourceExhausted,
        C::FailedPrecondition => RpcCode::FailedPrecondition,
        C::Aborted => RpcCode::Aborted,
        C::OutOfRange => RpcCode::OutOfRange,
        C::Unimplemented => RpcCode::Unimplemented,
        C::Internal => RpcCode::Internal,
        C::Unavailable => RpcCode::Unavailable,
        C::DataLoss => RpcCode::DataLoss,
        C::Unauthenticated => RpcCode::Unauthenticated,
        _ => RpcCode::Unknown,
    };
    let reason = error
        .reason()
        .map(|reason| reason.as_str())
        .or_else(|| error.unknown_reason())
        .filter(|reason| {
            !reason.is_empty()
                && reason.len() <= 96
                && reason
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
        .unwrap_or("loams_upstream_error");
    ApiError::new(code, reason, "The engine call failed")
}
