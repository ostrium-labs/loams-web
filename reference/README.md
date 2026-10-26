# Retained OCaml reference environment

This harness builds the untouched upstream commit
`e180e57ae94ea6869038fccde8621d738dd2b84a` from Git objects. It never copies the
current Rust working tree into the reference image and never modifies retained
OCaml sources. `lock.json` records resolved linux/amd64 image digests for
OCaml 5.5.0, Bun 1.3.0 and Postgres 18 Alpine, exact default/Hypha opam repository
commits, and version pins for Eio 1.6, Wiretype/PPX 0.6.0 and Rowtype 0.9.0.
The issuer uses upstream `test/oidc/bun.lock`, with `oidc-provider` 9.13.0.
Metadata snapshots for the Hypha Wiretype and Rowtype releases include their
published archive checksums and source commit hashes.

Prerequisites: Linux amd64, Python 3.12+, Git, working rootless Podman, network
access and enough disk space for the OCaml development image and dependencies.
The runner intentionally uses the full upstream test/dev-setup dependencies
because `dune build @all` includes renderer examples and Melange. It does not
silently drop those examples to reduce the dependency closure. Build-time
system dependencies are those provided by the digest-pinned base image;
`--assume-depexts` prevents an unpinned apt installation. A missing development
library therefore fails visibly and requires an explicit image/lock update.

```sh
python3 reference/manage.py prepare
python3 reference/verify.py
python3 reference/manage.py build
python3 reference/manage.py run
python3 -m unittest discover -s reference -p 'test_*.py'
```

Podman accesses storage outside the checkout and may require an approved
invocation under a filesystem sandbox. Network or permission failures are
errors, never service-dependent passes. `prepare` runs once: it refuses an
existing extracted source directory. It records the Git archive SHA-256 and
all extracted file digests under `reference/.work/source-manifest.json`.
`build` may be repeated to pick up harness changes while retaining that same
source archive. It freezes version pins before solving package dependencies.
The source checkout path does not need to be mounted into a running container.

`run` creates a unique isolated Podman network, starts pinned Postgres on it
without publishing a host port, waits for its health check with a deadline,
and configures both `SPINDLE_TEST_PG` and `SPINDLE_TEST_OIDC`. Bun launches each
issuer inside its OAuth suite, exactly as upstream does. Network and database
containers are removed after execution; existing containers are never reused
or removed. The database is ephemeral tmpfs with durability disabled as in
upstream `compose.yaml`. It is solely a disposable test database.

The image runner exports installed package versions and `opam switch export
--full --freeze`, builds `@all`, runs `dune runtest --force` (including generated
Melange-client freshness checks), and explicitly executes all 15 runnable
Alcotest suites. The helper `test_gz.ml` is not an executable suite. Each suite
report includes source digest, exact command, elapsed time, exit status,
reported test count, and skip evidence. Zero-accounted tests and any service
skip fail the overall result even if the upstream process exits successfully.
The reported per-suite execution count comes from the explicit executions;
the separate `dune runtest` pass intentionally repeats tests and is not added
to that count. Startup, commands and service health checks have time bounds.

Ignored `reference/results/` contains command logs, `attempt.json`, the inner
`report.json`, installed metadata and switch export. An incomplete build
records zero tests executed. These generated artifacts are execution evidence
for that attempt and are not checked-in permanent passing claims. Failed test
containers still leave their reports for inspection. Re-run after changing
the lock or harness. Direct invocation example:

```sh
podman build --platform linux/amd64 --tag localhost/loams-web-reference:e180e57ae94e \
  --file reference/.work/Containerfile reference/.work
```

Repository/image pins bound dependency selection; they are not a claim of
bit-for-bit reproducible compiler output. A complete solved transitive lock
is available only after successful build/export. The installed package export
and immutable source checksums are the evidence to retain alongside a release
or CI run. No reference suite pass should be claimed from static validation
of these scripts alone. Check the committed `attempt-status.json` for the
latest blocker and `results/attempt.json` for local attempt details;
`results/build.log` records image download/build output. Image pulls stalled;
the separate Bun archive fetch received only 320,129 of 39.7 MB in 115,971 ms.
Podman then reported no usable local image, so the reference runner could not
start and none of the 15 OCaml suites ran. Local harness unit tests and the
402-file source manifest did pass.
