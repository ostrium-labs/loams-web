---
title: Loams Web Rust preview
description: Build and run the first Rust procedure and HTTP adapter.
---

The Rust workspace is an early backend vertical slice of the Loams Web
port. It includes typed procedures, schema validation, safe API errors,
request context, a JSON HTTP adapter, and OpenAPI 3.2 generation. The
OCaml documentation elsewhere on this site describes the retained
Spindle reference.

From the repository root, with the toolchain in `rust-toolchain.toml`:

```sh
cargo test --locked --workspace
cargo run --locked -p bill-split
```

Send a bill to the example:

```sh
curl http://127.0.0.1:8080/split \
  -H 'content-type: application/json' \
  --data '{"total":90,"tip":10,"people":3}'
```

The response is `{"each":33.0}`. The input schema constrains `total` to
nonnegative values, `tip` to 0–100, and `people` to 1–50. These bounds are
declared on the Rust type using Schemars and used for runtime validation
and documentation. JSON with missing or unknown fields, incompatible
types, or values outside the bounds is rejected before the handler runs.

`GET /openapi.json` exposes the descriptor-generated document. Generate it
without starting a server or running a handler:

```sh
cargo run --locked -p bill-split -- --openapi > /tmp/bill-split.openapi.json
```

Generate the TypeScript fetch client from the same descriptor:

```sh
cargo run --locked -p bill-split -- --client > /tmp/bill-split.ts
cargo run --locked -p bill-split -- --descriptors > /tmp/bill-split.descriptors.json
cargo run --locked -p loams-web-cli -- /tmp/bill-split.descriptors.json --write /tmp/client.ts
cargo run --locked -p loams-web-cli -- /tmp/bill-split.descriptors.json --check /tmp/client.ts
```

The generator supports a strict JSON-schema subset and fails on unsupported
shapes. Numeric values use TypeScript `number`; protobuf generation and a
lossless int64 representation are outside this preview. `--check` fails for
missing or stale artifacts. This helper does not add `loams app` to the
separate engine CLI.

Set `LOAMS_WEB_ADDR` to change the listen address. The default is
`127.0.0.1:8080`. Ctrl-C stops accepting connections and gives active
requests up to five seconds to drain.

The `loams-web` facade re-exports the core types. Its default features add
the HTTP and OpenAPI crates. Disable default features for a core-only
consumer; the core has no axum dependency.

The HTTP adapter initially supports static paths and JSON procedures.
GET, HEAD and OPTIONS require unit input and an empty request body. The
default limits are 1 MiB buffered request and response payloads, 128
concurrent procedure calls, and a 30-second body/invocation deadline.
Excess concurrency receives 503 immediately. Declared errors share the
response payload bound; fixed framework error envelopes are still returned
when that bound is too small for them. The adapter has no listener-level
connection or header deadline policy yet. Its invocation deadline cancels
the handler future; this has not established Spindle cancellation parity.
This preview does not implement the complete Spindle transport policy,
OAuth, cookies, sessions, streaming, RPC, SDK integration, frontend hosts,
or the procedure and WireType macros. Application validation beyond the
generated JSON schema can use the procedure's input validator. Runtime
validation and schema construction must not fetch external schemas.

The port plan in `prds/loams-web-port.md` tracks the remaining milestones.
The `conformance/` directory accounts for upstream rows separately from
execution evidence; a fixture or a green Rust subset does not establish
full Spindle parity. The OCaml baseline has not yet been run in this
environment.
