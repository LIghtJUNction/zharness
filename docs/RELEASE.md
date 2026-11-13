# Release contract

The direct pinned dependencies are zcall 0.2.0 and zplugin 0.3.0; zcall pins
zconv 0.3.0. Default/native runtime dependencies stay within Project Z. The
explicit wasm feature adds eight Wasmi runtime packages. The dependency audit
checks core, default, wasm-only and all-feature graphs separately, excluding
compiler-only procedural macro/parser packages from runtime claims.

## Select a release

Dispatch Release on trusted `main` with `bump` set to patch, minor, major or
custom. Supply `version` only for custom, as strict SemVer. `dry_run` defaults
to true: it validates the final candidate without pushing a version/tag or
publishing. For a committed request, change `.github/release-request.json` to:

```json
{"bump":"custom","version":"0.4.0","dry_run":false}
```

This file is an explicit publication trigger, not a recurring desired version.
Ordinary source/tag pushes do not publish. Publication runs in the same manual
or request workflow and never depends on a generated push triggering another job.

## Exact candidate and validation

The plan validates crate ownership, versions, current tags and exact published
dependency pins. It computes one reproducible version commit. Version changes
update Cargo manifests, the existing lockfile, README and the pending changelog;
dependency versions are not silently re-resolved during a release bump.

Linux, macOS and Windows run Rust stable and MSRV 1.99.1 against that exact
candidate. Checks include fmt/Clippy, debug/release tests, doctests and strict
rustdoc, four feature configurations, real native/Wasm guests, mixed-backend
hosts, hooks/routing/replacement failures, benchmarks in smoke mode, dependency
audits, examples and package verification. A separate job checks the no_std
core and Wasm-host feature on non-native targets. Test evidence records actual
compiler versions and runner platforms; compilation-only targets are not runtime
support claims.

After validation, the publication job recreates and checks the identical commit,
verifies packaged source, and atomically pushes the version commit plus immutable
annotated tag. Concurrent main changes fail the push. It publishes the package,
checks crates.io/archive and registry-index checksums, and creates GitHub Release
from the verified changelog and CI evidence. Release concurrency never cancels
an active upload. Read-only CI jobs have only contents:read permissions.

Only the publication step receives `CARGO_REGISTRY_TOKEN` through the same-named
environment variable. No cargo login, token file, secret command argument,
credential cache or token-bearing artifact is created.

## Recovery

If a partial publication fails after the immutable tag exists, rerun with custom,
the same version and dry_run=false. Recovery accepts only a tag with this
workflow's provenance. Already-uploaded bytes must match the verified package
checksum; a conflicting published version or foreign tag is rejected. Missing
registry visibility is retried finitely and reported. Existing immutable tags
are never overwritten or silently adopted.

The workflow retains `release-state.json`, package manifests/checksums and
archives to identify the completed stage. If registry publication succeeded but
Release creation failed, recovery verifies existing bytes and completes metadata
without uploading a different package. Never bump merely to conceal a partial
release. Historical tags and published archives remain unchanged.

## Local verification

```sh
python3 scripts/check.py
python3 scripts/audit.py
python3 scripts/release.py plan --bump minor --dry-run true
```

The plan requires a clean checkout. Full benchmark measurements are separate
from validation smoke runs; see the recorded performance report for flags,
samples and environment. The Evidence workflow performs validation and records
data without registry write credentials.
