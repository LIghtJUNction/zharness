# Release contract

The 0.2.2 patch adds execution hooks to published 0.2.1 without changing existing
lifecycle enums or requiring changes to existing Hooks implementations. The exact
production closure remains zharness -> zcall 0.1.2 -> zconv 0.2.2. No MCP workspace,
command runtime or optional third-party dependency is introduced.

Dispatch Release on the trusted default branch. Select patch/minor/major or an
exact SemVer; manual publish defaults to false. To publish this already-bumped
candidate, set version=0.2.2. Alternatively, update .github/release-request.json on
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
