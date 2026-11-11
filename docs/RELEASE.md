# Release contract

Published 0.2.3 contains lifecycle hooks. The source checkout additionally has
skills and native extension primitives. The runtime closure is zharness ->
zcall 0.1.2 -> zconv 0.2.2 and zharness -> zplugin 0.1.0. The dependency audit
checks the zplugin macro's compilation graph separately. Native libraries used
by tests/examples are built locally from packaged sources; no sibling repository
checkout, MCP workspace or background runtime is required.

Dispatch Release on the trusted default branch. Select patch/minor/major or an
exact SemVer; manual publish defaults to false. Select a new version above the
current published release. Alternatively, update .github/release-request.json on
main with the exact crate/version/zcall pin and publish=true. Old mismatched markers
fail closed. Ordinary source/tag pushes never initiate registry publication.

Preflight rejects ownership conflicts, already published versions, missing/yanked
exact dependencies and failed network checks. Linux/Windows/macOS x stable/MSRV,
all examples (including real subprocess hooks), no_std, WASM, dependency audit,
package verification and publish dry-run gate the actual publication job.

Only cargo publish receives CARGO_REGISTRY_TOKEN. No cargo login, secret command
argument or token logging is used. A non-force version push stops on concurrent
main changes. Tag and GitHub Release are created only after cargo publish --locked
succeeds, in the same workflow. Release concurrency never cancels active uploads.

If upload succeeds but tag/release creation fails, recover metadata for the same
immutable version rather than blindly republishing or bumping. Evidence workflows
validate a separate next-patch candidate without registry write credentials and
retain source, environment and raw CSVs; they do not publish that candidate.
