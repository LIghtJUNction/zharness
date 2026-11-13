"""Release contract tests: untrusted inputs, immutable refs, exact source and ownership."""

import json
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import release


class VersionTests(unittest.TestCase):
    def test_stable_and_prerelease_semver(self):
        self.assertLess(release.semver("1.0.0-alpha.9"), release.semver("1.0.0-alpha.10"))
        self.assertLess(release.semver("1.0.0-rc.1"), release.semver("1.0.0"))
        self.assertEqual(release.semver("1.0.0+a"), release.semver("1.0.0+b"))
        self.assertEqual(release.next_version("0.1.9", "patch"), "0.1.10")
        self.assertEqual(release.next_version("0.1.9", "minor"), "0.2.0")
        self.assertEqual(release.next_version("0.1.9", "major"), "1.0.0")
        self.assertEqual(release.next_version("0.1.9", "custom", "1.0.0-rc.1"), "1.0.0-rc.1")

    def test_rejects_ambiguous_or_injected_versions(self):
        for value in ["v1.0.0", "01.0.0", "1.0", "1.0.0-01", "1.0.0-", "1.0.0+",
                      "1.0.0\n", "1.0.0; touch owned", "$(id)", "1.0.0-α", "18446744073709551616.0.0"]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                release.semver(value)
        for bump, value in [("patch", "1.0.0"), ("custom", ""), ("custom", "0.0.1"),
                            ("custom", "0.1.0+metadata"), ("shell", "")]:
            with self.subTest(bump=bump, value=value), self.assertRaises(ValueError):
                release.next_version("0.1.0", bump, value)

    def test_request_is_exactly_typed_data(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "request.json"
            valid = {"bump": "custom", "version": "0.1.0", "dry_run": True}
            path.write_text(json.dumps(valid))
            self.assertEqual(release.request_settings(path), valid)
            for data in [dict(valid, dry_run="false"), dict(valid, extra=True), dict(valid, version=1)]:
                path.write_text(json.dumps(data))
                with self.assertRaises(ValueError):
                    release.request_settings(path)
            path.write_text('{"bump":"patch","bump":"major","version":"","dry_run":false}')
            with self.assertRaises(ValueError):
                release.request_settings(path)


class GitReleaseTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.old_directory = Path.cwd()
        self.repository = Path(self.directory.name) / "repository"
        self.repository.mkdir()
        os.chdir(self.repository)
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.invalid")
        Path(".gitignore").write_text("target/\n")
        Path("Cargo.toml").write_text('''[package]
name = "zharness"
version = "0.1.0"
repository = "https://github.com/LIghtJUNction/zharness"
[dependencies]
zcall = { version = "=0.2.0", default-features = false }
zplugin = { version = "=0.3.0", default-features = false, features = ["macros"] }
''')
        Path("Cargo.lock").write_text('''version = 4
[[package]]
name = "zharness"
version = "0.1.0"
dependencies = ["zcall", "zplugin"]
[[package]]
name = "zcall"
version = "0.2.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "preserved"
[[package]]
name = "zplugin"
version = "0.3.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "preserved-plugin"
''')
        Path("CHANGELOG.md").write_text("# Changes\n\n## Unreleased\n\nInitial implementation.\n")
        Path("README.md").write_text('zharness = { version = "0.1.0", features = ["std"] }\n')
        self.git("add", ".")
        self.git("commit", "-qm", "initial")
        self.source = self.git("rev-parse", "HEAD")
        self.remote = str(Path(self.directory.name) / "remote.git")
        subprocess.run(["git", "init", "--bare", "-q", self.remote], check=True)
        self.git("remote", "add", "origin", self.remote)
        self.git("push", "-q", "origin", "main")

    def tearDown(self):
        os.chdir(self.old_directory)
        self.directory.cleanup()

    def git(self, *args):
        return subprocess.check_output(["git", *args], text=True).strip()

    def test_candidate_commit_is_reproducible_and_pins_are_preserved(self):
        first = release.candidate("0.2.0", self.source)
        self.assertEqual(release.root_version(), "0.2.0")
        self.assertIn('version = "=0.2.0"', Path("Cargo.toml").read_text())
        lock = Path("Cargo.lock").read_text()
        self.assertIn('version = "0.3.0"', lock)
        self.assertIn('checksum = "preserved"', lock)
        self.assertIn('version = "0.2.0"', Path("README.md").read_text())
        self.assertNotIn("Unreleased", Path("CHANGELOG.md").read_text())
        self.assertEqual(release.changelog_notes("0.2.0"), "Initial implementation.")
        self.git("checkout", "--detach", self.source)
        second = release.candidate("0.2.0", self.source, first)
        self.assertEqual(first, second)
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_rejects_dirty_checkout(self):
        Path("README.md").write_text("uncommitted user work")
        with self.assertRaisesRegex(ValueError, "clean checkout"):
            release.candidate("0.2.0", self.source)
        self.assertEqual(Path("README.md").read_text(), "uncommitted user work")

    def test_changelog_promotes_bracketed_pending_and_preserves_history(self):
        Path("CHANGELOG.md").write_text("# Changes\n\n## [Unreleased]\n\n- New task ABI.\n\n## [0.1.0]\n\n- Original.\n")
        self.git("add", "CHANGELOG.md")
        self.git("commit", "-qm", "pending changes")
        source = self.git("rev-parse", "HEAD")
        release.candidate("0.2.0", source)
        self.assertEqual(release.changelog_notes("0.2.0"), "- New task ABI.")
        self.assertEqual(release.changelog_notes("0.1.0"), "- Original.")
        with self.assertRaisesRegex(ValueError, "missing"):
            release.changelog_notes("0.3.0")

    def test_rejects_unexpected_candidate_commit(self):
        with self.assertRaisesRegex(ValueError, "differs"):
            release.candidate("0.2.0", self.source, "f" * 40)
        self.assertEqual(self.git("ls-remote", "origin", "refs/heads/main").split()[0], self.source)

    def test_atomic_publish_refs_and_same_run_retry(self):
        candidate = release.candidate("0.2.0", self.source)
        with patch.object(release, "registry_preflight"):
            release.push_release("0.2.0", self.source, candidate, False)
            self.assertEqual(release.tag_provenance("v0.2.0"), candidate)
            # Diagnostic outputs are ignored just as in the real repository.
            release.push_release("0.2.0", self.source, candidate, False)
        self.assertEqual(self.git("ls-remote", "origin", "refs/heads/main").split()[0], candidate)
        self.assertIn("refs/tags/v0.2.0", self.git("ls-remote", "origin", "refs/tags/v0.2.0"))

    def test_foreign_tag_is_never_overwritten(self):
        candidate = release.candidate("0.2.0", self.source)
        self.git("tag", "v0.2.0", self.source)
        with patch.object(release, "registry_preflight"), self.assertRaises(ValueError):
            release.push_release("0.2.0", self.source, candidate, True)
        self.assertEqual(self.git("rev-parse", "v0.2.0"), self.source)
        self.assertEqual(self.git("ls-remote", "origin", "refs/heads/main").split()[0], self.source)

    def test_concurrent_main_advance_blocks_ref_updates(self):
        candidate = release.candidate("0.2.0", self.source)
        self.git("checkout", "--detach", self.source)
        Path("concurrent.txt").write_text("other author's work")
        self.git("add", ".")
        self.git("commit", "-qm", "concurrent")
        other = self.git("rev-parse", "HEAD")
        self.git("push", "-q", "origin", "HEAD:main")
        self.git("checkout", "--detach", candidate)
        with patch.object(release, "registry_preflight"), self.assertRaisesRegex(ValueError, "main advanced"):
            release.push_release("0.2.0", self.source, candidate, False)
        self.assertEqual(self.git("ls-remote", "origin", "refs/heads/main").split()[0], other)
        self.assertFalse(release.tag_exists("v0.2.0"))

    def test_stale_checkout_is_allowed_only_for_read_only_planning(self):
        Path("concurrent.txt").write_text("new main commit")
        self.git("add", ".")
        self.git("commit", "-qm", "concurrent")
        other = self.git("rev-parse", "HEAD")
        self.git("push", "-q", "origin", "HEAD:main")
        self.git("checkout", "--detach", self.source)
        with patch.dict(os.environ, {"GITHUB_REPOSITORY": release.REPOSITORY,
                                     "GITHUB_REF": "refs/heads/main"}):
            for dry_run in (False, True):
                settings = {"bump": "minor", "version": "", "dry_run": dry_run}
                with self.subTest(dry_run=dry_run), \
                        patch.object(release, "registry_preflight") as preflight, \
                        patch.object(release, "emit") as emit:
                    if dry_run:
                        release.plan(settings)
                        planned = emit.call_args.kwargs
                        self.assertEqual(planned["source"], self.source)
                        self.assertEqual(planned["candidate"], self.git("rev-parse", "HEAD"))
                        self.assertEqual(self.git("rev-parse", "HEAD^"), self.source)
                        self.assertTrue(planned["dry_run"])
                        preflight.assert_called_once_with("0.2.0", False)
                        # A dry-run candidate can never bypass the publication guard.
                        with self.assertRaisesRegex(ValueError, "main advanced"):
                            release.push_release("0.2.0", self.source, planned["candidate"], False)
                    else:
                        with self.assertRaisesRegex(ValueError, "main advanced"):
                            release.plan(settings)
                        self.assertEqual(self.git("rev-parse", "HEAD"), self.source)
                        preflight.assert_not_called()
                        emit.assert_not_called()
                    self.assertEqual(self.git("ls-remote", "origin", "refs/heads/main").split()[0], other)
                    self.assertFalse(release.tag_exists("v0.2.0"))

    def test_pr_and_fork_plans_are_read_only(self):
        contexts = [(release.REPOSITORY, "refs/pull/7/merge", "main only"),
                    ("contributor/zharness", "refs/heads/main", "owning repository"),
                    ("contributor/zharness", "refs/pull/7/merge", "owning repository")]
        for repository, ref, error in contexts:
            self.git("checkout", "--detach", self.source)
            with patch.dict(os.environ, {"GITHUB_REPOSITORY": repository, "GITHUB_REF": ref}), \
                    self.subTest(repository=repository, ref=ref), \
                    patch.object(release, "registry_preflight") as preflight, \
                    patch.object(release, "emit") as emit:
                settings = {"bump": "minor", "version": "", "dry_run": False}
                with self.assertRaisesRegex(ValueError, error):
                    release.plan(settings)
                self.assertEqual(self.git("rev-parse", "HEAD"), self.source)
                preflight.assert_not_called()
                emit.assert_not_called()
                release.plan(dict(settings, dry_run=True))
                planned = emit.call_args.kwargs
                self.assertEqual(planned["source"], self.source)
                self.assertEqual(planned["candidate"], self.git("rev-parse", "HEAD"))
                self.assertTrue(planned["dry_run"])
                preflight.assert_called_once_with("0.2.0", False)
                self.assertEqual(self.git("ls-remote", "origin", "refs/heads/main").split()[0], self.source)
                self.assertFalse(release.tag_exists("v0.2.0"))

    def test_package_hashes_require_exact_clean_commit_and_registry_sources(self):
        commit = release.candidate("0.2.0", self.source)
        path = Path("target/package/zharness-0.2.0.crate")
        path.parent.mkdir(parents=True)

        def archive(sha=commit, dirty=False, dependency='zcall = "=0.2.0"'):
            manifest = '[package]\nname = "zharness"\nversion = "0.2.0"\n[dependencies]\n' + dependency
            files = {"Cargo.toml": manifest,
                     ".cargo_vcs_info.json": json.dumps({"git": {"sha1": sha, "dirty": dirty}})}
            with tarfile.open(path, "w:gz") as output:
                for name, text in files.items():
                    data = text.encode()
                    info = tarfile.TarInfo("zharness-0.2.0/" + name)
                    info.size = len(data)
                    output.addfile(info, io.BytesIO(data))

        archive()
        self.assertEqual(set(release.package_hashes("0.2.0")), {"zharness"})
        for arguments in [{"sha": "f" * 40}, {"dirty": True},
                          {"dependency": 'zcall = { version = "=0.2.0", path = "../zcall" }'}]:
            archive(**arguments)
            with self.subTest(arguments=arguments), self.assertRaises(ValueError):
                release.package_hashes("0.2.0")


class RegistryTests(unittest.TestCase):
    def test_exact_pins_are_checked_for_missing_or_yanked_versions(self):
        package = {"crate": {"repository": "https://github.com/LIghtJUNction/zharness"}, "versions": []}
        owner = {"users": [{"login": "LIghtJUNction"}]}
        manifest = '[dependencies]\nzcall = { version = "=0.2.0", default-features = false }\n'
        for result in (None, {"version": {"yanked": True}}):
            with patch.object(Path, "read_text", return_value=manifest), \
                    patch.object(release, "api", side_effect=[package, owner, result]), \
                    self.subTest(result=result), self.assertRaisesRegex(ValueError, "missing or yanked"):
                release.registry_preflight("0.4.0")
        with patch.object(Path, "read_text", return_value=manifest), \
                patch.object(release, "api", side_effect=[package, owner, {"version": {"yanked": False}}]) as api:
            release.registry_preflight("0.4.0")
            self.assertEqual(api.call_args.args, ("crates/zcall/0.2.0",))

    def test_loose_or_non_registry_pins_are_rejected(self):
        package = {"crate": {"repository": "https://github.com/LIghtJUNction/zharness"}, "versions": []}
        owner = {"users": [{"login": "LIghtJUNction"}]}
        for value in ['"0.2"', '{ version = "0.2" }',
                      '{ version = "=0.2.0", path = "../zcall" }',
                      '{ version = "=0.2.0", git = "https://example.invalid/zcall" }']:
            with patch.object(Path, "read_text", return_value='[dependencies]\nzcall = ' + value), \
                    patch.object(release, "api", side_effect=[package, owner]), \
                    self.subTest(value=value), self.assertRaisesRegex(ValueError, "exact registry"):
                release.registry_preflight("0.4.0")

    def test_foreign_owner_or_repository_blocks_publication(self):
        data = {"crate": {"repository": "https://example.invalid/other"}, "versions": []}
        with patch.object(release, "api", return_value=data), self.assertRaisesRegex(ValueError, "another repository"):
            release.registry_preflight("0.1.0")
        data["crate"]["repository"] = "https://github.com/LIghtJUNction/zharness"
        with patch.object(release, "api", side_effect=[data, {"users": [{"login": "other"}]}]), self.assertRaisesRegex(ValueError, "owner"):
            release.registry_preflight("0.1.0")

    def test_registry_collision_and_checksum_mismatch_fail_closed(self):
        data = {"crate": {"repository": "https://github.com/LIghtJUNction/zharness"}, "versions": [{"num": "0.1.0"}]}
        with patch.object(release, "api", side_effect=[data, {"users": [{"login": "LIghtJUNction"}]}]), self.assertRaisesRegex(ValueError, "already exists"):
            release.registry_preflight("0.1.0")
        with patch.object(release, "api", return_value={"version": {"checksum": "foreign", "yanked": False}}), self.assertRaisesRegex(ValueError, "checksum differs"):
            release.registry_matches("zharness", "0.1.0", "expected")

    def test_missing_publish_token_stops_before_registry_request(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(release, "api") as api:
            with self.assertRaisesRegex(ValueError, "CARGO_REGISTRY_TOKEN"):
                release.publish("0.1.0")
            api.assert_not_called()

    def test_confirmed_identical_upload_is_idempotent(self):
        with patch.object(release, "api", return_value={"version": {"checksum": "same", "yanked": False}}):
            self.assertTrue(release.registry_matches("zharness", "0.1.0", "same"))
        with patch.object(release, "api", return_value={"version": {"checksum": "same", "yanked": True}}), self.assertRaisesRegex(ValueError, "yanked"):
            release.registry_matches("zharness", "0.1.0", "same")


if __name__ == "__main__":
    unittest.main()
