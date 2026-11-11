# Release contract

Dispatch Release only on the trusted default branch. Select patch/minor/major or
an exact legal SemVer; publish defaults to false. The owner and version preflight
checks crates.io before publication and rejects network failures, collisions,
existing versions and missing/yanked exact dependencies. CI verifies the closure
zharness -> zcall 0.1.2 -> zconv 0.2.2; local integration paths are not packaged.

The candidate's manifest and lockfile are updated and locally committed before
cargo package verification, without --allow-dirty. Linux/Windows/macOS and stable/
MSRV gate the publish job. Only cargo publish receives CARGO_REGISTRY_TOKEN. It is
not passed in command arguments, written with cargo login or logged. Tag and
GitHub Release creation happen only after a successful cargo publish --locked,
inside the same workflow. No tag-trigger chaining is required.

Concurrent default-branch edits stop the non-force version push before registry
publication. The release concurrency group never cancels an active publish. A
registry failure leaves no tag or release; a validated version commit may remain.
If registry publication succeeds and GitHub tagging fails, recover the exact
immutable version rather than trying to republish or blindly bumping again.

Composition and release evidence independently runs a full local benchmark and
exercises a 0.1.2 candidate bump, package verification and publish dry-run without
registry write credentials. It does not claim a real release occurred.
