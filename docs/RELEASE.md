# Release contract

The native primitives are currently source version 0.2.0, unreleased. The older
0.1.1 published package does not acquire source-only APIs automatically. Ordinary
pushes run tests and dry-run evidence; they do not publish to the registry.

Dispatch Release on the trusted default branch. Select patch/minor/major or an
exact SemVer; manual publish defaults to false. To release the current candidate
without a further bump, set version=0.2.0 explicitly. Alternatively, push a matching
.github/release-request.json on main with crate, exact version, exact zcall pin
and publish=true. The retained historical 0.1.1 request does not match 0.2.0 and
will fail closed if incorrectly reused. Tag pushes do not initiate publication.

Owner/version preflight checks crates.io and rejects network errors, collisions,
existing versions and missing/yanked exact dependencies. The complete production
dependency graph is zharness -> zcall 0.1.2 -> zconv 0.2.2. No sibling MCP repository
or local integration path is needed. Dependency audit also rejects reintroduced
integration/ content or MCP checkouts in workflows.

The candidate manifest and lockfile are updated and locally committed before
package verification, without --allow-dirty. Linux/Windows/macOS × stable/MSRV,
all standalone examples, no_std, WASM, dependency audit and package/dry-run checks
gate publication. Only cargo publish receives CARGO_REGISTRY_TOKEN; no cargo login,
secret command argument or printed token is used.

A non-force version push rejects concurrent main changes before registry upload.
Tag and GitHub Release creation occur only after cargo publish --locked succeeds,
in the same workflow rather than relying on a token-created tag to trigger more
work. Release concurrency never cancels an active publication. On registry
failure no new tag/release is created, although a version commit may remain.

If registry publication succeeds but a later GitHub operation fails, recover the
exact immutable version's tag/release; do not blindly republish or bump again.
The Native harness evidence workflow separately exercises the next patch version,
package verification and publish dry-run without registry write credentials.
