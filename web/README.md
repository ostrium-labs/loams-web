# Loams Web React example

This workspace contains the browser client, host-independent React primitives,
and a DOM host for the Rust `bill-split` example. The API client in
`app/src/generated/client.ts` is generated from the Rust procedure descriptor;
regenerate it from the repository root with:

```sh
cargo run -p bill-split -- --client > web/app/src/generated/client.ts
```

Install Bun 1.3.0, then run the checks from this directory:

```sh
bun install --frozen-lockfile
bun run typecheck
bun test
bun run build
```

To try the app locally, start the Rust service in one terminal and the Vite
server in another. The Vite development server proxies `/split` to
`http://127.0.0.1:8080`:

```sh
cargo run -p bill-split
bun run dev
```

The framework-neutral `@loams/react` components depend on a `HostProvider`.
`@loams/react/dom` supplies native labels, buttons, inputs, forms, and live
status regions for browser applications.
