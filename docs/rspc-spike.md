# LW-006 rspc and Specta spike

Status: adapter feasibility established; adoption not recommended without a
named Loams owner for the fork or compatibility layer. Checked on 2026-10-26.

## Upstream and pins

The official [rspc repository](https://github.com/specta-rs/rspc) marks the
project unmaintained. In its [maintenance announcement](https://github.com/specta-rs/rspc/discussions/351),
the maintainer said they were stepping back from development and PR reviews,
described the code and documentation as rough, and called out the broken Tauri
integration. `cargo info` resolved the latest published rspc version as
`1.0.0-rc.5`. Its manifest pins `specta = 2.0.0-rc.1` and enables the old
Specta TypeScript feature by default.

Specta is still receiving releases, but its latest published core observed in
this spike was `2.0.0-rc.27`; the separately published
`specta-typescript` exporter was `0.0.14`. The official
[Specta release page](https://github.com/specta-rs/specta/releases) calls
`rc.25` a pre-release intended as the last release candidate before stable
2.0.0. The `rspc` release does not use that current Specta API: it compiles
against the older `rc.1` API.

## Adapter feasibility

A disposable adapter was compiled against the workspace's
`loams-web-core::Procedure<Input, Output>`. Its resolver cloned an
`Arc<Procedure<...>>`, called `Procedure::invoke(Ctx::new(...), input)`, and
mapped `ApiError` to `rspc::Error`. The core handler and its validation still
run through the existing `Procedure` implementation.

The crate graph did not compile with only `rspc = 1.0.0-rc.5`: Cargo selected
`specta-macros 2.0.0-rc.27` for Specta rc.1's prerelease dependency range, and
the old Specta crate failed to build against those macros. Adding
`specta-macros = 2.0.0-rc.1` as an exact direct pin made the graph compile.
The temporary adapter then passed `cargo check` with the real workspace core
path. No rspc or Specta dependency was added to the application workspace.

For typed TypeScript router output, request/response DTOs also need Specta
`Type` derives. Current procedures derive Schemars/Serde and use those schemas
for runtime validation, OpenAPI and the first-party fetch client. A production
adapter would either maintain a second type-export derivation alongside the
existing schema or build and support a custom Specta exporter from the
canonical descriptor. It would also need to translate Loams auth, request IDs,
error codes and structured error details into rspc's request context and error
model. The tiny compile probe only established invocation reuse; it did not
establish transport, auth, error-detail, cancellation, streaming or generated
client parity.

## Maintenance cost and recommendation

Even a thin adapter leaves Loams responsible for:

- locking rspc `1.0.0-rc.5`, Specta `2.0.0-rc.1`, and the otherwise implicit
  `specta-macros 2.0.0-rc.1` compatibility pin;
- carrying Specta derives/export behavior for every DTO or maintaining a
  descriptor-to-Specta bridge, including drift tests against Schemars/Serde;
- maintaining the Axum integration and mapping Loams context, cancellation,
  streaming and structured errors across two evolving APIs;
- taking dependency, bug-fix and security updates without upstream PR review
  or a supported rspc release.

That ongoing ownership is larger than the code in the resolver closure. The
recommendation is to keep Connect plus the generated TypeScript fetch client as
the supported RPC/client path and not merge rspc dependencies into the
workspace yet. Reopen adoption if upstream maintenance resumes or Loams names a
team to own a maintained fork and its compatibility/test matrix. This is an
engineering recommendation, not a product decision to remove rspc from the
brief; that support promise remains an M0 adoption gate.

## Reproduction record

The isolated probe used `rspc = "=1.0.0-rc.5"`,
`specta = "=2.0.0-rc.1"`, and
`specta-macros = "=2.0.0-rc.1"`, with `loams-web-core` referenced by local
path. The probe defined Serde/Schemars/Specta input and output structs and
registered a mutation whose resolver invoked the shared `Procedure`. It
compiled with the repository's pinned Rust 1.97.1 toolchain. The source was
kept outside the repository so this feasibility check does not add an
unmaintained dependency or an untested integration surface to the workspace.
