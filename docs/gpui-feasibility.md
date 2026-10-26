# GPUIX and GPUI Kit feasibility spike

Date: 2026-10-26. This is a source/build feasibility result, not a supported
desktop-host implementation.

## Pinned inputs

| Project | Revision | Relevant dependency |
|---|---|---|
| GPUIX | `a53b294401280014e94eb9bd98a7c7ed1929f92d` | `packages/native` 0.11.0; its `zed` submodule is `f77e5cfe9dad04288566f00c66ecf3c58d6f84b3` |
| GPUI Kit | tag `v0.8.0`, commit `1676826d9d49b90f4d7b6808b69924f58796f6f1` | `gpui-pre =0.3.9`; `gpui-pre-platform =0.3.9`; `gpui-pre-web =0.3.9`; optional `gpui-fast =0.1.10` family |

Sources were checked out from the official GitHub repositories at those
revisions under `/tmp`; no local GPUIX or GPUI Kit checkout was present before
the spike. GPUIX's `rust-toolchain.toml` pins Rust 1.97.1.

## Compatibility result

GPUIX's native crate depends on `gpui` by path into its own Zed submodule; at
the pinned commit that package is `gpui 0.2.2`. Kit 0.8.0's workspace pins the
separately packaged `gpui-pre` snapshot at exactly `0.3.9`. A smoke application's
resolved Cargo graph contains both package IDs (`gpui 0.2.2` and `gpui-pre
0.3.9`), plus both platform graphs. They are distinct Rust types even if their
APIs happen to match. A `gpui::Window`, `App`, element, or context from one
graph cannot be passed to APIs compiled against the other.
Kit's optional `gpui-fast` family is another backend and does not remove this
identity constraint.

The nominal extension traits exist in GPUIX source, but no external extension
API is currently exposed:

- `CustomElement`, `CustomElementFactory`, `CustomElementRegistry`, and
  `CustomRenderContext` are declared `pub` in
  `packages/native/src/custom_elements/mod.rs`.
- `lib.rs` declares `mod custom_elements` privately and does not re-export those
  items, so downstream crates cannot name them.
- Even exposing that module is insufficient: the public trait's `render`
  signature names `crate::renderer::GpuixView`, which is `pub(crate)`. The
  context also carries internal aliases/types from private renderer/text
  modules. The renderer and its registry field are crate-private.
- The native addon crate's public API is principally the N-API renderer. A
  `cdylib`/`rlib` dependency does not make its private internal registry a
  dynamically loadable plugin mechanism.

An external Rust bridge therefore needs a deliberate GPUIX API patch, not just
a `pub use`. The maintainable boundary is a new public extension context with
public GPUI types, props/events/children and lifecycle, while keeping the view,
selection internals and renderer state opaque. The renderer should expose a
registry-registration hook and invoke it before the first render. GPUIX's
`CustomElementFactory` code is a useful implementation template but cannot be
implemented externally at this pin as written.

## Patch and build recipe

For an integration experiment, vendor/fork both trees and retain the exact
pins above. Make Kit's workspace GPUI aliases (`gpui`, platform, web, macros,
reqwest client and sum-tree) resolve to the corresponding crates in GPUIX's
`zed` submodule, then compile Kit against that graph and fix source/API drift.
Do not merely add the original Kit crates.io dependency next to GPUIX: that
build may compile both graphs, but their GPUI types remain unusable across the
bridge. Preserve the Kit 0.8.0 feature selection and platform flags when
mapping aliases. Then add the narrow GPUIX registration/context API described
above and compile a small app that registers one factory, renders one element,
and uses one Kit component in the same GPUI window. Record the patch commit and
platform-specific commands before treating the bridge as reproducible.

The GPUIX native library check passed after supplying the host's missing
development metadata from packages unpacked under `/tmp` (no system package
installation):

```sh
PATH=/home/keshav/.cargo/bin:$PATH \
PKG_CONFIG_PATH=/tmp/lw-gpui-pkgs/root/usr/lib/x86_64-linux-gnu/pkgconfig:/tmp/lw-gpui-pkgs/root/usr/share/pkgconfig \
PKG_CONFIG_SYSROOT_DIR=/tmp/lw-gpui-pkgs/root \
  cargo check \
  --manifest-path packages/native/Cargo.toml --lib --no-default-features
```

The host lacks `libfontconfig1-dev`, so its development packages and transitive
pkg-config metadata were downloaded and extracted under `/tmp/lw-gpui-pkgs`.
The check completed successfully with existing system runtime libraries. It
emitted existing unused/dead-code warnings in GPUIX; no source changes were
made to that checkout.

A minimal Cargo app at `/tmp/gpuix-kit-smoke` was resolved against the exact
local GPUIX and Kit sources with default features disabled. Its `cargo check`
also passed with the same temporary package path and confirms both GPUI
identities can coexist in one dependency graph. This empty smoke app does not
pass values between the two graphs or register/render a GPUIX custom element.
Native link behavior, cross-graph type use, Kit API drift beyond dependency
compilation, and GUI runtime behavior remain unverified. The intended external
factory integration remains structurally blocked at these pins until GPUIX
exposes an API and the dependency graphs are unified.

## Gate decision

Keep the GPUIX desktop host as a separate, conditional host. Basic GPUIX
rendering can still be explored independently, but the GPUI Kit bridge should
remain deferred until a small app compiles with a single shared GPUI package
identity and a reviewed extension API patch. Do not make the DOM release depend
on this bridge.
