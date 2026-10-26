use connectrpc::client::{ClientBody, ClientTransport};
use http_body_util::{BodyExt, Full};
use loams_web_core::{Ctx, Principal, Procedure, RpcCode};
use loams_web_platform::{
    AuthProvider, CredentialSource, Platform, ScopedToken, ServicePackage as Package,
    VerificationFuture, VerifiedCtx, VerifiedIdentity, authenticate, sdk,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

fn principal(namespace: &str, subject: &str) -> Principal {
    Principal {
        namespace: namespace.into(),
        subject: subject.into(),
        scopes: vec!["read".into(), "write".into()],
    }
}
async fn verified(namespace: &str, subject: &str, request_id: &str) -> VerifiedCtx {
    VerifiedCtx::from_verified(
        VerifiedIdentity::new(principal(namespace, subject), CredentialSource::ApiKey),
        request_id,
        100,
    )
    .await
    .unwrap()
}

#[derive(Clone, Default)]
struct Recording {
    seen: Arc<Mutex<Vec<Observed>>>,
    replies: Arc<Mutex<VecDeque<Reply>>>,
}
#[derive(Clone)]
struct Observed {
    path: String,
    headers: http::HeaderMap,
    body: Value,
}
struct Reply {
    status: u16,
    body: Value,
    token: Option<String>,
}
impl Recording {
    fn queue(&self, body: Value, token: Option<&str>) {
        self.replies.lock().unwrap().push_back(Reply {
            status: 200,
            body,
            token: token.map(str::to_owned),
        });
    }
    fn error(&self, body: Value) {
        self.replies.lock().unwrap().push_back(Reply {
            status: 503,
            body,
            token: None,
        });
    }
}
impl ClientTransport for Recording {
    type ResponseBody = Full<bytes::Bytes>;
    type Error = std::io::Error;
    fn send(
        &self,
        request: http::Request<ClientBody>,
    ) -> connectrpc::client::BoxFuture<
        'static,
        Result<http::Response<Self::ResponseBody>, Self::Error>,
    > {
        let seen = self.seen.clone();
        let reply = self.replies.lock().unwrap().pop_front().unwrap_or(Reply {
            status: 200,
            body: json!({}),
            token: None,
        });
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let body = body
                .collect()
                .await
                .map_err(std::io::Error::other)?
                .to_bytes();
            let body = serde_json::from_slice(&body).map_err(std::io::Error::other)?;
            seen.lock().unwrap().push(Observed {
                path: parts.uri.path().into(),
                headers: parts.headers,
                body,
            });
            let mut response = http::Response::builder()
                .status(reply.status)
                .header("content-type", "application/json");
            if let Some(token) = reply.token {
                response = response.header("loams-consistency-token", token);
            }
            response
                .body(Full::new(bytes::Bytes::from(reply.body.to_string())))
                .map_err(std::io::Error::other)
        })
    }
}
fn platform(recording: Recording) -> Platform<Recording> {
    Platform::with_transport(
        recording,
        sdk::TransportOptions::new("http://fixture.invalid").json(true),
    )
    .unwrap()
}
fn seed(package: Package, text: &str) -> ScopedToken {
    ScopedToken::parse(package, text).unwrap()
}

#[tokio::test]
async fn upstream_token_vectors_run_through_request_session_adapter_without_rounding() {
    let corpus: Value = serde_json::from_str(include_str!("fixtures/tokens.json")).unwrap();
    for vector in corpus["parse"].as_array().unwrap() {
        let text = vector["text"].as_str().unwrap();
        assert_eq!(
            ScopedToken::parse(Package::Collection, text)
                .unwrap()
                .token()
                .to_string(),
            text
        );
    }
    let ctx = verified("alpha", "alice", "vectors").await;
    let platform = platform(Recording::default());
    for vector in corpus["merge"].as_array().unwrap() {
        let request = platform.request(
            &ctx,
            [
                seed(Package::Collection, vector["a"].as_str().unwrap()),
                seed(Package::Collection, vector["b"].as_str().unwrap()),
            ],
        );
        assert_eq!(
            request
                .response_token(ctx.ctx(), Package::Collection)
                .await
                .unwrap()
                .as_deref(),
            vector["expect"].as_str()
        );
        assert_eq!(
            request
                .response_token(ctx.ctx(), Package::Sql)
                .await
                .unwrap(),
            None
        );
    }
    for text in corpus["malformed"].as_array().unwrap() {
        assert_eq!(
            ScopedToken::parse(Package::Collection, text.as_str().unwrap())
                .unwrap_err()
                .reason,
            "invalid_consistency_token"
        );
    }
}

#[tokio::test]
async fn context_identity_namespace_and_options_prevent_accidental_cross_request_reuse() {
    let ctx = verified("alpha", "alice", "same-text-id").await;
    let other = verified("alpha", "alice", "same-text-id").await;
    let recording = Recording::default();
    let request =
        platform(recording.clone()).request(&ctx, [seed(Package::Collection, "v1:s1/p0@8")]);
    let reconstructed = Ctx::new("same-text-id").with_principal(principal("alpha", "alice"));
    assert_eq!(
        request
            .options(&reconstructed, sdk::CallOptionsOverrides::new())
            .await
            .unwrap_err()
            .reason,
        "request_scope_mismatch"
    );
    assert_eq!(
        request
            .options(other.ctx(), sdk::CallOptionsOverrides::new())
            .await
            .unwrap_err()
            .reason,
        "request_scope_mismatch"
    );
    let mut changed = ctx.ctx().clone();
    changed.principal.as_mut().unwrap().namespace = "other".into();
    assert_eq!(
        request
            .response_token(&changed, Package::Collection)
            .await
            .unwrap_err()
            .reason,
        "request_scope_mismatch"
    );
    let foreign = sdk::collection_v1::GetDocumentsRequest {
        namespace: "other".into(),
        ..Default::default()
    };
    let error=request.call(ctx.ctx(),foreign,sdk::CallOptionsOverrides::new(),|client,message,options|async move{client.documents().get(message,options).await}).await.unwrap_err();
    assert_eq!(error.reason, "namespace_scope_mismatch");
    assert!(recording.seen.lock().unwrap().is_empty());
    let options =
        sdk::CallOptionsOverrides::new().session(Arc::new(sdk::ConsistencySession::new()));
    assert_eq!(
        request
            .options(ctx.ctx(), options)
            .await
            .unwrap_err()
            .reason,
        "session_override_not_allowed"
    );
}

#[tokio::test]
async fn generated_sdk_write_read_sql_and_graph_run_inside_one_core_handler() {
    let ctx = verified("alpha", "alice", "handler").await;
    let recording = Recording::default();
    recording.queue(
        json!({"token":"v1:s7/p0@9007199254740993"}),
        Some("v1:s9/p1@2"),
    );
    recording.queue(json!({"readToken":"v1:s7/p0@9007199254740994"}), None);
    recording.queue(json!({"consistencyToken":"v1:s20/p0@5"}), None);
    recording.queue(json!({}), None);
    let request = platform(recording.clone()).request(
        &ctx,
        [
            seed(Package::Collection, "v1:s1/p0@3"),
            seed(Package::Sql, "v1:s2/p0@4"),
            seed(Package::Graph, "v1:s3/p0@5"),
        ],
    );
    let work = request.clone();
    let handler=Procedure::<(),String>::builder("sdk.handler").handler(move|ctx,_|{
        let work=work.clone();
        async move {
            work.call(&ctx,sdk::collection_v1::WriteDocumentsRequest{collection:"notes".into(),..Default::default()},sdk::CallOptionsOverrides::new(),|sdk,request,options|async move{sdk.documents().write(request,options).await}).await?;
            work.call(&ctx,sdk::collection_v1::GetDocumentsRequest{collection:"notes".into(),..Default::default()},sdk::CallOptionsOverrides::new(),|sdk,request,options|async move{sdk.documents().get(request,options).await}).await?;
            work.call(&ctx,sdk::sql_v1::QueryRequest{sql:"select 1".into(),..Default::default()},sdk::CallOptionsOverrides::new(),|sdk,request,options|async move{sdk.sql().query(request,options).await}).await?;
            work.call(&ctx,sdk::graph_v1::ExecuteRequest::default(),sdk::CallOptionsOverrides::new(),|sdk,request,options|async move{sdk.graph().query(request,options).await}).await?;
            Ok(work.response_token(&ctx,Package::Collection).await?.unwrap())
        }
    }).build().unwrap();
    assert_eq!(
        handler
            .invoke_json(ctx.ctx().clone(), json!(null))
            .await
            .unwrap(),
        json!("v1:s1/p0@3,s7/p0@9007199254740994,s9/p1@2")
    );
    assert_eq!(
        request
            .response_token(ctx.ctx(), Package::Sql)
            .await
            .unwrap()
            .as_deref(),
        Some("v1:s2/p0@4,s20/p0@5")
    );
    assert_eq!(
        request
            .response_token(ctx.ctx(), Package::Graph)
            .await
            .unwrap()
            .as_deref(),
        Some("v1:s3/p0@5")
    );
    let seen = recording.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 4);
    assert!(seen[0].path.ends_with("DocumentService/WriteDocuments"));
    assert_eq!(seen[0].body["namespace"], "alpha");
    assert!(!seen[0].body["idempotencyKey"].as_str().unwrap().is_empty());
    assert_eq!(
        seen[1].body["consistency"]["atLeast"],
        "v1:s1/p0@3,s7/p0@9007199254740993,s9/p1@2"
    );
    assert_eq!(seen[2].body["consistency"]["atLeast"], "v1:s2/p0@4");
    assert!(seen[3].body.get("consistency").is_none());
    for call in seen.iter() {
        assert!(!call.headers.contains_key("loams-consistency"));
    }
}

#[tokio::test]
async fn explicit_call_and_message_consistency_win_without_losing_response_tokens() {
    let ctx = verified("alpha", "alice", "explicit").await;
    let recording = Recording::default();
    recording.queue(json!({"readToken":"v1:s1/p0@101"}), None);
    recording.queue(json!({}), None);
    recording.queue(json!({}), None);
    recording.queue(json!({}), None);
    let request =
        platform(recording.clone()).request(&ctx, [seed(Package::Collection, "v1:s1/p0@100")]);
    for consistency in [
        sdk::Consistency::Eventual,
        sdk::Consistency::Strong,
        sdk::Consistency::AtLeast("v1:s4/p0@8".into()),
    ] {
        request
            .call(
                ctx.ctx(),
                sdk::collection_v1::GetDocumentsRequest::default(),
                sdk::CallOptionsOverrides::new().consistency(consistency),
                |sdk, message, options| async move { sdk.documents().get(message, options).await },
            )
            .await
            .unwrap();
    }
    let explicit: sdk::collection_v1::GetDocumentsRequest =
        serde_json::from_value(json!({"consistency":{"atLeast":"v1:s5/p0@9"}})).unwrap();
    request
        .call(
            ctx.ctx(),
            explicit,
            sdk::CallOptionsOverrides::new(),
            |sdk, message, options| async move { sdk.documents().get(message, options).await },
        )
        .await
        .unwrap();
    let seen = recording.seen.lock().unwrap().clone();
    assert_eq!(seen[0].headers["loams-consistency"], "eventual");
    assert!(!seen[1].headers.contains_key("loams-consistency"));
    assert_eq!(seen[2].headers["loams-consistency"], "at_least:v1:s4/p0@8");
    for call in &seen[..3] {
        assert!(call.body.get("consistency").is_none());
    }
    assert_eq!(seen[3].body["consistency"]["atLeast"], "v1:s5/p0@9");
    assert_eq!(
        request
            .response_token(ctx.ctx(), Package::Collection)
            .await
            .unwrap()
            .as_deref(),
        Some("v1:s1/p0@101")
    );
}

#[tokio::test]
async fn separate_principals_keep_outbound_credentials_and_tokens_isolated() {
    let make = |namespace: &str, subject: &str, token: &str| {
        VerifiedIdentity::new(principal(namespace, subject), CredentialSource::ApiKey)
            .with_outbound_auth(Arc::new(sdk::ApiKey::new(token).unwrap()))
    };
    let alice = VerifiedCtx::from_verified(make("alpha", "alice", "secret-alice"), "alice", 100)
        .await
        .unwrap();
    let bob = VerifiedCtx::from_verified(make("beta", "bob", "secret-bob"), "bob", 100)
        .await
        .unwrap();
    let recording = Recording::default();
    let platform = platform(recording.clone());
    let first = platform.request(&alice, [seed(Package::Collection, "v1:s1/p0@10")]);
    let second = platform.request(&bob, [seed(Package::Collection, "v1:s2/p0@20")]);
    let (one, two) = tokio::join!(
        first.call(
            alice.ctx(),
            sdk::collection_v1::GetDocumentsRequest::default(),
            sdk::CallOptionsOverrides::new(),
            |sdk, message, options| async move { sdk.documents().get(message, options).await }
        ),
        second.call(
            bob.ctx(),
            sdk::collection_v1::GetDocumentsRequest::default(),
            sdk::CallOptionsOverrides::new(),
            |sdk, message, options| async move { sdk.documents().get(message, options).await }
        )
    );
    one.unwrap();
    two.unwrap();
    let seen = recording.seen.lock().unwrap();
    for call in seen.iter() {
        match call.body["namespace"].as_str().unwrap() {
            "alpha" => {
                assert_eq!(call.headers["authorization"], "Bearer secret-alice");
                assert_eq!(call.body["consistency"]["atLeast"], "v1:s1/p0@10");
            }
            "beta" => {
                assert_eq!(call.headers["authorization"], "Bearer secret-bob");
                assert_eq!(call.body["consistency"]["atLeast"], "v1:s2/p0@20");
            }
            other => panic!("unexpected namespace {other}"),
        }
    }
    assert!(!format!("{first:?} {second:?} {alice:?} {bob:?}").contains("secret"));
}

#[tokio::test]
async fn successful_write_token_remains_after_later_safe_sdk_error() {
    let ctx = verified("alpha", "alice", "after-write").await;
    let recording = Recording::default();
    recording.queue(json!({"token":"v1:s7/p0@11"}), None);
    recording.error(json!({"code":"unavailable","message":"secret-upstream-message"}));
    let request = platform(recording).request(&ctx, []);
    request
        .call(
            ctx.ctx(),
            sdk::collection_v1::WriteDocumentsRequest::default(),
            sdk::CallOptionsOverrides::new(),
            |sdk, message, options| async move { sdk.documents().write(message, options).await },
        )
        .await
        .unwrap();
    let error = request
        .call(
            ctx.ctx(),
            sdk::collection_v1::GetDocumentsRequest::default(),
            sdk::CallOptionsOverrides::new().max_retries(0),
            |sdk, message, options| async move { sdk.documents().get(message, options).await },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, RpcCode::Unavailable);
    assert!(!serde_json::to_string(&error).unwrap().contains("secret"));
    assert_eq!(
        request
            .response_token(ctx.ctx(), Package::Collection)
            .await
            .unwrap()
            .as_deref(),
        Some("v1:s7/p0@11")
    );
}

struct TrustedProvider;
impl AuthProvider<()> for TrustedProvider {
    fn verify<'a>(&'a self, _: &'a ()) -> VerificationFuture<'a> {
        Box::pin(async {
            Ok(
                VerifiedIdentity::new(principal("alpha", "verified"), CredentialSource::Tailscale)
                    .with_expiry(200),
            )
        })
    }
}

#[tokio::test]
async fn provider_boundary_checks_identity_expiry_without_identity_headers() {
    let ctx = authenticate(&TrustedProvider, &(), "verified", 100)
        .await
        .unwrap();
    assert_eq!(ctx.source(), CredentialSource::Tailscale);
    assert_eq!(ctx.ctx().principal.as_ref().unwrap().subject, "verified");
    assert_eq!(
        authenticate(&TrustedProvider, &(), "expired", 200)
            .await
            .unwrap_err()
            .reason,
        "identity_expired"
    );
    assert_eq!(
        VerifiedCtx::from_verified(
            VerifiedIdentity::new(principal("", "missing"), CredentialSource::AuthDisabled),
            "missing",
            100
        )
        .await
        .unwrap_err()
        .reason,
        "invalid_identity"
    );
    let error = sdk::LoamsError::new(sdk::ErrorKind::Internal, sdk::Code::Internal, "secret")
        .with_metadata("secret", "secret")
        .with_client_reason("future_valid_reason");
    let safe = loams_web_platform::map_sdk_error(error);
    assert_eq!(safe.reason, "future_valid_reason");
    assert!(!serde_json::to_string(&safe).unwrap().contains("secret"));
}
