# Portable conformance workbench

Run from the repository root, with Python 3 and no third party packages:

```sh
python3 conformance/check.py
python3 -m unittest discover -s conformance -p 'test_*.py'
```

These commands check traceability, fixture consistency, and the checker itself.
They do **not** run the OCaml or Rust implementation. The default output makes
that distinction explicit. This corpus is an initial representative subset,
not HTTP, WebSocket, OAuth, or Connect certification.

`inventory.json` pins the source inventory digest and source digest of every
indexed test suite. It accounts for all 16 upstream suites, every indexed
literal registration label (including multiplicities), every lexical
`test_case` registration, and each literal `row`/`crow` behavior. Every entry
has a planned/deferred disposition and rationale. Generated and property test
expansions are explicitly unexecuted at suite level: static extraction does
not enumerate their runtime cases. Published Structured Fields vector files
and license are accounted for separately. Source changes require review,
then `python3 conformance/inventory.py --write`; the checker refuses stale
ledgers and registration counts that disagree with the pinned source index.
The small OCaml lexer skips nested comments, character literals, and quoted
strings; it does not pretend to evaluate OCaml expressions.

`cases/portable.json` contains 15 reusable cases. Each carries a source anchor,
RFC section where applicable, deterministic timing, target list, fragmentation
model, expected outcome, and any intentional divergence. Source interfaces
consulted: `core/server.mli`, `core/refusal.mli`, `core/log.mli`,
`http/framing.mli`, `http/multipart.mli`, `http/websocket.mli` and
`client/spindle_client.mli`; split inputs come from `examples/split.ml`.

For split cases, `input` is the Bill object, and `expected` is either
`{"each":33.0}` or `{"invalid_fields":["total","tip","people"]}`.
Compare validation fields in that declaration order. Invalid input must be
rejected before handler evaluation. The fixture does not dictate REST status
or error JSON envelope. The Rust example can load this file through
`include_str!("../../../conformance/cases/portable.json")` and `serde_json`,
filter by `kind == "split"`, and execute its real handler and validation.

Wire text uses JSON strings decoded as Latin-1 (`*_latin1`); binary frames use
hex (`*_hex`). `whole`, `bytewise`, and `every_single_cut` mean one injected
read, one byte per injected read, and every split of the bytes into two
nonempty injected reads. TCP write boundaries do not establish read
fragmentation. Use an injected parser transport for that claim; a raw TCP
adapter may report only network-observable behavior. Framing fixtures test
parsing, not route mounting; multipart tests preserve part order; the WS case
begins after a successful handshake and includes the required client mask.
Adapters must report their unsupported cases explicitly.

`runtime_status.rust = "implemented"` on valid split cases links the actual
Rust runner and command; it records adapter availability, never a passing result.
Invalid split cases are `implemented_partial`: Rust proves rejection, but
currently returns a fixed invalid_input error without the individual invalid
fields. Their complete upstream expectations remain in the fixtures and
their divergence records this unimplemented parity requirement.
Run `cargo test -p bill-split --test portable` to obtain fresh execution.

Runtime observations are optional and separate from fixtures:

```sh
python3 conformance/check.py --observations /tmp/rust-observations.json
```

The JSON shape is:

```json
{
  "version": 1,
  "target": "rust",
  "adapter": "name and invocation of real adapter",
  "revision": "tested commit or source digest",
  "results": [
    {"id": "split-basic", "status": "passed", "observed": {"each": 33.0}},
    {"id": "split-zero", "status": "planned", "reason": "Adapter absent."}
  ]
}
```

Include **every** case ID once (the illustration above is abbreviated and
will be rejected). Runtime statuses are `passed`, `failed`, `planned`, or
`deferred`. Executed results require observations matching the expected
normalized outcome, and failed observations fail the command. Nonexecuted
cases require a reason. Error redaction observes `{"public":...}` and checks
both the safe message and absence of secret canaries in all public fields.
Capture actual implementation output; hand-authored observations are not
proof. Use `--require-executed-kind split` with observations to fail if any split
case is planned or deferred. This option requires an observations file.
A report with planned/deferred entries remains partial coverage even
when its checks pass. Full upstream adapters, controlled I/O, official Connect
profiles, service fixtures, and an execution-required CI gate remain future
work; static validation must never stand in for those gates.

The executable TCP adapter runs three valid split cases against a fresh server
on an ephemeral loopback port, bounds startup and requests, disables proxy
environment inheritance, and terminates the child even if startup or a request
fails. Readiness is read from stderr using a selector and deadline, without
sleep-based polling. Build and run:

```sh
cargo build --workspace
python3 conformance/run_rust.py --binary target/debug/bill-split --output /tmp/loams-web-rust-observations.json
python3 conformance/check.py --observations /tmp/loams-web-rust-observations.json
```

The report records the executable SHA-256 and actual decoded HTTP responses.
It currently leaves twelve cases explicitly planned, including invalid split
cases whose per-field problem parity is absent. It exits nonzero on any
executed mismatch and writes failed observations for inspection. A loopback
socket permission boundary may require an approved invocation. Results are
written outside the checkout and must be regenerated after executable changes.
