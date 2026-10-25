# Loams Web CLI

Create a runnable Rust application using this unpublished framework checkout:

```sh
cargo run -p loams-web-cli -- new /tmp/hello-app --name hello-app --framework-path "$PWD"
cd /tmp/hello-app
cargo run
```

`--framework-path` accepts either the repository root or its
`crates/loams-web` facade directory. The generated Cargo dependency uses the
canonical absolute facade path; keep the checkout available or edit the
manifest after moving it. External crates use the workspace-compatible major
versions (`serde 1`, `schemars 1`, `tokio 1`, `axum 0.8`). Offline operation
requires those dependencies already cached. Generation itself performs only
local filesystem work and executes no shell or Cargo commands.

The generated server exposes a validated typed POST `/hello` procedure and
`GET /openapi.json`. It binds `127.0.0.1:8080` by default; set `LOAMS_WEB_ADDR`
to override it. Ctrl-C requests bounded graceful shutdown through the Loams
Web server. A standalone `[workspace]` keeps the new app independent even
inside another Cargo checkout.

The destination's parent must exist. Existing destinations, files and
symlinks are refused; package names use a conservative lowercase ASCII Cargo
subset. The library entry point is `scaffold(&ScaffoldOptions { directory,
name, framework_path })`, returning the canonical created directory.

Existing TypeScript artifact commands remain available:

```sh
loams-web-cli descriptors.json --write client.ts
loams-web-cli descriptors.json --check client.ts
```
