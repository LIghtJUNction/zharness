"""Fail closed on unreviewed dependencies and distinguish optional Wasm runtime costs."""

import json
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
REPOSITORIES = {
    "zcall": "zcall", "zconv": "zconv", "zguard": "zguard",
    "zplugin": "zplugin", "zplugin-macros": "zplugin", "zsys": "zsys",
}
GIT_SOURCES = {name: "https://github.com/LIghtJUNction/" + repo + ".git"
               for name, repo in REPOSITORIES.items()}
DIRECT = {"zcall", "zplugin", "zguard"}
NATIVE = {"zharness", "zcall", "zconv", "zplugin", "zsys", "zguard"}
MACROS = {"zplugin-macros", "syn", "quote", "proc-macro2", "unicode-ident"}
WASM = {"wasmi", "wasmi_core", "wasmi_ir", "wasmi_collections",
        "wasmparser", "spin", "libm", "bitflags"}
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"


def validate_manifest(manifest):
    assert set(manifest.get("dependencies", {})) == DIRECT
    for name in DIRECT:
        dependency = manifest["dependencies"][name]
        assert isinstance(dependency, dict), name
        expected = {"git": GIT_SOURCES[name], "default-features": False}
        if name == "zplugin":
            expected["features"] = ["macros"]
        assert dependency == expected, (name, "development dependencies must remain canonical Git-only")
    assert manifest["features"] == {
        "default": ["std"], "std": ["zplugin/std"], "wasm": ["zplugin/wasm"]}
    assert not manifest.get("dev-dependencies") and not manifest.get("build-dependencies")
    assert not manifest.get("target"), "target-specific dependencies require review"


def locked_packages(lock, manifest):
    """Bind each reviewed package name to one exact lock version and source."""
    entries = lock.get("package", [])
    packages = {p["name"]: p for p in entries}
    assert len(packages) == len(entries), "duplicate Cargo lock package names require review"
    assert set(packages) == NATIVE | MACROS | WASM, "unreviewed or missing locked package"
    root = packages["zharness"]
    assert root.get("source") is None and root["version"] == manifest["package"]["version"]
    for name, package in packages.items():
        assert isinstance(package.get("version"), str) and package["version"], name
        if name in GIT_SOURCES:
            prefix = "git+" + GIT_SOURCES[name] + "#"
            source = package.get("source", "")
            assert isinstance(source, str) and source.startswith(prefix), (name, "wrong Git lock source")
            assert re.fullmatch(r"[0-9a-f]{40}", source[len(prefix):]), (name, "missing exact Git commit")
            assert "checksum" not in package, (name, "Git package has registry checksum")
        elif name != "zharness":
            assert package.get("source") == REGISTRY, (name, "unreviewed registry source")
            assert re.fullmatch(r"[0-9a-f]{64}", package.get("checksum", "")), (name, "missing registry checksum")
    assert packages["zplugin"]["source"] == packages["zplugin-macros"]["source"], "plugin workspace commits differ"
    assert packages["zplugin"]["version"] == packages["zplugin-macros"]["version"], "plugin workspace versions differ"
    return packages


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


def validate_metadata(metadata, manifest, wasm, lock=None):
    validate_manifest(manifest)
    if lock is None:
        lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    locked = locked_packages(lock, manifest)
    expected_runtime = NATIVE | (WASM if wasm else set())
    assert {p["name"] for p in metadata["packages"]} == expected_runtime | MACROS
    assert runtime_names(metadata) == expected_runtime
    packages = {p["name"]: p for p in metadata["packages"]}
    assert len(packages) == len(metadata["packages"]), "duplicate Cargo package names require review"
    root = packages["zharness"]
    assert root.get("source") is None
    assert metadata["resolve"]["root"] == root["id"], "wrong metadata root"
    assert Path(metadata["workspace_root"]).resolve() == ROOT.resolve(), "wrong metadata workspace"
    assert Path(root["manifest_path"]).resolve() == (ROOT / "Cargo.toml").resolve(), "wrong root manifest"
    assert root["version"] == manifest["package"]["version"]
    dependencies = {d["name"]: d for d in root["dependencies"]}
    assert len(dependencies) == len(root["dependencies"]) == len(DIRECT)
    assert set(dependencies) == DIRECT
    for name, dependency in dependencies.items():
        # Cargo documents "*" for an omitted requirement. The Git package's
        # exact version and source are checked against Cargo.lock below.
        assert dependency["req"] == "*" and not dependency["uses_default_features"]
        assert dependency["source"] in {"git+" + GIT_SOURCES[name], locked[name]["source"]}
        assert dependency.get("kind") is None and dependency.get("target") is None
        assert dependency.get("rename") is None and not dependency.get("optional")
        assert not dependency.get("path") and not dependency.get("registry")
        assert dependency["features"] == (["macros"] if name == "zplugin" else [])
    for name, package in packages.items():
        assert package["version"] == locked[name]["version"], (name, "metadata/lock version mismatch")
        assert package.get("source") == locked[name].get("source"), (name, "metadata/lock source mismatch")
    if wasm:
        assert packages["wasmi"]["version"] == "2.0.0"
    return sorted(expected_runtime)


def main():
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    validate_manifest(manifest)
    locked = locked_packages(lock, manifest)
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
        closure = validate_metadata(metadata, manifest, wasm, lock)
        evidence.append({"configuration": name, "runtime_packages": closure,
                         "third_party_runtime_packages": len(WASM) if wasm else 0})
    print(json.dumps({"result": "pass", "crate": "zharness",
                      "version": manifest["package"]["version"],
                      "compilation_only_packages": sorted(MACROS),
                      "locked_internal_packages": [
                          {"name": name, "version": locked[name]["version"], "source": locked[name]["source"]}
                          for name in sorted(GIT_SOURCES)],
                      "configurations": evidence}, indent=2))


if __name__ == "__main__":
    main()
