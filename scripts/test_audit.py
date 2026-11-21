"""The dependency policy binds Git sources to the lock and rejects hidden costs."""

import copy
from pathlib import Path
import unittest
import audit
from audit import REGISTRY, runtime_names, validate_manifest, validate_metadata

DIRECT = {"zcall": "zcall", "zplugin": "zplugin", "zguard": "zguard"}
INTERNAL = {**DIRECT, "zconv": "zconv", "zsys": "zsys", "zplugin-macros": "zplugin"}
WASM = {"wasmi", "wasmi_core", "wasmi_ir", "wasmi_collections",
        "wasmparser", "spin", "libm", "bitflags"}


class ManifestTests(unittest.TestCase):
    def manifest(self):
        dependencies = {name: {"git": "https://github.com/LIghtJUNction/" + repo + ".git",
                               "default-features": False} for name, repo in DIRECT.items()}
        dependencies["zplugin"]["features"] = ["macros"]
        return {"package": {"name": "zharness", "version": "0.6.0"},
                "dependencies": dependencies, "features": {
                    "default": ["std"], "std": ["zplugin/std"], "wasm": ["zplugin/wasm"]}}

    def test_explicit_optional_engine_policy(self):
        try:
            validate_manifest(self.manifest())
        except Exception as error:
            self.fail("Canonical Git-only development manifest was rejected: " + repr(error))

    def test_extra_runtime_or_build_dependency_fails(self):
        for section in ("dependencies", "build-dependencies", "dev-dependencies"):
            manifest = self.manifest()
            manifest.setdefault(section, {})["surprise"] = "1"
            with self.subTest(section=section), self.assertRaises(AssertionError):
                validate_manifest(manifest)

    def test_sources_and_implicit_default_features_fail(self):
        for change in ({"path": "../zplugin"}, {"git": "https://example.invalid/plugin"},
                       {"default-features": True}, {"optional": True}, {"version": "0.3"},
                       {"version": "=0.3.1"}, {"version": "*"}, {"rev": "a" * 40},
                       {"branch": "main"}, {"tag": "v0.3.1"}, {"features": ["std"]}):
            manifest = self.manifest()
            manifest["dependencies"]["zplugin"].update(change)
            with self.subTest(change=change), self.assertRaises(AssertionError):
                validate_manifest(manifest)

    def test_registry_fallback_and_target_dependencies_fail(self):
        manifest = self.manifest()
        manifest["dependencies"]["zcall"].pop("git")
        manifest["dependencies"]["zcall"]["version"] = "=0.4.0"
        with self.assertRaises(AssertionError):
            validate_manifest(manifest)
        manifest = self.manifest()
        manifest["target"] = {"cfg(unix)": {"dependencies": {"surprise": "1"}}}
        with self.assertRaises(AssertionError):
            validate_manifest(manifest)

    def test_wasm_must_not_imply_std_or_become_default(self):
        for features in (["zplugin/wasm", "std"], ["zplugin/wasm", "zplugin/std"]):
            manifest = self.manifest()
            manifest["features"]["wasm"] = features
            with self.assertRaises(AssertionError):
                validate_manifest(manifest)
        manifest = self.manifest()
        manifest["features"]["default"].append("wasm")
        with self.assertRaises(AssertionError):
            validate_manifest(manifest)


class ClosureTests(unittest.TestCase):
    def test_proc_macros_and_build_dependencies_are_not_runtime(self):
        def package(name, kind="lib"):
            return {"id": name, "name": name, "targets": [{"kind": [kind]}]}

        def dependency(name, kind=None):
            return {"pkg": name, "dep_kinds": [{"kind": kind}]}

        metadata = {"packages": [package("root"), package("normal"),
                                 package("macro", "proc-macro"), package("parser"), package("build")],
                    "resolve": {"root": "root", "nodes": [
                        {"id": "root", "deps": [dependency("normal"), dependency("macro"),
                                                   dependency("build", "build")]},
                        {"id": "normal", "deps": []},
                        {"id": "macro", "deps": [dependency("parser")]},
                        {"id": "parser", "deps": []}, {"id": "build", "deps": []}]}}
        self.assertEqual(runtime_names(metadata), {"root", "normal"})
        changed = copy.deepcopy(metadata)
        changed["resolve"]["nodes"][0]["deps"].append(dependency("parser"))
        self.assertEqual(runtime_names(changed), {"root", "normal", "parser"})


class ExactClosureTests(unittest.TestCase):
    def snapshot(self, wasm=False):
        manifest = ManifestTests().manifest()
        versions = {"zharness": "0.6.0", "zguard": "0.1.0", "zcall": "0.4.0", "zconv": "0.4.0",
                    "zplugin": "0.3.1", "zsys": "0.1.1", "zplugin-macros": "0.3.1",
                    "syn": "2.0.0", "quote": "1.0.0", "proc-macro2": "1.0.0",
                    "unicode-ident": "1.0.0", **{name: "2.0.0" for name in WASM}}
        graph = {"zharness": ["zcall", "zplugin", "zguard"], "zcall": ["zconv"],
                 "zplugin": ["zsys", "zplugin-macros"] + (["wasmi"] if wasm else []),
                 "zplugin-macros": ["syn", "quote", "proc-macro2", "unicode-ident"],
                 "wasmi": sorted(WASM - {"wasmi"})}
        lock = {"version": 4, "package": []}
        for name, version in versions.items():
            entry = {"name": name, "version": version}
            if name in INTERNAL:
                entry["source"] = "git+https://github.com/LIghtJUNction/" + INTERNAL[name] + ".git#" + "a" * 40
            elif name != "zharness":
                entry.update(source=REGISTRY, checksum="b" * 64)
            lock["package"].append(entry)
        packages = [{"id": name, "name": name, "version": entry["version"],
                     "source": entry.get("source"),
                     "manifest_path": str(audit.ROOT / "Cargo.toml") if name == "zharness" else "/dependency/" + name,
                     "targets": [{"kind": ["proc-macro" if name == "zplugin-macros" else "lib"]}]}
                    for entry in lock["package"] if (name := entry["name"]) not in WASM or wasm]
        packages[0]["dependencies"] = [
            {"name": name, "req": "*", "uses_default_features": False,
             "source": "git+https://github.com/LIghtJUNction/" + repo + ".git",
             "features": ["macros"] if name == "zplugin" else [],
             "kind": None, "target": None, "rename": None, "optional": False}
            for name, repo in DIRECT.items()]
        nodes = [{"id": p["id"], "deps": [{"pkg": dep, "dep_kinds": [{"kind": None}]}
                                         for dep in graph.get(p["name"], [])]} for p in packages]
        metadata = {"packages": packages, "workspace_root": str(audit.ROOT),
                    "resolve": {"root": "zharness", "nodes": nodes}}
        return manifest, metadata, lock

    def test_shared_system_library_is_in_the_exact_runtime_closure(self):
        manifest, metadata, lock = self.snapshot()
        self.assertEqual(validate_metadata(metadata, manifest, False, lock),
                         ["zcall", "zconv", "zguard", "zharness", "zplugin", "zsys"])

    def test_optional_wasm_closure_keeps_registry_dependencies(self):
        manifest, metadata, lock = self.snapshot(wasm=True)
        result = validate_metadata(metadata, manifest, True, lock)
        self.assertTrue(WASM.issubset(result))
        self.assertNotIn("syn", result)

    def test_versions_are_bound_to_lock_not_obsolete_hardcoded_pins(self):
        manifest, metadata, lock = self.snapshot()
        for collection in (metadata["packages"], lock["package"]):
            next(p for p in collection if p["name"] == "zsys")["version"] = "0.2.0"
        validate_metadata(metadata, manifest, False, lock)

    def test_old_plugin_macro_or_system_versions_are_rejected(self):
        for name, version in [("zplugin", "0.3.0"), ("zplugin-macros", "0.3.0"),
                              ("zsys", "0.1.0")]:
            manifest, metadata, lock = self.snapshot()
            next(p for p in metadata["packages"] if p["name"] == name)["version"] = version
            with self.subTest(name=name), self.assertRaises(AssertionError):
                validate_metadata(metadata, manifest, False, lock)

    def test_duplicate_plugin_versions_are_rejected(self):
        manifest, metadata, lock = self.snapshot()
        duplicate = copy.deepcopy(next(p for p in metadata["packages"] if p["name"] == "zplugin"))
        duplicate.update(id="old-plugin", version="0.3.0")
        metadata["packages"].insert(0, duplicate)
        metadata["resolve"]["nodes"].append({"id": "old-plugin", "deps": []})
        with self.assertRaisesRegex(AssertionError, "duplicate"):
            validate_metadata(metadata, manifest, False, lock)

    def test_metadata_source_replacement_or_commit_drift_is_rejected(self):
        for source in (None, REGISTRY, "git+https://example.invalid/zsys.git#" + "a" * 40,
                       "git+https://github.com/LIghtJUNction/zsys.git#" + "c" * 40):
            manifest, metadata, lock = self.snapshot()
            next(p for p in metadata["packages"] if p["name"] == "zsys")["source"] = source
            with self.subTest(source=source), self.assertRaises(AssertionError):
                validate_metadata(metadata, manifest, False, lock)

    def test_missing_ambiguous_and_noncanonical_locked_sources_fail(self):
        for fault in ("missing", "duplicate", "registry", "foreign", "unlocked", "extra"):
            manifest, metadata, lock = self.snapshot()
            entry = next(p for p in lock["package"] if p["name"] == "zcall")
            if fault == "missing":
                lock["package"].remove(entry)
            elif fault == "duplicate":
                lock["package"].append(copy.deepcopy(entry))
            elif fault == "registry":
                entry["source"] = REGISTRY
            elif fault == "foreign":
                entry["source"] = "git+https://example.invalid/zcall.git#" + "a" * 40
            elif fault == "unlocked":
                entry["source"] = "git+https://github.com/LIghtJUNction/zcall.git"
            else:
                lock["package"].append({"name": "surprise", "version": "1.0.0"})
            with self.subTest(fault=fault), self.assertRaises(AssertionError):
                validate_metadata(metadata, manifest, False, lock)

    def test_plugin_and_macro_must_share_lock_source_and_version(self):
        for field, value in (("source", "git+https://github.com/LIghtJUNction/zplugin.git#" + "c" * 40),
                             ("version", "0.3.2")):
            manifest, metadata, lock = self.snapshot()
            next(p for p in lock["package"] if p["name"] == "zplugin-macros")[field] = value
            next(p for p in metadata["packages"] if p["name"] == "zplugin-macros")[field] = value
            with self.subTest(field=field), self.assertRaises(AssertionError):
                validate_metadata(metadata, manifest, False, lock)

    def test_declared_dependency_metadata_cannot_change_sources_or_features(self):
        for change in ({"req": "=0.4.0"}, {"source": REGISTRY}, {"uses_default_features": True},
                       {"kind": "dev"}, {"optional": True}, {"path": "/tmp/zcall"},
                       {"features": ["std"]}, {"target": "cfg(unix)"}, {"rename": "alias"}):
            manifest, metadata, lock = self.snapshot()
            next(d for d in metadata["packages"][0]["dependencies"] if d["name"] == "zcall").update(change)
            with self.subTest(change=change), self.assertRaises(AssertionError):
                validate_metadata(metadata, manifest, False, lock)

    def test_external_registry_checksum_and_source_are_required(self):
        for change in ({"source": None}, {"source": "git+https://example.invalid/syn#" + "a" * 40},
                       {"checksum": ""}, {"checksum": "not-a-checksum"}):
            manifest, metadata, lock = self.snapshot()
            next(p for p in lock["package"] if p["name"] == "syn").update(change)
            with self.subTest(change=change), self.assertRaises(AssertionError):
                validate_metadata(metadata, manifest, False, lock)

    def test_root_identity_cannot_be_redirected(self):
        for change in ("workspace", "manifest", "root"):
            manifest, metadata, lock = self.snapshot()
            if change == "workspace":
                metadata["workspace_root"] = str(audit.ROOT / "foreign")
            elif change == "manifest":
                metadata["packages"][0]["manifest_path"] = str(audit.ROOT / "foreign" / "Cargo.toml")
            else:
                metadata["resolve"]["root"] = "zcall"
            with self.subTest(change=change), self.assertRaises(AssertionError):
                validate_metadata(metadata, manifest, False, lock)


if __name__ == "__main__":
    unittest.main()
