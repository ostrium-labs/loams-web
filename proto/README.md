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

The official suite is currently **not run**: `go`, `buf`, and an official
conformance runner are absent from this environment, and the cached runtime
crate does not include its upstream conformance adapter or runner profiles.
The local evidence is `cargo test -p loams-web-rpc`: unary Connect, gRPC and
gRPC-Web through the runtime (binary and JSON codecs), validation/error details,
malformed/oversize input, deadlines, and a real TCP same-port test. The TCP test
uses the actual HTTP/2 client for gRPC; its local listener needs socket permission.

Runtime pins are connectrpc 0.9.1 and buffa 0.9.2. Generation uses
connectrpc-build/codegen 0.9.0 and protox 0.10.0; generated service code is compiled
and tested against the selected runtime. An official suite run against this
exact application integration remains a release gate.
