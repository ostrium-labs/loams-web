# LW-007 auth and Durable boundary check

Checked against the Loams source revision listed in the port plan on
2026-10-26. This is source/API evidence only; no authenticated Loams server or
Durable task was exercised.

## Authentication

The Rust SDK exposes `Client::who_am_i` for
`loams.instance.v1.InstanceService/WhoAmI` in `sdks/rust/src/client.rs`. At the
pinned source, the engine handler in `crates/loams/src/api/connect.rs` reads
`loams::auth::Principal` from the authenticated request context and returns
its subject, kind, display name and email. A request reaching the handler
without that context gets `not_implemented`; the SDK method's adjacent docs
also still describe it as unimplemented. Treat this API as a candidate
introspection path, not as a verified supported auth contract, until a real
authenticated server call proves the response and failure semantics.

The engine's verifier is in its server/auth package, not an app-facing shared
library. `loams-web-platform` currently defines a `VerificationProvider`
boundary and accepts only a `VerifiedIdentity`; it does not parse credentials
or trust client-supplied identity headers. No concrete Loams verifier adapter
is in this repository.

## Durable

The engine serves its embedded Resonate gateway at `POST /durable/` on the
authenticated main port. `crates/loams/src/api/durable.rs` applies the
engine's principal and organization authorization before forwarding the
protocol envelope to `loams_durable::DurableGateway`. The Durable crate also
has an unauthenticated loopback listener; an app must use the main-port route,
not connect to that listener remotely.

The app-facing boundary is therefore available as an authenticated HTTP
protocol endpoint, but the published Loams Rust SDK does not expose a Durable
worker/client module. This repository has no worker registration, task
acquisition, checkpoint, or replay adapter. Do not treat forwarding one
request envelope as task execution or crash-recovery proof.

## Next proof

1. Make a request-scoped SDK call to `WhoAmI` using the credential presented
   to the Loams Web request, and verify success, rejection, principal mapping,
   expiry, and credential precedence against a real auth-enabled Loams test
   server. If the endpoint or SDK contract is still incomplete, open the
   upstream extraction/API task instead of implementing a second verifier.
2. Pin the supported external Resonate client and endpoint contract, then
   register one deterministic worker against `POST /durable/` through the
   engine's authenticated main port. Exercise task delivery, authorization,
   lease takeover, restart replay, and the crash window after an effect but
   before checkpoint. If the pinned client is not publishable or supported,
   keep the feature gated and open the upstream SDK task.

The `VerificationProvider` and Durable gateway path are useful seams, but
LW-007 remains open until these end-to-end checks pass.
