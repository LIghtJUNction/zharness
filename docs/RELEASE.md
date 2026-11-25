# Release contract

Development uses Git-only Project Z dependencies. The committed lock records
the selected source and package version; it is not replaced by registry data
during release planning or version bumps. The direct dependencies are zcall,
zplugin and zguard. Their Git closure also contains zconv, zplugin-macros and
zsys; zplugin and its macro package share a repository but remain distinct
package identities. The explicit wasm feature adds Wasmi runtime packages.
The dependency audit checks core, default, wasm-only and all-feature graphs
separately, excluding compiler-only procedural macro/parser packages from
runtime claims.

## External Cargo packages

The release script reads the clean developer commit and committed Cargo.lock.
For every Git package it verifies the exact registry name/version, repository,
non-yanked state, API checksum and matching sparse-index checksum. A missing,
ambiguous or mismatched source stops packaging; no dependency update is run.

Packaging uses a separate local checkout at `target/registry-stage`.
Only manifest Git locations and their matching lock sources/checksums change.
Each manifest selector becomes the exact locked version, while dependency
features, existing registry entries, package versions and source files remain
unchanged. The clean projection commit has the developer candidate as its sole
parent. It has no remote and is never pushed to main or used as a release tag.

The developer candidate remains the input to the native/MSRV/Wasm checks.
Cargo package verification and publication dry-runs use the projection and an
owned package target. The package directory is still read from that invocation's
Cargo metadata; foreign/shared output directories and symlink archives are
rejected. Archive VCS identity binds to the projection, while release/tag identity
binds to the developer commit. The original manifest and packaged lock are
checked against the projection and registry checksums.

Retained evidence includes `release-projection.json`, its reconstructible
patch, package hashes and both commit identities. These are validation artifacts,
not tracked binary files. The developer source remains Git-only after release,
and its own package/lock version still advances, so the next automatic bump
starts from the version just released.

## Select a release

Dispatch Release on trusted `main` with `bump` set to patch, minor, major or
custom. Supply `version` only for custom, as strict SemVer. `dry_run` defaults
to true: it validates the final candidate without pushing a version/tag or
publishing. For a committed request, change `.github/release-request.json` to:

```json
{"bump":"custom","version":"0.5.0","dry_run":false}
```

This file is an explicit publication trigger, not a recurring desired version.
Ordinary source/tag pushes do not publish. Publication runs in the same manual
or request workflow and never depends on a generated push triggering another job.

## Exact candidate and validation

The plan validates crate ownership, versions, current tags and exact registry
identities derived from the committed dependency lock. It computes one reproducible version commit. Version changes
update Cargo manifests, the existing lockfile, README and the pending changelog;
dependency versions are not silently re-resolved during a release bump.

A reviewed commit may already contain the requested custom version. A new
release must have strictly higher SemVer precedence than every existing version
tag and must be absent from the registry. A matching existing tag follows the
separate provenance-checked recovery path. Planning a pre-versioned commit does
not introduce another version bump or relax source, owner or validation gates.

Linux, macOS and Windows run Rust stable and MSRV 1.99.1 against that exact
candidate. Checks include fmt/Clippy, debug/release tests, doctests and strict
rustdoc, four feature configurations, real native/Wasm guests, mixed-backend
hosts, hooks/routing/replacement failures, benchmarks in smoke mode, dependency
audits, examples and package verification. A separate job checks the no_std
core and Wasm-host feature on non-native targets. Test evidence records actual
compiler versions and runner platforms; compilation-only targets are not runtime
support claims.

After validation, the publication job recreates and checks the identical commit,
verifies the separate registry package, and atomically pushes the developer
version commit plus immutable annotated tag. Concurrent main changes fail the push. It publishes the package,
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

The workflow retains `release-state.json`, projection provenance, package
manifests/checksums and archives to identify the completed stage. If registry publication succeeded but
Release creation failed, recovery verifies existing bytes and completes metadata
without uploading a different package. Missing archives may be rebuilt only
from the verified projection, after prior hash evidence and the clean developer
tag are checked; the rebuilt bytes must match that evidence before any registry
or GitHub step. An old developer-SHA archive is not silently relabelled as a
projection archive. Historical resume checkouts run the script at their exact
tag and keep its original provenance/checksum contract. Never bump merely to
conceal a partial release. Historical tags and published archives remain unchanged.

## Local verification

```sh
python3 scripts/check.py
python3 scripts/audit.py
python3 scripts/release.py plan --bump minor --dry-run true
```

The plan requires a clean checkout. Full benchmark measurements are separate
from validation smoke runs; see the recorded performance report for flags,
samples and environment. The Evidence workflow performs validation and records
data without registry write credentials. Read-only dry runs may validate an
immutable PR/fork checkout or an earlier main commit; they never push refs.
Real publication still requires the owning repository, main, and an unchanged
remote source commit, checked again before the atomic push.
