# Loams SDK request adapter

This crate uses the published Rust SDK `loams` 0.1.2, including its generated
Connect clients, token parser, `ConsistencySession`, `CallOptionsOverrides`,
retry policy and credential sources. It does not depend on an engine/server
package. TLS is enabled by default;
`--no-default-features` supports HTTP fixture/dev endpoints without TLS.
The SDK's proto build requires `protoc` on `PATH`.

Create a shared `Platform::connect(TransportOptions)` or
`Platform::with_transport(transport, options)`. It holds a pristine SDK client:
no global credentials, namespace or consistency session. For each request:

1. Call `authenticate(provider, request, request_id, now_unix_seconds)`, or
   `VerifiedCtx::from_verified` with an identity already verified by trusted code.
2. Parse explicitly scoped inbound tokens with `ScopedToken::parse(package, text)`.
3. Create `platform.request(&verified_context, seeds)`.
4. Use `request.call(&ctx, generated_message, options, |sdk, message, options| async
   move { sdk.documents().get(message, options).await })` inside the typed handler.
5. After outbound work finishes, select a package explicitly with
   `request.response_token(&ctx, ServicePackage::Collection).await` for an ordinary
   response header. This also works after a successful write and a later error.

The adapter binds state to the core request cache's identity, not merely a textual
request ID. A copied context shares the request; a new context with the same ID,
namespace and subject does not. Altered principals and foreign namespaces are
rejected before the generated call runs. Application code must still declare the
SDK errors it intends to expose in its procedure descriptor.

Tokens remain separate for collection, SQL, graph and stream packages within one
namespace/principal/request. All normalization and merges use the SDK. Graph
reads do not receive implicit session tokens. Handlers spanning incompatible
response scopes must define an explicit composite application context; this
adapter never labels a merged cross-package token as one ordinary header.
Live resume versions remain outside this consistency-token API. Streaming
responses must carry later state in typed events/trailers after headers commit.

## Explicit consistency and the SDK boundary

The inspected SDK's `app_modules::prepared` injects an unset request's session
token independently of `CallOptionsOverrides.consistency`; an explicit header
could therefore conflict with an implicit message field. `RequestClient::call`
prevents that conflict by using an empty SDK session for an explicit call, then
folding its returned package tokens into the request session using SDK
`record_in`. Explicit generated-message consistency remains unchanged. The
low-level `options` helper rejects explicit call consistency; use `call` so the
response merge cannot be forgotten. Existing foreign session overrides are also
rejected. An upstream SDK change can eventually remove this small workaround.

SDK 0.1.2's generated app-module futures borrow the SDK runtime and are not
`Send`, so `RequestClient::call` drives them on a Tokio blocking worker through
the current runtime handle. The typed request and response are owned across
that boundary; transport I/O remains on Tokio. Dropping the handler future does
not stop a call that has already entered the blocking worker. Configure SDK
deadlines and host-side request admission for the expected service latency.

## Authentication boundary and verification scope

`AuthProvider<Request>` is a host-owned verification interface, with no permissive
default implementation. `Request` may include trusted connection/ingress evidence
owned by the host. `VerifiedIdentity::new` is a trusted assertion by that provider,
not a credential verifier. Namespace, subject and optional expiry are checked
when the verified request context is created. Outbound `TokenSource` values stay
on that request's SDK client; the core context never contains bearer credentials.

This crate does not implement issuer/JWT verification, credential precedence,
API-key storage, Tailscale identity-header trust, app cookie sessions, or a new
auth scheme. The SDK's `WhoAmI` response alone does not define the complete
namespace/scope/expiry verification contract. Hosts must supply a supported
engine verification/introspection or audited trusted-ingress provider. Contexts
belong to one bounded request; long-lived subscriptions must reauthorize and
create a new context when identity or expiry changes.

Tests replay the shared SDK token vectors through the request-session adapter
and exercise generated SDK writes, reads, SQL and graph calls through a core
handler and recording transport. They verify emitted request bodies/headers,
explicit precedence, exact 64-bit offsets, response merging, isolation and safe
errors. This is not a report of full shared-fixture-server conformance, live
engine authentication, or two-process deployment verification.
