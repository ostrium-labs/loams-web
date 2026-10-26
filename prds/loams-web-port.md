# Loams Web: Rust port plan v2

Status: implementation in progress. The initial Rust workspace implements
the LW-004 backend vertical slice, OpenAPI and TypeScript generators, and
an initial conformance workbench; see the
[Rust preview guide](../docs/src/content/docs/loams-web.md). M0 remains open
until its reference, protocol and integration evidence is recorded.
Date: 2026-10-25. Supersedes the assumptions and sequencing in the supplied 2026-10-23 v1 brief.

Implementation progress (2026-10-26): the workspace now also has bounded
validation problems, `WireType`/procedure macros, one-port REST/Connect/gRPC/
gRPC-Web serving, a request-scoped adapter for published Loams SDK 0.1.2,
and a React 19 DOM app with shared primitives. Rust/Bun checks and the
source-pinned reference harness checks pass. The shared Loams fixture corpus
verifies (38 fixtures, 28 required) and its 25 Node harness tests pass, but the
Rust SDK does not define the six required fixture test names; filtered Cargo
invocations run zero tests and no Rust fixture report is produced. The official
Connect v1.0.6 profile remains unrun: its stdin/stdout compatibility adapter and
ConformanceService are not implemented, and fetching the pinned runner asset
stalled at zero bytes. The OCaml reference image could not be inspected after
slow external image downloads, and the attempt recorded zero suites. The
LW-006 rspc/Specta spike is recorded in
[the feasibility report](../docs/rspc-spike.md): the isolated adapter compiles,
but rspc is unmaintained and needs an explicit Specta macro pin. The LW-008
GPUIX/Kit source spike is recorded in [the feasibility report](../docs/gpui-feasibility.md): the pinned
Kit and GPUIX graphs use distinct GPUI package identities, and GPUIX exposes no
usable external custom-element API. A patched shared-graph build remains
unverified, so these additions do not close M0 or claim full Spindle parity.
The LW-007 auth/Durable source boundary is recorded in
[the feasibility report](../docs/platform-feasibility.md); it identifies an
engine-authenticated `WhoAmI` candidate and `/durable/` gateway, but no
Loams-Web verifier or worker adapter has passed an end-to-end test.

## 0. Decision

Proceed with a behavior-preserving Rust port of Spindle, using Tokio, axum/tower and hyper, with real React 19 and a DOM host as the first release path. Develop GPUIX as a second host. Keep the GPUI Kit bridge and GPU browser target behind explicit feasibility gates. Rust-native React remains a separate future program.

Preserve Spindle's route descriptions, validation, documented errors, dependency semantics, bounded streaming and test discipline. Do not translate its HTTP engine or Eio implementation mechanically.

The original ten-week schedule is a useful target for a constrained backend + DOM preview. It is not yet supported as an estimate for the entire v1 feature list, twelve integration crates, three render targets, a widget bridge, protocol code generation, and packaging on three desktop operating systems. Re-estimate after the first two weeks of measured work.

## 1. Evidence and scope

Local clone: `/home/keshav/Ostrium/loams-web`.

| Source | Inspected revision | What it establishes |
|---|---|---|
| `hyphatech/spindle` | `e180e57ae94ea6869038fccde8621d738dd2b84a` | Eight OCaml packages; API contracts, implementations, test structure, docs, one draft PRD |
| Existing local `loams` checkout | `bc902971d84c83383523b9e0cb166e151f643d8a` | Rust SDK, auth modes, Connect stack, token semantics, Live protocol, Durable embedding, CLI ownership |
| GPUIX | `a53b294401280014e94eb9bd98a7c7ed1929f92d` | React/native manifests, custom-element implementation, private exports and registry |
| GPUI Kit | `v0.8.0` | Published GPUI snapshot dependency and optional backend |

See [inspection report](loams-web-inspection.md) and [source inventory](loams-web-source-inventory.json). The inventory records all 402 tracked upstream files and static test labels. Inspection is static: upstream suites have **not** been run in this environment. `opam`, `dune`, `bun`, `cargo` and `rustup` are absent from PATH. No claim of conformance follows from inspection.

The checkout retains full upstream Git history and its existing `origin`; a hosted `ostrium-labs/loams-web` fork has not been created. The inspection below predates implementation. Rust implementation files now live beside the retained reference source.

### Corrections to v1

1. **rspc is unmaintained.** Preserve the requested first-party rspc surface, but isolate and pin it. M0 must decide whether Loams accepts maintaining the adapter/fork. A Connect + TanStack Query fallback is available, but replacing rspc requires an explicit product decision. [Upstream notice](https://github.com/specta-rs/rspc).
2. **Loams already uses `connectrpc` + `buffa`.** The Rust SDK uses `connectrpc 0.9.1`, `buffa 0.9.2`, and shared generated proto crates; the server mounts that stack into axum. Evaluate it before introducing tonic/prost or a new tower protocol implementation. The runtime documents Connect, gRPC and gRPC-Web from one registration; conformance still needs independent proof. [Runtime documentation](https://docs.rs/connectrpc/0.9.1/connectrpc/).
3. **The conformance corpus is not an external black-box harness.** Many cases invoke private parsers, Eio flows and virtual-time servers. Separate network-observable requirements from implementation tests before porting them.
4. **The fork includes MCP and OAuth.** `mcp/` and `mcp-client/` are real packages, absent from v1's inventory. Decide their release scope explicitly. The remaining broadcast PRD is marked draft; it is not evidence of shipped behavior.
5. **Tokens merge; they are not a last-response scalar.** Reuse the SDK's per-package consistency session and explicit-consistency precedence. Graph tokens require special handling.
6. **Auth is `off` or combinations of `apikey`, `oidc`, `tailscale`.** These are not four mutually exclusive runtime enum variants. `off` is loopback-only in the engine's contract.
7. **The facade crate is missing from the proposed layout.** `#[loams_web::procedure]` needs a `loams-web` facade crate re-exporting the macro and public types, or every example must use a different path. Add the facade.
8. **Proto generation requires an evolution model.** Arbitrary serde Rust types cannot automatically become stable protobuf services. Add a restricted RPC schema and a committed field-number ledger.
9. **GPUIX widget registration is internal at the inspected pin.** Factories exist, but `custom_elements` and `renderer` are private modules and the view registry is `pub(crate)`. Plan an upstream extension or small maintained patch before an external bridge crate.
10. **GPUI dependencies differ.** GPUIX uses the `remorses/zed` submodule at `f77e5cfe9dad04288566f00c66ecf3c58d6f84b3`; GPUI Kit 0.8.0 defaults to `gpui-pre =0.3.9`, with optional `gpui-fast`. The important requirement is identical GPUI types and a compatible platform graph. Multiple versions can sometimes coexist in Rust, but their types are not interchangeable; native linkage may add further constraints. [GPUIX manifest](https://github.com/remorses/gpuix/blob/a53b294401280014e94eb9bd98a7c7ed1929f92d/packages/native/Cargo.toml), [Kit manifest](https://github.com/longbridge/gpui-kit/blob/v0.8.0/Cargo.toml).

## 2. Requirements and release boundaries

Retain the names in the brief: repository `loams-web`; npm `@loams/web` and `@loams/react`; CLI `loams app ...`; future Rust-native family `loams-react`. Do not rename `@loams/client`.

| Delivery | Required behavior |
|---|---|
| First backend/DOM preview | Typed routes and errors; input validation; OpenAPI 3.2 + local Scalar; REST; one shared procedure through RPC transports; auth/token adapter; bill-split; generated TS client; shared primitives with DOM host |
| Integration completion | Required Loams SDK corpus through the actual adapter path; Live reconnect; Durable external worker crash recovery; Postgres/MySQL wire pools and migration coordination; agent-run viewer |
| GPUIX desktop preview | Same app logic and primitive API; native input and focus tests; supported desktop packaging; no widget-bridge dependency for basic rendering |
| Conditional expansion | GPUI Kit DataTable bridge and GPU browser host after their gates; additional widgets later |
| Deferred | Own HTTP engine, broad MCP server/client parity, general cross-language app SDK packaging, full Rust React semantics, Electron replacement |

Every upstream behavior has one status in a traceability manifest: planned, passing, approved divergence, or deferred. A deferred security or framing behavior cannot be hidden behind a blanket claim of Spindle parity.

### Non-functional requirements

- Bound connection count, request headers, buffered bodies, uploads, per-stream queues, database acquisition, RPC messages and shutdown. Reject overload without unbounded waiting.
- Preserve authentication-before-body behavior, per-request dependency caching, and typed failures. Route/schema discovery must not execute handlers or require a live database.
- Keep credentials, bodies, raw query strings, SQL values and raw consistency tokens out of logs and exported spans. Namespace and principal attributes need bounded, approved representations.
- Maintain tenant/principal isolation through pooled clients, consistency sessions, Live subscriptions and Durable jobs.
- Reproduce security failures, timeout decisions and crash scenarios deterministically where practical. Record OS/browser/toolchain limits for renderer results.
- Set performance budgets after baseline measurements: p50/p95/p99 latency, RSS, concurrent streams, input latency and package size. No performance claim is justified by architecture alone.
- Build the backend/DOM path without GPU libraries or nightly Rust. Use Bun for new repository JS tooling; inherited external test runners remain explicitly identified.

## 3. Architecture

```mermaid
flowchart TD
    DOM[React DOM] --> WEB[@loams/web hooks and clients]
    GPU[React GPUIX] --> WEB
    LANG[Other language clients] --> RPC[Connect / gRPC / gRPC-Web]
    WEB --> RSPC[rspc adapter]
    WEB --> RPC
    REST[REST clients] --> HTTP[axum / tower / hyper]
    RSPC --> HTTP
    RPC --> HTTP
    HTTP --> CTX[Request context: principal, scope, consistency, deadline]
    CTX --> CALL[Typed procedure invocation]
    DEF[Procedure descriptor and schema registry] --> CALL
    DEF --> GEN[OpenAPI / TS / optional proto generation]
    CALL --> SDK[Generated loams Rust SDK]
    CALL --> DUR[Resonate worker adapter]
    CALL --> SQL[sqlx wire pools]
    SDK --> PLATFORM[Loams services]
    DUR --> PLATFORM
    SQL --> PLATFORM
```

One procedure descriptor records stable name, HTTP binding, RPC binding, input/output schema, declared errors, credentials, dependency metadata, stream kind and optional Durable identity. Adapters decode their transport, validate, construct `Ctx`, invoke the same typed handler and encode the outcome. No handler calls another adapter to reach its own implementation.

Use tower services as the composition boundary. Keep core types independent of axum extractors and RPC runtime types. The HTTP crate owns the default server policy and transport limits; platform owns outbound Loams integration. Avoid embedding the engine or linking its server binary into an app.

### Workspace

Keep all twelve proposed crates and add `crates/loams-web/` as a small facade. Establish crate directories as they acquire behavior; do not create twelve empty published packages in M0.

| Crate | Responsibility and boundary |
|---|---|
| `loams-web-core` | Procedure descriptors, typed handlers, dependencies, request context, validation traits and transport-independent errors |
| `loams-web-http` | axum/tower/hyper adapter; limits, REST decoding, SSE/WS, static assets and shutdown |
| `loams-web-derive` | `WireType` and procedure macros; compile-time diagnostics; metadata generation |
| `loams-web-openapi` | OpenAPI 3.2 serializer from descriptors; local Scalar assets; strict generation checks |
| `loams-web-rpc` | Separate rspc and Connect adapters; descriptor-driven invocation and RPC code generation |
| `loams-web-platform` | Generated SDK binding, auth mapping contract, scope-safe tokens and Engine wrappers |
| `loams-web-live` | Watch/ModifyQuerySet/Authenticate lifecycle, SSE/WS/rspc stream adapters and resumption |
| `loams-web-durable` | External Resonate worker/client integration; replay and job identity, not a second Durable server |
| `loams-web-sql` | sqlx Postgres/MySQL wire clients, optional SQLite dev adapter, branch resolution and migrations |
| `loams-web-session` | Cookie/session stores and `Principal` abstraction; credential verification delegated through a defined provider |
| `loams-web-telemetry` | tracing/OTel configuration and exporter hooks; no global subscriber installed by a library |
| `loams-web-cli` | App scaffolding/generation/build entrypoints; integrated into the existing `loams` command separately |
| `loams-web` | Public facade and feature flags; enables the brief's `loams_web::procedure` syntax |

Keep `proto/`, `web/ui-primitives`, `web/host-dom`, `web/host-gpuix`, `web/kit-bridge`, `web/app`, `web/client`, `examples/`, and `docs/` as proposed. `@loams/react` exports primitives and explicit host subpaths; `@loams/web` exports generated-client integration and hooks. Publish host code as separate entrypoints so DOM builds do not bundle native bindings.

## 4. Procedure, schemas and code generation

### 4.1 Minimal vertical slice first

Implement the descriptor and an explicit builder before committing to complex macros. Prove `Bill -> Share` through an in-process handler, REST, schema output and typed error. Add macro sugar over that working representation.

The sample should explicitly derive serde traits:

```rust
#[derive(serde::Serialize, serde::Deserialize, WireType)]
struct Bill {
    #[wire(min = 0.0)]
    total: f64,
    #[wire(min = 0, max = 100)]
    tip: u32,
    #[wire(min = 1, max = 50)]
    people: u32,
}
```

`WireType` emits validation and schema metadata; do not promise that a derive rewrites the input item to insert serde derives. Test renames, defaults, optional/nullable fields, enums, nested validation, recursion, numeric bounds, non-finite floats and encoding/decoding differences. Unsupported serde representations must produce a diagnostic or an explicit loose-schema report.

Keep one schema representation for validation bounds and route metadata. Choose Schemars/Specta adapters in the spike by matching actual serde shapes and rspc's pinned requirements. Do not maintain three independent derivation systems with separate validation rules.

### 4.2 Proto emission

Make app `.proto` export **opt-in** in v0.1. REST-only values, raw responses, multipart files and WebSockets are valid procedures without a protobuf service.

RPC-enabled procedures accept a deliberate subset: named messages, stable field tags, explicit scalar widths, repeated fields, optional presence, maps with supported keys, and discriminated variants mapped to `oneof`. Define handling for timestamps, bytes, decimal values, JSON objects and 64-bit TS integers before generation. Reject shapes that have no lossless mapping.

Commit an app schema/field-number ledger. Renames keep tags; deletions reserve tags and names; generation is deterministic; breaking checks compare against a released descriptor set. A Rust field reorder must never change wire numbers. Pin imported Loams descriptors/protos by release and commit/digest, and import them without redefining messages.

Use proto-derived messages directly for services shared with SDKs. For Rust-first app DTOs, generate explicit conversions and test them. Preserve unknown-field/evolution behavior supported by the chosen runtime. rspc and Connect share handlers and descriptors; neither must become the other's generator.

### 4.3 Protocol and error behavior

Prefer the existing Loams `connectrpc`/`buffa` family if its pinned version passes the required official Connect conformance profiles and app tests. Record streaming shape, encoding, compression and HTTP-version coverage. Introduce tonic only if a demonstrated requirement is missing and the cost of a second message runtime is accepted.

Test one port with native HTTP/2 gRPC, Connect unary and streaming, browser gRPC-Web and REST. Browsers do not gain client/bidirectional request streaming from the gRPC-Web adapter. Include deadlines, cancellation, response metadata/trailers, malformed envelopes and proxy behavior. Enable reflection if `grpcurl` is an exit-gate tool, or supply the descriptor explicitly.

`ApiError` contains canonical RPC code, stable application reason, safe message, typed details, request ID and optional retry metadata. REST status, Connect status and gRPC/gRPC-Web trailers are transport projections, not identical HTTP status codes. Preserve Loams `ErrorInfo`/reason semantics; never relay internal exception text. OpenAPI lists dependency and handler errors. Record intentional changes from Spindle's `error/message/problems` JSON envelope.

## 5. Spindle behavior and conformance migration

Create `conformance/cases/` plus adapters for the pinned OCaml reference and Rust server. Each case records upstream file/row, RFC section if present, bytes or structured input, expected outcome, fragmentation and timing model, supported target and any divergence rationale.

| Class | Porting method | Gate |
|---|---|---|
| HTTP wire and framing | Raw TCP client; malformed heads, length conflicts, chunking, pipelines, HEAD/204/304, disconnect and drain | Both reference and Rust evaluated against explicit expectations |
| Parser-only rows | Extract portable fixtures; use a library-level harness where the framework exposes the parser | Do not claim a network test exercised an internal parser |
| Structured Fields | Preserve all published JSON vectors and their license; run parsing and serialization vectors | Every applicable vector accounted for |
| Multipart and URL encoding | Preserve repeated-field order, malformed boundaries, UTF-8 errors, streaming limits | Buffered and streaming paths pass |
| WebSockets | Raw-frame peer, fragmented/bytewise input, masking, close races, subprotocol and origin checks | Protocol requirements plus application typing tests |
| Framework behavior | Rust in-process tests for DI caching, staged extraction, errors, sessions, auth-before-body and route ambiguity | One behavior per upstream source reference |
| Schema and generation | Golden OpenAPI/TS/proto, deterministic output, strict mode and compile-fail cases | No descriptor/runtime drift |
| Time and pressure | Injected clock/controlled I/O, barriers for backpressure and disconnect | No sleep-based race deciding a verdict |
| Database/session | Real Postgres/MySQL/SQLite where supported; acquisition, transactions, rotation and expiry | No silent service-dependent skip in required CI |
| OAuth/MCP/client | Classify reusable app requirements separately from full upstream package parity | Deferred scope is visible |

hyper intentionally makes some parsing/framing decisions before application code runs. Preserve accepted bytes/status/close behavior where feasible; assess mismatches for protocol/security impact. If a required rejection cannot be enforced above hyper, change the acceptance contract or transport policy explicitly. Do not normalize mismatches away to make the suite green. TCP writes do not guarantee server read boundaries; deterministic parser fragmentation needs an injected transport.

M0 ports a representative subset and inventories the rest. Full HTTP/WS/forms coverage belongs to M1 and subsequent hardening; promising all of it plus workspace setup in the first week is unrealistic.

## 6. Loams integration contract

### Auth and sessions

Map engine auth configuration (`off`, API key, OIDC, Tailscale and supported combinations) to an app `Principal` with namespace, subject, roles/scopes, credential source and expiry. Keep app cookie sessions distinct from engine credentials.

The engine's authenticator is currently in its server package, which shares the SDK's package name `loams`. Resolve a reusable verification/introspection boundary in M0: a supported SDK operation where available, a small shared Apache library extracted upstream, or a verified trusted-proxy contract. Do not depend on the server crate, duplicate key storage, trust client-supplied identity headers, or invent an app-only auth scheme. Test Tailscale's trusted ingress boundary and mixed credential precedence.

Mint keys through the Loams flow; retain hashes or opaque references as its contract requires. Test cookie name binding, rotation, old/bad cookie behavior, session fixation, expiry, secure attributes, logout and cross-origin mutations against the upstream cases.

### Consistency and SDK use

Seed a request-scoped SDK `ConsistencySession` from validated inbound context and pass it through `CallOptions::session`, `CallOptions::consistency` and required metadata. Reuse `ConsistencyToken::merge`: maximum offset per `(stream, partition)`, normalized, with exact integer values. Explicit call consistency wins over implicit session behavior.

Preserve service-package scopes and graph-read exceptions from `docs/sdk/runtime-contract.md` R4. Do not store one process-global mutable token or credential on a shared pooled client. Define which namespace/service scope the public response header represents; use an explicit composite application context if a handler spans incompatible scopes.

For ordinary responses, finalize the merged token before committing headers, including a safe error after a successful write. Streaming responses cannot change headers after the first byte: carry subsequent state in typed events/trailers as supported. Keep Live resume versions distinct from collection/SQL tokens.

Replay required shared SDK fixtures **through a Loams Web handler/adapter** to the fixture server, checking observed outbound calls and report coverage. A passing upstream SDK suite alone does not prove propagation. Add two-process write/read and concurrent-client isolation tests. All engine calls go through the generated SDK; missing SDK coverage is an upstream SDK task, not permission to hand-roll HTTP.

### Live

Adapt the existing `Watch`, `ModifyQuerySet` and `Authenticate` protocol. Resume uses `StateVersion` plus query set; the protocol sends full results on reconnect. SSE IDs may encode adapter resume state, but must not pretend a collection consistency token is a Live cursor. Reauthorize on identity change, bound queues, drop/resync slow consumers and clean up quiet disconnected subscriptions. Update query caches through one host-independent client layer.

### Durable

Use the pinned Resonate SDK against Loams's authenticated external Durable gateway. The inspected `loams-durable::DurableRuntime` is an **in-process engine** runtime, not a ready-made external app worker API; prove transport, task delivery and auth in M0/M4.

Use a caller-stable logical idempotency key scoped by namespace, procedure/version and caller as appropriate. A newly generated HTTP request ID changes across retries and cannot alone supply replay identity. Define behavior for key reuse with different inputs, retries, expired leases and changed handler versions.

Durability does not make arbitrary side effects exactly once. Express checkpointed steps through the runtime; pass stable keys to external effects or coordinate a transaction/outbox. Test the crash window after an effect but before its checkpoint, lease takeover and process restart. Clarify whether a call waits for completion or returns an accepted promise handle.

### SQL, Studio and telemetry

- Configure sqlx `PgPool` and `MySqlPool` against the real Loams wire endpoints; resolve branch identity before pooling. A branch endpoint/identity change must not reuse a stale pool. SQLite runs blocking work outside async executors.
- Use a migration ledger, checksums and database/branch locking in addition to a Durable job. Test concurrent migration starters, restart and dialect-specific DDL behavior. Do not promise transactional DDL equally on both wires.
- Agent-run viewer consumes pinned agentd protos/client capabilities, with authorized diffs and approval/merge mutations. Missing generated SDK coverage is an explicit upstream deliverable. Do not bypass authorization for demo convenience.
- Export standard tracing/OTel to configured collectors. Langfuse and GlitchTip hooks use their supported integrations; an OTLP URL is not evidence that every tool accepts every signal. Redact raw tokens and bound labels.

## 7. React 19 and GPUIX

Implement `Box`, `Text`, `Stack`, `VirtualList`, `Markdown`, `Input` and `Button` with a shared semantic API: spacing/theme tokens, controlled values, focus, disabled state, keyboard activation, accessible naming and normalized events. Avoid exposing `HTMLElement`, DOM events, CSS selectors or a GPUI entity in shared props. Define an escape hatch for host-specific features rather than pretending every browser API is portable.

Shared app TSX uses React components only. Each leaf host has its own compiler configuration; GPUIX leaves use its required JSX runtime and intrinsic types. Test module resolution and bundle graphs so DOM and GPUIX intrinsic namespaces do not contaminate each other. GPUIX's inspected React package requires React `^19.2.0` and reconciler `^0.33.0`; select compatible exact pins rather than an unspecified React 19 minor. [React package manifest](https://github.com/remorses/gpuix/blob/a53b294401280014e94eb9bd98a7c7ed1929f92d/packages/react/package.json).

### Early native bridge gate

Within the first two weeks, allocate a bounded spike:

1. Expose/register one external Rust factory through a minimal GPUIX patch or supported upstream hook.
2. Align GPUI Kit and GPUIX to one usable GPUI/platform dependency family; inspect the lock graph, compile and render together. Include any required Kit initialization/theme/focus ownership.
3. Render DataTable, update rows from React, receive selection events, preserve focus and destroy/recreate repeatedly without leaked entities or subscriptions.
4. Test on one desktop OS first. State the patch ownership and remaining OS work.

Pass: a reproducible source build with a small reviewed patch and tests. Fail: ship GPUIX built-ins with a simpler table or keep the heavy widget on DOM; defer Kit bridge rather than blocking the framework. The current published native addon cannot be assumed to load an unrelated Rust widget crate dynamically. [Custom-element implementation](https://github.com/remorses/gpuix/blob/a53b294401280014e94eb9bd98a7c7ed1929f92d/packages/native/src/custom_elements/mod.rs), [private exports](https://github.com/remorses/gpuix/blob/a53b294401280014e94eb9bd98a7c7ed1929f92d/packages/native/src/lib.rs).

### Browser GPU gate and desktop relationship

The inspected GPUIX browser path needs nightly Rust, matching wasm-bindgen `0.2.127`, shared-memory isolation headers and WebGPU support. Its README says browser accessibility props lack an AccessKit adapter. Keep that build experimental until accessibility, input/IME, isolation deployment and browser fallback tests pass. DOM remains the supported browser route. [GPUIX browser instructions](https://github.com/remorses/gpuix#examples).

Do not replace Electron or merge the existing GPUI agents app in this project. Measure startup, RSS, app size, input latency, platform coverage and maintenance cost first. Pure GPUI Kit tools may consume the same APIs independently; they are not React host parity evidence.

## 8. Milestones and implementation tickets

Estimates below are planning ranges for one engineer. Agent help, if later authorized, may accelerate bounded tasks but is not assumed to remove integration work. Overlap only after the shared descriptor and runtime contracts stabilize.

| Milestone | Target window | Concrete work | Exit gate |
|---|---|---|---|
| M0 — baseline and decisions | Weeks 0–2 | Fork policy/licensing; reference runtime; test inventory; descriptor/bill-split slice; dependency pins; rspc decision; Connect baseline; auth/Durable boundaries; GPUI bridge spike | Reference baseline recorded; representative wire/schema tests pass on Rust; six dependency/boundary decisions documented |
| M1 — framework semantics | Weeks 2–5 | Routing/DI; staged extractors; errors; request limits; CORS; cookies/sessions; static/files policy; SSE/WS; shutdown | Applicable ported HTTP/WS/forms/framework requirements green, divergences reviewed |
| M2 — schema and generators | Weeks 4–7 | WireType; macro; separate input/output schema; strict OpenAPI 3.2; local Scalar; TS generation; deterministic checks | Bounds and all input/error metadata agree at runtime and in docs; compile-fail diagnostics and generation checks pass |
| M3 — protocol parity | Weeks 6–9 | rspc adapter; Connect stack; stable opt-in proto ledger; reflection/descriptors; deadline/error/stream tests | Same handler invoked by REST, rspc, grpcurl and TS Connect; official conformance scope recorded; TS/Python/Go smoke passes |
| M4 — platform integration | Weeks 8–12 | SDK fixture adapter; principal mapping; scoped tokens; Live; external Durable; SQL branches/migrations; telemetry | Required fixture reports pass; cross-process RYW + isolation; Live reconnect; crash/replay and concurrent migration tests pass |
| M5 — shared app and desktop | Weeks 10–14 | Primitives + DOM; agent viewer; GPUIX desktop; optional bridge from M0 result; input/visual tests | Same shared app works on DOM and designated desktop platforms; unsupported widgets have explicit fallbacks |
| M6 — distribution and release | Weeks 13–16+ | `loams app`; scaffold MCP integration; docs/llms index; clean-machine install; release packaging; conditional wasm experiment | Repeatable install/dev/gen/build; documented support matrix; required CI green; v0.1 release checklist complete |
| M7 — measured improvements | Later | Benchmarks, optional HTTP engine, broader SDK emit, additional widgets | Work justified by measured bottleneck and preserved conformance |

These ranges are a provisional rebaseline, not measured delivery commitments. Full desktop support, Kit alignment failures or upstream SDK changes can extend them. If ten weeks is fixed, target a backend + DOM preview, one RPC procedure family, and a limited desktop preview; defer wasm, broad widgets and broad MCP parity explicitly.

### First implementation batch

1. **LW-001:** Establish licensing layout: unchanged MIT upstream sources/tests; Apache-2.0 new Rust code; complete Hypha copyright and MIT text in retained license files/NOTICE; preserve the IETF fixture license and bundled Scalar notices. Do not replace upstream MIT with a blanket Apache label.
2. **LW-002:** Create a reproducible OCaml 5.5 reference image with Hypha opam repository, Eio/wiretype pins, Bun OIDC test issuer and Postgres. Capture `make lint`/`make test` results and explicit skips before changing behavior.
3. **LW-003:** Inventory individual conformance rows and classify them; establish a machine-readable traceability manifest. Port split, malformed framing, HEAD, multipart boundaries and one WS close scenario as the first cases.
4. **LW-004:** Add minimal `core`, `http`, facade and example crates, Tokio/axum/hyper server, explicit procedure builder and in-process invocation. Add only the error/schema traits needed for bill-split.
5. **LW-005 (attempted; remains open):** The official Connect v1.0.6 server profile needs a stdin/stdout compatibility adapter and ConformanceService, neither of which the bill-split app provides; its runner asset transfer stalled at 0 bytes. Corpus and harness checks pass, but the Rust SDK lacks the six required fixture tests, and a filtered Cargo check proves the apparent test commands run zero tests. See [`proto/README.md`](../proto/README.md). Implement the adapter and actual fixture reporting before closing this gate; do not claim conformance from the existing local RPC tests.
6. **LW-006:** Spike complete; adoption gate remains open. A temporary adapter compiled against the existing `Procedure::invoke`, but rspc is unmaintained, the exact release requires an additional compatibility pin, and typed export requires parallel Specta `Type` derives. See [the spike report](../docs/rspc-spike.md). Keep Connect plus the generated fetch client as the supported path unless Loams explicitly takes ownership of an internal fork and its dependency/API maintenance.
7. **LW-007:** Source boundary is recorded in [the feasibility report](../docs/platform-feasibility.md). The SDK has a `WhoAmI` call and the engine serves an authenticated `/durable/` gateway, but SDK docs mark `WhoAmI` unimplemented and no app verifier or external worker adapter is proven. Run the authenticated server and task-replay checks before closing this gate; open upstream SDK/extraction tasks for any missing supported interface.
8. **LW-008:** Source inspection is recorded in `docs/gpui-feasibility.md` with exact pins, API visibility findings and a proposed patch/build recipe. The required gate remains open until Kit and GPUIX compile together against one GPUI package identity and a small external factory app renders successfully.

M0 completion means evidence exists for these gates, not merely that these tickets or directories exist.

## 9. Verification and CI

- Rust: formatting, clippy, unit/integration/compile-fail tests, feature combinations, docs and package/install checks. Stable backend lane separate from nightly GPU browser lane.
- Reference: pinned OCaml baseline and applicable differential cases; unexpected semantic divergences fail. Full generated case accounting prevents a small green subset from becoming a parity claim.
- Protocol: official Connect conformance for the chosen pin; native gRPC and browser gRPC-Web; REST/rspc; TS/Python/Go generated app callers; malformed input, trailers and deadlines.
- SDK: frozen fixture corpus with required-case accounting, actual adapter call path, version/token vectors, unavailable-service paths and app scenarios. Do not infer readiness from a language count or stale prose.
- Platform: two server processes, separate clients/principals, simultaneous token merges, auth combinations, Live gaps/expiry, process-kill Durable tests with side-effect crash windows, SQL branch identity and migration contention.
- UI: Bun typechecks/tests; DOM interaction/accessibility checks; GPUIX automation screenshots, keyboard/focus/IME tests on designated OS runners; browser GPU capability/isolation checks. Headless screenshots do not establish IME or screen-reader support.
- Release: generated-artifact diff check, dependency/license graph, reproducible version pins, package provenance and clean-machine scaffold/build smoke. Docs examples must compile/run and client generation must require no running production service.

New JS commands use Bun. Existing Loams conformance scripts contain Node/pnpm assumptions; verify Bun compatibility and add a supported upstream invocation where needed. Do not globally rewrite another repository's toolchain to satisfy a Loams Web convention.

## 10. Decisions (ADRs)

All decisions below are proposed unless directly fixed by the brief. They are reviewable here before implementation; no additional permission prompt is required to perform the planned reversible implementation work.

| ADR | Decision | Alternative and trade-off | Status / revisit trigger |
|---|---|---|---|
| 001 | Retain upstream history and behavior provenance; license new code Apache while preserving upstream notices | Clean new repo is tidier but loses fork traceability | Fixed direction; exact retained-source layout in M0 |
| 002 | axum/tower over hyper; core descriptors independent of transport | Custom engine gives control but materially expands risk/work | Fixed hyper-first direction; revisit only with M7 measurements |
| 003 | Prefer Loams `connectrpc`/`buffa`; prove conformance | tonic is mature but adds a second message/runtime family; handwritten Connect adds maintenance | Proposed; replace only for documented missing requirements |
| 004 | Do not adopt rspc by default; keep a bounded adapter prototype and use Connect + generated TS as the supported path | rspc preserves the brief's tRPC-like surface but adds a dead upstream, Specta-specific DTO derives and error mapping | Recommended by LW-006 spike; revisit only with named internal maintenance ownership or upstream resumption |
| 005 | One descriptor + validation schema; explicit builder before macros | Macro-first is attractive but can conceal an incoherent runtime model | Proposed; reassess after vertical slice |
| 006 | Opt-in protobuf generation with stable field ledger | Automatic tags by Rust field order break evolution | Proposed; revisit only after compatibility evidence |
| 007 | Request/principal-scoped SDK consistency sessions | Global token state is simpler but leaks causality across callers/scopes | Proposed; must satisfy existing SDK R4 |
| 008 | Real React + primitives; DOM release, GPUIX progressive | Renderer-specific apps duplicate product logic; Rust React now delays release | Fixed route from brief; bridge/browser gates remain conditional |
| 009 | External Durable adapter with logical idempotency keys and explicit side-effect rules | Embedding a second Durable engine changes the application-layer boundary | Proposed; external worker feasibility in M0 |
| 010 | Extend the existing `loams` CLI via a small upstream integration | Publishing a second binary named `loams` conflicts with engine installs | Proposed; standalone `loams-web-cli` helper is acceptable before upstream delegation lands |

## 11. Open questions resolved and remaining

| Question | Finding / default | Remaining evidence |
|---|---|---|
| Canonical 13 SDK languages | TypeScript, Python, Rust, Go, C#, Java, Kotlin, Swift, Ruby, C++, Dart, Objective-C, PHP; use `sdks/conformance/required.mjs` and fixture manifest as enforcement | Registry publication, required-case coverage and app parity per language; docs counts are not proof |
| Rust Connect | Existing stack is the first candidate; no new tower implementation planned by default | Official conformance results at exact dependency pin |
| App proto default | Opt-in for v0.1; explicit RPC-compatible types and evolution ledger | Mapping/conversion spike |
| npm names | Public registry returned HTTP 404 for both `@loams/web` and `@loams/react` on inspection | Authenticated organization permission and eventual publish test; 404 is not a reservation |
| GPUI Kit bridge | Internal factories exist; public extension point and dependency alignment need work | Compiled DataTable demonstration and maintained patch cost |
| Desktop replacement | Neither Electron nor existing GPUI agents app is replaced in v0.1 | Comparative measurements and a separate migration decision |
| CLI integration | `loams` command enum lives in the engine repository; no app delegation was found in the inspected enum | Small upstream subcommand/delegation change with version handshake |
| Auth library boundary | Engine verifier is not currently a standalone app library at the inspected paths | Supported introspection/shared auth extraction and fixture parity |

## 12. Future Rust React 19

Retain R0–R5 as a separate roadmap: semantics contract, core/hooks, scheduling, async/Suspense/Actions, authoring, and optional server rendering. R0 must specify supported React version/behaviors, effect ordering and cleanup, strict-mode expectations, update scheduling, hydration/SSR exclusions and host contract. Port selected upstream tests with their license/provenance and distinguish applicable tests from JS runtime assumptions.

A Rust `rsx!` authoring model can preserve selected semantics but cannot run arbitrary existing JS React components. Keeping primitive names unchanged alone does not guarantee application source compatibility. An embedded JS engine changes that trade-off and requires a capability model. Benchmark against the shipped React/GPUIX app and study Dioxus, Leptos, Sycamore, GPUI and gpui-shell before committing. Start only after a measured product constraint justifies the ongoing compatibility cost.
