# Loams Web inspection report

Date: 2026-10-25. Scope: static repository and integration review for the Rust port plan.

## 1. Checkout and verification limits

- Upstream: `https://github.com/hyphatech/spindle.git`.
- Local clone: `/home/keshav/Ostrium/loams-web`.
- HEAD: `e180e57ae94ea6869038fccde8621d738dd2b84a`; latest inspected release commit in its history: `e06f0b5 Release 0.11.0`.
- Full Git history retained. `origin` still points to upstream; no hosted fork or push was performed.
- Existing Loams source: `/home/keshav/Ostrium/loams`, HEAD `bc902971d84c83383523b9e0cb166e151f643d8a`. No tracked modifications were reported during inspection.
- 402 upstream tracked files; 178 `.ml` files / 50,598 lines; 110 `.mli` files / 7,941 lines. These totals include tests/examples, not just runtime code.
- Read repository guidance, build/package manifests, module contract inventory, central routing/dependency/schema/transport implementations, test harnesses, case registrations, examples, docs structure and the remaining PRD. Reviewed relevant Loams integration implementations and external dependency source.
- This is a planning review, not a line-by-line correctness proof or a completed behavioral port. The inventory is exhaustive file accounting; inclusion in it does not assert every line was manually audited.
- No baseline suite executed: `opam`, `dune`, `bun`, `cargo` and `rustup` are unavailable on PATH. Podman and Python are available. Toolchain installation and runtime verification belong to M0.

The machine-readable [inventory](loams-web-source-inventory.json) records upstream file hashes, source-interface summaries, test registration labels, private-parser dependencies and external metadata snapshots. Static call-site counts are not runtime test counts: generators and parameterized tables expand them.

## 2. Actual upstream package boundaries

| Package / path | Findings | Port consequence |
|---|---|---|
| `spindle-http` / `http` | Header grammars, dates, ranges, framing, parser/writer, SSE, WebSocket frames, JWK/JWS, trace/log context | hyper replaces transport machinery; structured values and observable protocol rules still need explicit coverage |
| `spindle` / `core` | Described routes/returns, DI, codecs, staged body access, application construction, limits, cookies/sessions, middleware, files, streams, schema printers | Core value is the common descriptor and behavior, not OCaml syntax |
| `spindle-client` / `client` | Standalone outbound client, SSE/WS and OAuth; links HTTP layer rather than framework | Loams calls use its generated SDK; general HTTP/OAuth client parity must be explicitly scoped |
| `spindle-postgres` / `postgres` | rowtype backend, bounded pools/transactions, telemetry, DB sessions | sqlx is a replacement architecture; test transaction/pool/session behavior rather than driver equivalence |
| `spindle-sqlite` / `sqlite` | WAL pools, session store and migration | Define async/blocking boundary and filesystem behavior for optional dev storage |
| `spindle-cli` / `cli` | Route-derived OpenAPI/Zod/Melange generation and stale-artifact checks | Not the same thing as a project-scaffolding CLI; preserve strict generation/check commands |
| `spindle-mcp` / `mcp` | Typed tool schemas, discover/call/progress, OAuth/security, elicitation-related behavior | Larger scope than a scaffolding MCP tool; do not claim automatic full parity |
| `spindle-mcp-client` / `mcp-client` | Tool discovery/calls and shared protocol library | Can remain deferred independently of core server delivery |

`wiretype`/`ppx_wiretype` and `rowtype` are external Hypha dependencies, not implementations contained in this repository. A Rust derive port needs a separately defined supported schema contract; it cannot be translated from source that is not here.

## 3. Behaviors that a conventional axum wrapper would miss

1. `core/route.mli`, `route_repr.ml`: a route describes inputs, success variants, refusal codes, credential requirements and opaque dependencies without executing a request. Route input names deduplicate, including case-insensitive headers. Construction validates ambiguous paths and illegal declarations.
2. `core/dep.mli`, `dep_repr.ml`: dependencies have identity; values and refusals cache once per request. Non-body dependencies run first. Invalid inputs aggregate within a stage; other refusals short-circuit. Shared request resources are validated at startup.
3. `core/returns.mli`, `refusal.mli`: successes have declared status/body shapes. Errors have stable branchable codes, safe messages and structured input problems; private diagnostics stay private. Undeclared outcomes are test failures and runtime diagnostics.
4. `core/server.mli`, `deadline.mli`, `body.mli`, `wire.ml`: body budget, header deadline, minimum body rate, bounded discard, send timeout, connection admission and graceful drain are independent controls. A generic request timeout does not cover all of them.
5. `core/form.mli`, `multipart.mli`, `http/urlencoded.mli`: repeated fields retain order, form and file inputs differ, uploads may be streamed, and the body may not be both streamed and decoded independently.
6. `core/cookie.mli`, `session.mli`, `key.mli`: signed/encrypted cookies bind to names and age, support rotated keys, and malformed/retired cookies become absent. Server sessions rotate/destroy independently of credential authentication.
7. `core/cors.mli`, `bearer.mli`, `request.mli`: mutation/WS origin policy, trusted proxies, token audience and credential-source rules are deliberate behavior. Do not assume framework middleware defaults match.
8. `core/files.mli`, `static.mli`, `conditional.mli`: static assets loaded at startup differ from dynamic disk files. Traversal/symlink protection, conditional requests, ranges and file replacement races have explicit contracts.
9. `core/event.mli`, `broadcast.mli`, `http/websocket.mli`: typed messages, pre-encoded fan-out, bounded slow subscribers, explicit socket/stream errors and disconnect cleanup. Dropping a request future is not automatically equivalent to Spindle's cancellation policy.
10. `core/openapi.mli`, `described.ml`, `openapi.ml`, `zod.ml`, `melange.ml`: input/output schemas can differ; operation/component collisions fail generation; loose routes are reported; strict generation requires no live application resource. Scalar is bundled for same-origin/offline use.
11. `http/trace.mli`, `core/log.mli`, `metrics.mli`: safe fields, route-based names and bounded metric dimensions matter as much as exporter choice. Library startup must not install a global logger or tracing subscriber unexpectedly.
12. `core/test.mli`, `test/in_memory.ml`: in-process clients share rendering rules with the server; tests have virtual time and controlled I/O. Replacing these with sleeps and real ports loses deterministic failure coverage.

## 4. Test corpus assessment

| Suite | Source lines | What must migrate |
|---|---:|---|
| `test_spindle.ml` | 10,635 | Routing, dependency/cache/resource behavior, codecs, cookies, sessions, CORS, files, errors, streams, limits and concurrency |
| `test_http_rfc.ml` | 4,956 | RFC section tables, parser/framing cases, fragmentation, generated mutations/sequences and a second-parser referee |
| `test_wire.ml` | 1,235 | Multiple requests per connection, unread/discarded bodies, framing, byte budgets, timeouts, compression and shutdown |
| `test_websocket_rfc.ml` | 1,174 | Frame/protocol requirements, split reads, masking and close behavior |
| `test_websocket.ml` | 941 | Client/server integration, typed protocols and lifecycle |
| `test_multipart_rfc.ml` | 396 | RFC multipart grammar and streaming fragmentation |
| `test_structured.ml` | 254 | Published Structured Fields JSON vectors, including serialization |
| `test_openapi.ml` | 1,147 | Schema direction, deduplication, naming collisions, errors, generated clients and strictness |
| `test_oauth_rfc.ml` | 1,454 | JWT/JWK published examples, issuer/resource checks, OAuth flows and token secrecy |
| `test_mcp_spec.ml` | 1,228 | Protocol/tool/schema/security/client cases beyond app scaffolding |
| `test_spindle_postgres.ml` | 1,207 | Pool/transaction/session/database behavior |
| `test_spindle_sqlite.ml` | 488 | SQLite-backed pools, migrations and sessions |
| `test_cli.ml` | 227 | Artifact print/write/check, stale/missing outputs, strictness and exit codes |
| `test_docs.ml` | 69 | Example/document linkage |

Additional support includes `in_memory.ml`, `instants.ml`, `stub_server.ml`, `db_target.ml`, `tls_front.ml`, process fixtures, Melange tests and a Bun OIDC issuer. `test_gz.ml` is a helper, not proof of a standalone registered suite.

`test_http_rfc.ml` aliases `Spindle_http.Private.Head`, `Framing` and `Write`; other suites directly exercise internal WebSocket paths. Thus the test files are not portable black-box clients. Preserve the row's input and expected behavior, then choose network, parser, framework or generation adapters. Exact bytewise reads need deterministic injected transport; TCP packet boundaries alone are insufficient.

The upstream suite checks deliberate disagreement with `httpun`. Differential results against Spindle must be evaluated against those RFC expectations, not assumed to make either implementation correct.

`make test` starts Postgres via Podman Compose and installs/runs the Bun OIDC issuer. Bypassing these dependencies can skip required cases. CI runs OCaml 5.5 and a lower-bound dependency lane. Capture actual executed/skipped counts before using the baseline as a reference.

Only one work PRD remains: `prds/broadcast-stream.md`, explicitly **Draft**. Its proposed `with_subscription`, `forward` and `stream` helpers are absent from the inspected `core/broadcast.mli` export set. Treat its scenarios as design input, not an already implemented contract. Landed requirements live in interfaces, tests, docs, contributor rules and `CHANGES.md`.

## 5. Loams integration findings

| Evidence in adjacent `loams/` | Finding |
|---|---|
| `sdks/rust/Cargo.toml` | Package name `loams`, not `loams-sdk`; separate Cargo workspace because engine package has the same name; Rust 1.97 requirement; `connectrpc 0.9.1`, `buffa 0.9.2`, generated `loams-proto` and `loams-live-proto`; no tonic dependency |
| `crates/loams/Cargo.toml` | Engine already mounts Connect runtime in axum and includes health/reflection helpers; engine package is `publish = false` |
| `crates/loams/src/auth/mod.rs` | `Off` or `On { api_keys, oidc, tailscale }`, including combinations; authenticators belong to engine package today |
| `sdks/rust/src/consistency.rs` | Exact integer positions; parse/normalize/merge by maximum offset per pair |
| `sdks/rust/src/request.rs`, `call.rs` | Scoped consistency sessions and per-call session/consistency/metadata options are available for adapter reuse |
| `docs/sdk/runtime-contract.md` R4 | Per-package token merging, explicit consistency override and graph exception; raw header forwarding alone is incomplete |
| `proto/loams/live/v1/live.proto` | Watch stream plus unary mutation/query-set/auth operations; resume from StateVersion + query set; full resnapshot on reconnect |
| `crates/loams-durable/src/lib.rs`, `runtime.rs` | Embedded server/runtime uses in-process worker network; an external app runtime requires its own supported gateway/worker transport integration |
| Root `Cargo.toml` | Resonate family pinned to `ostrium-labs/resonate` commit `31ffb2011b324c187f1d324b83feec2e16787e74` |
| `sdks/conformance/required.mjs`, `fixtures/manifest.json` | Required-case enforcement and canonical language table; checked reports plus source references, not just hand-written coverage declarations |
| `sdks/README.md`, `fixtures/README.md` | Useful orientation, but prose readiness statements can differ from later app fixtures and manifests; inspect actual required cases |
| `crates/loams/src/main.rs` | Existing command enum owns `loams`; a new repository cannot add `loams app` by publishing another `loams` executable |

Canonical language implementations: TypeScript, Python, Rust, Go, C#, Java, Kotlin, Swift, Ruby, C++, Dart, Objective-C and PHP. This identifies the thirteen entries, not a verified claim that all thirteen registries and feature surfaces are production-ready.

The Rust SDK's current package metadata and adjacent source are stronger implementation evidence than old language page counts. Release/publishing status still needs its own check at implementation time.

## 6. External dependency checks

| Dependency | Verified finding | Practical impact |
|---|---|---|
| [rspc upstream](https://github.com/specta-rs/rspc) | README explicitly declares no ongoing maintenance | Early ownership/adapter decision; do not silently substitute a new API |
| [connectrpc 0.9.1](https://docs.rs/connectrpc/0.9.1/connectrpc/) | Documents tower/axum integration and all three RPC protocols | Reuse Loams runtime candidate; docs are not a recorded conformance result |
| [GPUIX React manifest](https://github.com/remorses/gpuix/blob/a53b294401280014e94eb9bd98a7c7ed1929f92d/packages/react/package.json) | Package version 0.11.0, React `^19.2.0`, reconciler `^0.33.0` | Exact compatible frontend pins |
| [GPUIX native manifest](https://github.com/remorses/gpuix/blob/a53b294401280014e94eb9bd98a7c7ed1929f92d/packages/native/Cargo.toml) | Native dependency is a path into its Zed fork; wasm-bindgen `=0.2.127` | Source rebuild and graph alignment required for new Rust widgets |
| [GPUIX exports](https://github.com/remorses/gpuix/blob/a53b294401280014e94eb9bd98a7c7ed1929f92d/packages/native/src/lib.rs) / [renderer](https://github.com/remorses/gpuix/blob/a53b294401280014e94eb9bd98a7c7ed1929f92d/packages/native/src/renderer.rs) | Private custom-element/renderer modules; registry on view is `pub(crate)` | Internal registry is not a supported external extension API |
| [GPUIX factories](https://github.com/remorses/gpuix/blob/a53b294401280014e94eb9bd98a7c7ed1929f92d/packages/native/src/custom_elements/mod.rs) | Typed render context, lifecycle, supported props/events and registry already exist internally | A bounded upstream exposure patch may be feasible; needs compilation evidence |
| [GPUI Kit v0.8.0](https://github.com/longbridge/gpui-kit/blob/v0.8.0/Cargo.toml) | Exact `gpui-pre =0.3.9` snapshots by default, optional `gpui-fast` family | Neither dependency family is automatically identical to GPUIX's vendored GPUI |
| [GPUIX browser guidance](https://github.com/remorses/gpuix#examples) | Nightly + matching wasm tooling, shared-memory isolation; README also flags missing browser AccessKit mapping | DOM is the browser release path until browser input/accessibility/deployment gates pass |

Public npm metadata requests for `https://registry.npmjs.org/@loams%2fweb` and `https://registry.npmjs.org/@loams%2freact` returned HTTP 404 / `{"error":"Not found"}`. No authenticated account or publish operation was used. This check cannot establish scope control, private-package existence or reservation rights.

## 7. Licensing and preservation

Spindle's `LICENSE` is MIT, copyright 2026 Hypha Technologies Ltd. Retain the complete notice and permission text in derivative distributions. New Apache-2.0 code can coexist with retained MIT material; a `NOTICE` credit alone should not replace the upstream license text.

`test/structured-field-tests/LICENSE.md` has IETF Trust redistribution terms and its own attribution/disclaimer. Preserve it with the corpus. `core/dune` identifies bundled Scalar 1.72.1 as MIT; keep its notices if the asset is retained. Record transitive Rust/frontend licenses when selecting actual dependencies.

No Loams Router source was imported. The plan keeps application integration through SDKs, wire protocols and independently usable libraries. A dependency/license graph check is required before release rather than assuming a crate's name establishes the intended boundary.

## 8. Recommendation

Use the [v2 plan](loams-web-port.md) as the implementation baseline. Its first milestone produces a runnable reference, a narrow Rust vertical slice, portable test traceability and evidence for the major dependency decisions. Keep the comprehensive v1 product direction, but make parity, external worker behavior and native host support earned exit gates.
