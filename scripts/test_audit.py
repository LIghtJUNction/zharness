"""The dependency policy rejects hidden runtime additions and source overrides."""

import copy
import unittest

from audit import PINS, runtime_names, validate_manifest


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


if __name__ == "__main__":
    unittest.main()
