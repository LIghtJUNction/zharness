"""Fail closed on unreviewed dependencies and distinguish optional Wasm runtime costs."""

import json
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
PINS = {"zcall": "=0.2.0", "zplugin": "=0.3.0"}
NATIVE = {"zharness", "zcall", "zconv", "zplugin"}
MACROS = {"zplugin-macros", "syn", "quote", "proc-macro2", "unicode-ident"}
WASM = {"wasmi", "wasmi_core", "wasmi_ir", "wasmi_collections",
        "wasmparser", "spin", "libm", "bitflags"}
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"


def validate_manifest(manifest):
    assert set(manifest.get("dependencies", {})) == set(PINS)
    for name, version in PINS.items():
        dependency = manifest["dependencies"][name]
        assert dependency["version"] == version and dependency.get("default-features") is False
        assert not any(key in dependency for key in ("path", "git", "optional"))
    assert manifest["dependencies"]["zplugin"]["features"] == ["macros"]
    assert manifest["features"] == {
        "default": ["std"], "std": ["zplugin/std"], "wasm": ["zplugin/wasm"]}
    assert not manifest.get("dev-dependencies") and not manifest.get("build-dependencies")
    assert not manifest.get("target"), "target-specific dependencies require review"


def runtime_names(metadata):
    """Follow normal dependencies, stopping at compiler-only proc-macro targets."""
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    seen, pending = set(), [metadata["resolve"]["root"]]
    while pending:
        identifier = pending.pop()
        if identifier in seen:
            continue
        seen.add(identifier)
        for dependency in nodes[identifier]["deps"]:
            target = packages[dependency["pkg"]]
            if any("proc-macro" in target["kind"] for target in target["targets"]):
                continue
            if any(kind["kind"] is None for kind in dependency["dep_kinds"]):
                pending.append(dependency["pkg"])
    return {packages[identifier]["name"] for identifier in seen}


def validate_metadata(metadata, manifest, wasm):
    expected_runtime = NATIVE | (WASM if wasm else set())
    assert {p["name"] for p in metadata["packages"]} == expected_runtime | MACROS
    assert runtime_names(metadata) == expected_runtime
    packages = {p["name"]: p for p in metadata["packages"]}
    root = packages["zharness"]
    assert len(root["dependencies"]) == len(PINS)
    assert root["version"] == manifest["package"]["version"]
    for dependency in root["dependencies"]:
        assert dependency["req"] == PINS[dependency["name"]]
        assert not dependency["uses_default_features"]
    for name, expected in {"zcall": "0.2.0", "zconv": "0.3.0",
                           "zplugin": "0.3.0", "zplugin-macros": "0.3.0"}.items():
        assert packages[name]["version"] == expected, (name, packages[name]["version"])
    if wasm:
        assert packages["wasmi"]["version"] == "2.0.0"
    for name, package in packages.items():
        if name != "zharness":
            assert package["source"] == REGISTRY, name
    return sorted(expected_runtime)


def main():
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    validate_manifest(manifest)
    assert not (ROOT / "build.rs").exists(), "build.rs is forbidden"
    assert not (ROOT / "integration").exists(), "MCP integration belongs in its own repository"
    for workflow in (ROOT / ".github/workflows").glob("*.yml"):
        content = workflow.read_text(encoding="utf-8")
        assert "integration/mcp" not in content and "repository: LIghtJUNction/zmcp" not in content
    cases = [
        ("core", ["--no-default-features"], False),
        ("default", [], False),
        ("wasm-no-std", ["--no-default-features", "--features", "wasm"], True),
        ("all", ["--all-features"], True),
    ]
    evidence = []
    for name, features, wasm in cases:
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--locked", "--format-version", "1", *features],
            cwd=ROOT, text=True, encoding="utf-8"))
        closure = validate_metadata(metadata, manifest, wasm)
        evidence.append({"configuration": name, "runtime_packages": closure,
                         "third_party_runtime_packages": len(WASM) if wasm else 0})
    print(json.dumps({"result": "pass", "crate": "zharness",
                      "version": manifest["package"]["version"],
                      "compilation_only_packages": sorted(MACROS),
                      "configurations": evidence}, indent=2))


if __name__ == "__main__":
    main()
