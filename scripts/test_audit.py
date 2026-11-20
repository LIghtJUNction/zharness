"""The dependency policy rejects hidden runtime additions and source overrides."""

import copy
import unittest

from audit import PINS, REGISTRY, runtime_names, validate_manifest, validate_metadata


class ManifestTests(unittest.TestCase):
    def manifest(self):
        dependencies = {name: {"version": pin, "default-features": False}
                        for name, pin in PINS.items()}
        dependencies["zplugin"]["features"] = ["macros"]
        return {"dependencies": dependencies, "features": {
            "default": ["std"], "std": ["zplugin/std"], "wasm": ["zplugin/wasm"]}}

    def test_explicit_optional_engine_policy(self):
        validate_manifest(self.manifest())

    def test_extra_runtime_or_build_dependency_fails(self):
        for section in ("dependencies", "build-dependencies", "dev-dependencies"):
            manifest = self.manifest()
            manifest.setdefault(section, {})["surprise"] = "1"
            with self.subTest(section=section), self.assertRaises(AssertionError):
                validate_manifest(manifest)

    def test_sources_and_implicit_default_features_fail(self):
        for change in ({"path": "../zplugin"}, {"git": "https://example.invalid/plugin"},
                       {"default-features": True}, {"optional": True}, {"version": "0.3"}):
            manifest = self.manifest()
            manifest["dependencies"]["zplugin"].update(change)
            with self.subTest(change=change), self.assertRaises(AssertionError):
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
    def snapshot(self):
        manifest = ManifestTests().manifest()
        manifest["package"] = {"version": "0.5.0"}
        versions = {"zharness": "0.5.0", "zcall": "0.4.0", "zconv": "0.4.0",
                    "zplugin": "0.3.1", "zsys": "0.1.1", "zplugin-macros": "0.3.1",
                    "syn": "2.0.0", "quote": "1.0.0", "proc-macro2": "1.0.0",
                    "unicode-ident": "1.0.0"}
        graph = {"zharness": ["zcall", "zplugin"], "zcall": ["zconv"],
                 "zplugin": ["zsys", "zplugin-macros"],
                 "zplugin-macros": ["syn", "quote", "proc-macro2", "unicode-ident"]}
        packages = [{"id": name, "name": name, "version": version,
                     "source": None if name == "zharness" else REGISTRY,
                     "targets": [{"kind": ["proc-macro" if name == "zplugin-macros" else "lib"]}]}
                    for name, version in versions.items()]
        packages[0]["dependencies"] = [
            {"name": name, "req": pin, "uses_default_features": False}
            for name, pin in PINS.items()]
        nodes = [{"id": name, "deps": [{"pkg": dep, "dep_kinds": [{"kind": None}]}
                                     for dep in graph.get(name, [])]}
                 for name in versions]
        return manifest, {"packages": packages, "resolve": {"root": "zharness", "nodes": nodes}}

    def test_shared_system_library_is_in_the_exact_runtime_closure(self):
        manifest, metadata = self.snapshot()
        self.assertEqual(validate_metadata(metadata, manifest, False),
                         ["zcall", "zconv", "zharness", "zplugin", "zsys"])

    def test_old_plugin_macro_or_system_versions_are_rejected(self):
        for name, version in [("zplugin", "0.3.0"), ("zplugin-macros", "0.3.0"),
                              ("zsys", "0.1.0")]:
            manifest, metadata = self.snapshot()
            next(p for p in metadata["packages"] if p["name"] == name)["version"] = version
            with self.subTest(name=name), self.assertRaises(AssertionError):
                validate_metadata(metadata, manifest, False)

    def test_duplicate_plugin_versions_are_rejected(self):
        manifest, metadata = self.snapshot()
        duplicate = copy.deepcopy(next(p for p in metadata["packages"] if p["name"] == "zplugin"))
        duplicate.update(id="old-plugin", version="0.3.0")
        metadata["packages"].insert(0, duplicate)
        metadata["resolve"]["nodes"].append({"id": "old-plugin", "deps": []})
        with self.assertRaisesRegex(AssertionError, "duplicate"):
            validate_metadata(metadata, manifest, False)

    def test_system_source_override_is_rejected(self):
        manifest, metadata = self.snapshot()
        next(p for p in metadata["packages"] if p["name"] == "zsys")["source"] = None
        with self.assertRaises(AssertionError):
            validate_metadata(metadata, manifest, False)


if __name__ == "__main__":
    unittest.main()
