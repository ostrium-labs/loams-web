# Opt-in application RPC schemas

The bills v1 contract is deliberately explicit: named scalar messages and one
unary method. Rust/serde field order never chooses protobuf tags. The RPC crate
compiles these sources with pinned protox and Connect/buffa generators, so building
does not need an installed protoc binary. Runtime messages use buffa; prost is a
build-only compiler dependency.

`field-tags.json` records field numbers, scalar types, optional presence and unary
service input/output identities. The build fails if sources differ from the
ledger. Reordering fields is safe. Keep released tags and names stable, reserve
deleted numbers and names in protobuf sources, and introduce a new package for
incompatible shapes. This preview has no released baseline comparison tool and
does not automatically turn arbitrary serde types into RPC-compatible messages.

Applications supply explicit protobuf-to-input and output-to-protobuf conversion
functions. Their existing core procedure still validates the typed input, invokes
the handler and validates the output. Application reasons, safe details, request
IDs and retry metadata use `ProcedureErrorDetail` in protocol error details.

The runtime owns Connect/gRPC/gRPC-Web encoding, compression and trailers. Local
adapter tests are separate from the official Connect conformance suite. No
official conformance run or broad streaming-language parity is implied here.

The official Connect conformance suite is **not run**. The current profile is
Connect Conformance v1.0.6 (released 2026-10-14; see the
[official release](https://github.com/connectrpc/conformance/releases/tag/v1.0.6)).
Its server mode does not probe an arbitrary application endpoint: it launches a
server-under-test process, writes a length-delimited `ServerCompatRequest` to
stdin, reads a `ServerCompatResponse` from stdout, and then invokes the six
`connectrpc.conformance.v1.ConformanceService` methods. The bill-split app
registers only its application procedure, and the repository has neither this
compatibility-process adapter nor the ConformanceService implementation. Thus
the existing one-port REST/Connect/gRPC/gRPC-Web smoke test is not a valid
substitute or a target for the official runner. The pinned v1.0.6 Linux x86-64
release asset was attempted from GitHub, but its transfer remained at 0 bytes;
`connectconformance` is not installed. No official test cases or result profile
were produced. Add the adapter/service before claiming this gate, then select
only feature combinations the runtime actually implements; do not claim the
full official matrix by default.

The separate shared Loams SDK fixture baseline also remains incomplete. The
fixture corpus verifies (38 fixtures, 28 required), and
`node --test sdks/conformance/conformance.test.mjs` passes all 25 harness tests.
However, the Rust SDK suite does not define the six required fixture test names
registered in `sdks/conformance/required.mjs`. With the pinned repository
compiler at `/tmp/loams-web-protoc-29.3/bin/protoc`, this command completes its
corpus and harness checks but reports missing Rust tests, no
`sdks/fixtures/results/rust.json`, and zero of 28 required fixture cases covered:

```sh
PATH=/home/keshav/.cargo/bin:/tmp/loams-web-protoc-29.3/bin:$PATH \
  sdks/conformance/run-all.sh rust
```

A direct Cargo invocation of one required name confirms `running 0 tests` (74
library tests and 18 integration tests are filtered out), despite Cargo returning
success. Therefore the wrapper's per-test “pass” labels are not evidence that
the shared baseline ran. Implement and report the six Rust fixture tests before
treating LW-005's SDK baseline as green.

Local application-level evidence remains `cargo test -p loams-web-rpc`: unary
Connect, gRPC and gRPC-Web through the runtime (binary and JSON codecs),
validation/error details, malformed/oversize input, deadlines, and a real TCP
same-port test. The TCP test uses the actual HTTP/2 client for gRPC; its local
listener needs socket permission.

Runtime pins are connectrpc 0.9.1 and buffa 0.9.2. Generation uses
connectrpc-build/codegen 0.9.0 and protox 0.10.0; generated service code is compiled
and tested against the selected runtime. An official suite run against this
exact application integration remains a release gate.
