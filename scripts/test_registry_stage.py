"""Registry packaging contracts: local Git and mocked Cargo/registry only."""
import copy
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import tomllib
import unittest
from unittest.mock import patch

import release

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
GIT_PACKAGES = (
    ("zcall", "0.4.0", "zcall", "564ede15db66530a6dbb78e8b1f4a85998b6f0d8", ["zconv"]),
    ("zconv", "0.4.0", "zconv", "0af1f7eb0766ff0ce9db5cee7c6184eff511a8c1", []),
    ("zguard", "0.1.0", "zguard", "6772b932a65e18f64e13591dee6edb2f6b099406", []),
    ("zplugin", "0.3.1", "zplugin", "cf02c1d9db575b92b36297dfe1fde72de9ee51e4", ["wasmi", "zplugin-macros", "zsys"]),
    ("zplugin-macros", "0.3.1", "zplugin", "cf02c1d9db575b92b36297dfe1fde72de9ee51e4", []),
    ("zsys", "0.1.1", "zsys", "808fcfdb73584ca7ca122e20b76f0209f56ab0f7", []),
)
MANIFEST = '''[package]
name = "zharness"
version = "0.6.0"
repository = "https://github.com/LIghtJUNction/zharness"
[features]
default = ["std"]
std = ["zplugin/std"]
wasm = ["zplugin/wasm"]
[dependencies]
zcall = { git = "https://github.com/LIghtJUNction/zcall.git", default-features = false }
zplugin = { git = "https://github.com/LIghtJUNction/zplugin.git", default-features = false, features = ["macros"] }
zguard = { git = "https://github.com/LIghtJUNction/zguard.git", default-features = false }
'''
LOCK = '''version = 4
[[package]]
name = "zharness"
version = "0.6.0"
dependencies = ["zcall", "zplugin", "zguard"]
[[package]]
name = "wasmi"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
'''
for _name, _version, _repo, _sha, _dependencies in GIT_PACKAGES:
    LOCK += ("\n[[package]]\nname = " + json.dumps(_name) + "\nversion = " + json.dumps(_version)
             + "\nsource = " + json.dumps("git+https://github.com/LIghtJUNction/" + _repo + ".git#" + _sha)
             + "\ndependencies = " + json.dumps(_dependencies) + "\n")


class RegistryStageTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name).resolve()
        previous = Path.cwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, previous)
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        Path(".gitignore").write_text("target/\n", encoding="utf-8")
        Path("Cargo.toml").write_text(MANIFEST, encoding="utf-8")
        Path("Cargo.lock").write_text(LOCK, encoding="utf-8")
        Path("README.md").write_text('zharness = "0.6.0"\n', encoding="utf-8")
        Path("CHANGELOG.md").write_text("# Changes\n\n## Unreleased\n\nFixture.\n", encoding="utf-8")
        self.commit()
        self.developer = self.git("rev-parse", "HEAD")
        self.inputs = {name: Path(name).read_bytes() for name in ("Cargo.toml", "Cargo.lock")}
        self.requests = []
        self.commands = []
        self.rows = {}
        self.crates = {}
        self.versions = {}
        for name, version, repo, _, _ in GIT_PACKAGES:
            checksum = hashlib.sha256((name + ":" + version).encode()).hexdigest()
            self.crates[name] = {"crate": {"id": name, "repository": "https://github.com/LIghtJUNction/" + repo}}
            self.versions[name] = {"version": {"crate": name, "num": version, "checksum": checksum, "yanked": False}}
            self.rows[name] = {"name": name, "vers": version, "cksum": checksum, "yanked": False}

    def git(self, *args, cwd=None):
        return subprocess.check_output(["git", *args], cwd=cwd, text=True).strip()

    def commit(self):
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")

    def api(self, path):
        self.requests.append(path)
        parts = path.split("/")
        if parts[1] == "zharness":
            return None
        if len(parts) == 2:
            return copy.deepcopy(self.crates[parts[1]])
        self.assertEqual(parts[2], self.versions[parts[1]]["version"]["num"] if self.versions[parts[1]] else
                         next(p[1] for p in GIT_PACKAGES if p[0] == parts[1]))
        return copy.deepcopy(self.versions[parts[1]])

    def index(self, name, version):
        self.requests.append("index/" + name + "/" + version)
        return copy.deepcopy(self.rows[name])

    def original_unchanged(self):
        self.assertEqual(self.git("rev-parse", "HEAD"), self.developer)
        self.assertEqual(self.git("status", "--porcelain"), "")
        self.assertEqual({p: Path(p).read_bytes() for p in self.inputs}, self.inputs)

    def archive(self, *, vcs=None, dirty=False, lock=None, manifest=None):
        stage = self.root / "target/registry-stage"
        source_manifest = (stage / "Cargo.toml").read_bytes()
        version = tomllib.loads(source_manifest.decode())["package"]["version"]
        proof = json.loads(Path("target/release-projection.json").read_text())
        payload = {
            "Cargo.toml": manifest if manifest is not None else source_manifest,
            "Cargo.toml.orig": source_manifest,
            "Cargo.lock": lock if lock is not None else (stage / "Cargo.lock").read_bytes(),
            ".cargo_vcs_info.json": json.dumps({"git": {"sha1": vcs or proof["projection_commit"], "dirty": dirty},
                                               "path_in_vcs": ""}).encode(),
        }
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w") as archive:
            for name, data in payload.items():
                entry = tarfile.TarInfo(f"zharness-{version}/" + name)
                entry.size = len(data)
                archive.addfile(entry, io.BytesIO(data))
        path = self.root / "target/package" / f"zharness-{version}.crate"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(gzip.compress(buffer.getvalue(), mtime=0))
        return path

    def mocked_run(self, *args, **kwargs):
        if args[0] != "cargo":
            return self.real_run(*args, **kwargs)
        self.commands.append((args, kwargs))
        self.assertIsNotNone(kwargs.get("cwd"), "Cargo packaging must receive the independent registry-stage cwd")
        self.assertEqual(Path(kwargs["cwd"]).resolve(), self.root / "target/registry-stage")
        self.assertEqual(kwargs["env"]["CARGO_TARGET_DIR"], str(self.root / "target"))
        manifest = tomllib.loads((Path(kwargs["cwd"]) / "Cargo.toml").read_text())
        self.assertTrue(all("git" not in dep and dep["version"].startswith("=")
                            for dep in manifest["dependencies"].values()),
                        "Cargo must receive exact registry dependencies")
        lock = tomllib.loads((Path(kwargs["cwd"]) / "Cargo.lock").read_text())
        self.assertFalse(any(p.get("source", "").startswith("git+") for p in lock["package"]),
                         "Cargo must receive the complete registry lock")
        self.original_unchanged()
        if args[1] == "metadata":
            self.assertEqual(args, ("cargo", "metadata", "--locked", "--format-version", "1", "--no-deps"))
            return json.dumps({"target_directory": str(self.root / "target")})
        if args[1] == "package":
            self.archive()
            return None
        self.assertIn(args, [
            ("cargo", "publish", "--locked", "--dry-run", "-p", "zharness", "--all-features"),
            ("cargo", "publish", "--locked", "-p", "zharness", "--all-features")])
        return subprocess.CompletedProcess(args, 0) if kwargs.get("check") is False else None

    def test_git_only_packages_project_before_mocked_cargo(self):
        self.real_run = release.run
        with patch.object(release, "api", side_effect=self.api), \
                patch.object(release, "index_version", side_effect=self.index, create=True), \
                patch.object(release, "run", side_effect=self.mocked_run):
            release.packages("0.6.0")
        self.original_unchanged()
        proof = json.loads(Path("target/release-projection.json").read_text())
        self.assertEqual(len(proof["dependencies"]), 6)
        self.assertNotEqual(proof["projection_commit"], self.developer)
        self.assertEqual(self.git("show", "-s", "--format=%P", proof["projection_commit"],
                                  cwd=self.root / "target/registry-stage"), self.developer)
        self.assertEqual(self.git("remote", cwd=self.root / "target/registry-stage"), "")
        self.assertFalse(proof["stage_refs_published"])
        self.assertEqual([c[0][1] for c in self.commands], ["package", "metadata", "publish", "metadata"])

    def test_git_only_preflight_uses_all_six_committed_tuples(self):
        with patch.object(release, "api", side_effect=self.api), \
                patch.object(release, "index_version", side_effect=self.index, create=True):
            try:
                release.registry_preflight("0.6.1")
            except ValueError as error:
                self.fail("A valid committed Git-only dependency closure must pass registry preflight: " + str(error))
        for name, version, _, _, _ in GIT_PACKAGES:
            self.assertIn("crates/" + name + "/" + version, self.requests)
            self.assertIn("index/" + name + "/" + version, self.requests)
        self.original_unchanged()
        self.assertFalse(Path("target/registry-stage").exists(), "preflight must not build a projection checkout")

    def prepare(self):
        return release.registry_stage.prepare(self.root, self.api, self.index)

    def test_six_package_projection_preserves_features_and_existing_registry(self):
        proof = self.prepare()
        stage = self.root / "target/registry-stage"
        manifest = tomllib.loads((stage / "Cargo.toml").read_text())
        original = tomllib.loads(MANIFEST)
        self.assertEqual(manifest["features"], original["features"])
        for name, spec in original["dependencies"].items():
            actual = manifest["dependencies"][name]
            version = next(p[1] for p in GIT_PACKAGES if p[0] == name)
            self.assertEqual(actual, {**{k: v for k, v in spec.items() if k != "git"}, "version": "=" + version})
        locked = tomllib.loads((stage / "Cargo.lock").read_text())["package"]
        self.assertEqual(next(p for p in locked if p["name"] == "wasmi"),
                         next(p for p in tomllib.loads(LOCK)["package"] if p["name"] == "wasmi"))
        for name, version, _, _, deps in GIT_PACKAGES:
            package = next(p for p in locked if p["name"] == name)
            self.assertEqual(package["version"], version)
            self.assertEqual(package["dependencies"], deps)
            self.assertEqual(package["source"], REGISTRY)
            self.assertEqual(package["checksum"], self.rows[name]["cksum"])
        pair = [p for p in proof["dependencies"] if p["name"] in ("zplugin", "zplugin-macros")]
        self.assertEqual(len(pair), 2)
        self.assertEqual(pair[0]["git"], pair[1]["git"])
        self.assertEqual(pair[0]["commit"], pair[1]["commit"])
        self.assertEqual({p["name"] for p in pair}, {"zplugin", "zplugin-macros"})
        self.original_unchanged()

    def test_missing_yanked_foreign_and_mismatched_registry_stop_before_cargo(self):
        original = copy.deepcopy((self.crates, self.versions, self.rows))
        for case in ("missing", "yanked", "repository", "checksum", "wrong-version"):
            self.crates, self.versions, self.rows = copy.deepcopy(original)
            if case == "missing":
                self.versions["zconv"] = None
            elif case == "yanked":
                self.versions["zconv"]["version"]["yanked"] = True
            elif case == "repository":
                self.crates["zplugin-macros"]["crate"]["repository"] = "https://github.com/other/macros"
            elif case == "checksum":
                self.rows["zsys"]["cksum"] = "f" * 64
            else:
                self.rows["zguard"]["vers"] = "9.0.0"
            with self.subTest(case=case), patch.object(release, "api", side_effect=self.api), \
                    patch.object(release, "index_version", side_effect=self.index), \
                    patch.object(release, "run") as cargo, self.assertRaises(ValueError):
                release.registry_preflight("0.6.1")
            cargo.assert_not_called()
            self.assertFalse(Path("target/registry-stage").exists())
            self.original_unchanged()

    def test_missing_ambiguous_or_wrong_git_lock_identity_fails_closed(self):
        chunks = LOCK.split("[[package]]")
        zcall_chunk = next(c for c in chunks[1:] if tomllib.loads("[[package]]" + c)["package"][0]["name"] == "zcall")
        variants = {
            "missing": LOCK.replace("[[package]]" + zcall_chunk, ""),
            "duplicate": LOCK + "[[package]]" + zcall_chunk,
            "repository": LOCK.replace("/zcall.git#", "/other.git#"),
            "qualified-source": LOCK.replace("/zcall.git#", "/zcall.git?branch=main#"),
        }
        for label, text in variants.items():
            self.git("reset", "--hard", self.developer)
            Path("Cargo.lock").write_text(text)
            self.commit()
            with self.subTest(case=label), patch.object(release, "api", side_effect=self.api), \
                    patch.object(release, "index_version", side_effect=self.index), \
                    patch.object(release, "run") as cargo, self.assertRaises(ValueError):
                release.registry_preflight("0.6.1")
            cargo.assert_not_called()
            self.assertFalse(Path("target/registry-stage").exists())

    def test_developer_version_bump_does_not_project_dependency_locations(self):
        before = tomllib.loads(LOCK)["package"]
        commit = release.candidate("0.6.1", self.developer)
        self.assertEqual(release.root_version(), "0.6.1")
        manifest = tomllib.loads(Path("Cargo.toml").read_text())
        self.assertEqual(manifest["dependencies"], tomllib.loads(MANIFEST)["dependencies"])
        after = tomllib.loads(Path("Cargo.lock").read_text())["package"]
        self.assertEqual([p for p in after if p["name"] != "zharness"],
                         [p for p in before if p["name"] != "zharness"])
        self.assertEqual(next(p["version"] for p in after if p["name"] == "zharness"), "0.6.1")
        proof = self.prepare()
        self.assertEqual(proof["developer_commit"], commit)
        self.assertNotEqual(proof["projection_commit"], commit)
        self.assertEqual(self.git("rev-parse", "HEAD"), commit)
        self.assertEqual(release.next_version(release.root_version(), "patch"), "0.6.2")
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_archives_reject_developer_head_dirty_git_and_checksum_mismatch(self):
        self.prepare()
        stage_lock = Path("target/registry-stage/Cargo.lock").read_bytes()
        self.real_run = release.run
        mutations = [
            {"vcs": self.developer},
            {"dirty": True},
            {"lock": stage_lock.replace(REGISTRY.encode(), b"git+https://github.com/LIghtJUNction/zcall.git#" + b"a" * 40, 1)},
            {"lock": stage_lock.replace(self.rows["zcall"]["cksum"].encode(), b"f" * 64)},
            {"manifest": Path("target/registry-stage/Cargo.toml").read_bytes() +
                         b'\n[target."cfg(unix)".dev-dependencies]\nforeign = {git="https://github.com/other/foreign.git"}\n'},
        ]
        self.archive()
        with patch.object(release, "run", side_effect=self.mocked_run):
            good = release.package_hashes("0.6.0")
            self.assertEqual(good["zharness"]["developer_commit"], self.developer)
            for mutation in mutations:
                self.archive(**mutation)
                with self.subTest(mutation=tuple(mutation)), self.assertRaises(ValueError):
                    release.package_hashes("0.6.0")

    def test_dirty_source_or_projection_stops_before_cargo(self):
        Path("README.md").write_text("uncommitted")
        with patch.object(release, "run") as cargo, self.assertRaisesRegex(ValueError, "clean developer"):
            release.packages("0.6.0")
        cargo.assert_not_called()
        self.git("checkout", "--", "README.md")
        self.prepare()
        Path("target/registry-stage/Cargo.toml").write_text("dirty")
        with patch.object(release, "run") as cargo, self.assertRaisesRegex(ValueError, "dirty or has changed"):
            release.package_hashes("0.6.0")
        cargo.assert_not_called()

    def test_archive_paths_cannot_escape_the_owned_checkout(self):
        self.prepare()
        self.archive()
        with tempfile.TemporaryDirectory() as directory:
            foreign = Path(directory).resolve()
            with patch.object(release, "run", return_value=json.dumps({"target_directory": str(foreign)})), \
                    self.assertRaisesRegex(ValueError, "owned directory"):
                release.package_directory()
            path = self.root / "target/package/zharness-0.6.0.crate"
            outside = foreign / path.name
            outside.write_bytes(path.read_bytes())
            path.unlink()
            try:
                path.symlink_to(outside)
            except OSError:
                # Platforms without symlink permission still verify the foreign-target guard.
                return
            with patch.object(release, "package_directory", return_value=path.parent), \
                    self.assertRaisesRegex(ValueError, "owned file"):
                release.package_hashes("0.6.0")

    def test_publish_and_resume_keep_developer_tag_and_projection_package(self):
        self.real_run = release.run
        with patch.object(release, "api", side_effect=self.api), \
                patch.object(release, "index_version", side_effect=self.index), \
                patch.object(release, "run", side_effect=self.mocked_run):
            release.packages("0.6.0")
        self.commands.clear()
        with patch.dict(os.environ, {"CARGO_REGISTRY_TOKEN": "fixture-not-a-real-token"}), \
                patch.object(release, "api", side_effect=self.api), \
                patch.object(release, "index_version", side_effect=self.index), \
                patch.object(release, "run", side_effect=self.mocked_run), \
                patch.object(release, "tag_provenance", return_value=self.developer), \
                patch.object(release, "registry_matches", side_effect=[False, True]) as matches, \
                patch.object(release, "wait_index") as wait:
            release.publish("0.6.0")
        self.assertEqual([c[0][1] for c in self.commands], ["metadata", "publish"])
        self.assertEqual(matches.call_count, 2)
        self.assertEqual(wait.call_count, 1)
        self.original_unchanged()
        self.commands.clear()
        with patch.dict(os.environ, {"CARGO_REGISTRY_TOKEN": "fixture-not-a-real-token"}), \
                patch.object(release, "api", side_effect=self.api), \
                patch.object(release, "index_version", side_effect=self.index), \
                patch.object(release, "run", side_effect=self.mocked_run), \
                patch.object(release, "tag_provenance", return_value=self.developer), \
                patch.object(release, "registry_matches", return_value=True), \
                patch.object(release, "wait_index"):
            release.publish("0.6.0")
        self.assertEqual([c[0][1] for c in self.commands], ["metadata"])
        self.original_unchanged()
