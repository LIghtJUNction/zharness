# Release contract

Version 0.2.1 combines the native primitives from the unpublished 0.2.0 baseline
with tool-output transforms. It is a patch increment of the current source line,
not a backport of those native APIs into the older published 0.1.1 package.
Ordinary source pushes run tests/evidence; only explicit release requests publish.

Dispatch Release on the trusted default branch. Select patch/minor/major or an
exact SemVer; manual publish defaults to false. To release the current version
without another bump, set version=0.2.1 explicitly. Alternatively, push a matching
.github/release-request.json on main with crate, version, exact zcall pin and
publish=true. A historical marker does not authorize a mismatched version. Tag
pushes never initiate publication. Use a fresh exact request for each release.

Owner/version preflight checks crates.io and rejects network errors, collisions,
existing versions and missing/yanked exact dependencies. The production closure
remains zharness -> zcall 0.1.2 -> zconv 0.2.2. No sibling MCP repository or local
integration path is needed. Dependency audit rejects reintroduced integration/
content or MCP checkouts in workflows.

The manifest/lockfile candidate is committed locally before package verification.
Linux/Windows/macOS x stable/MSRV, standalone examples, no_std, WASM, dependency
audit and package/publish-dry-run checks gate publication. Only cargo publish
receives CARGO_REGISTRY_TOKEN; no cargo login, secret argument or token logging.

A non-force version push rejects concurrent main changes before registry upload.
Tag and GitHub Release are created only after cargo publish --locked succeeds,
in the same workflow rather than relying on a token-created tag to trigger more
work. Release concurrency never cancels an active publication. Registry failure
creates no tag/release, though a verified version commit may already exist.

If upload succeeds but GitHub tagging/release fails, recover the exact immutable
version's metadata rather than republishing or blindly bumping. Evidence workflows
exercise the next patch candidate without credentials and retain both benchmark
CSVs plus the source/environment. They do not imply that candidate was published.
