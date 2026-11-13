#!/usr/bin/env python3
"""Build an independent core-only Wasm guest and execute pure/mixed plugin hosts."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def run(*command, toolchain=None, capture=False):
    env = dict(os.environ)
    if toolchain:
        env["RUSTUP_TOOLCHAIN"] = toolchain
    print("+", " ".join(map(str, command)), flush=True)
    result = subprocess.run(list(map(str, command)), cwd=ROOT, env=env,
                            check=True, timeout=300, text=True, encoding="utf-8",
                            stdout=subprocess.PIPE if capture else None)
    return result.stdout.strip() if capture else ""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--guest-toolchain")
    parser.add_argument("--host-toolchain")
    parser.add_argument("--build-only", action="store_true",
                        help="build the guest without executing hosts; print its path last")
    options = parser.parse_args()
    started = time.monotonic()
    label = re.sub(r"[^A-Za-z0-9_.-]", "_", os.environ.get("RUSTUP_TOOLCHAIN", "current"))
    for name, value in [("guest", options.guest_toolchain), ("host", options.host_toolchain)]:
        if value:
            label += "-" + name + "-" + re.sub(r"[^A-Za-z0-9_.-]", "_", value)
    output = ROOT / "target" / ("wasm-" + label)
    output.mkdir(parents=True, exist_ok=True)
    guest = output / "plugin.wasm"
    sdk_target = output / "sdk-build"
    sdk = sdk_target / "wasm32-unknown-unknown" / "release"
    guest_rust = run("rustc", "-Vv", toolchain=options.guest_toolchain, capture=True)
    host_rust = run("rustc", "-Vv", toolchain=options.host_toolchain, capture=True)
    run("rustfmt", "--edition=2024", "--check", ROOT / "examples/wasm/guest.rs",
        toolchain=options.guest_toolchain)
    sdk_build = run("cargo", "build", "--locked", "--release", "-p", "zharness", "--lib",
        "--no-default-features", "--target=wasm32-unknown-unknown", "--target-dir", sdk_target,
        "--message-format=json", toolchain=options.guest_toolchain, capture=True)
    # Cargo's build-directory layout differs across supported compiler versions.
    # Use reported artifact parents, including native procedural-macro libraries.
    directories = set()
    for line in sdk_build.splitlines():
        artifact = json.loads(line)
        if artifact.get("reason") == "compiler-artifact":
            directories.update(str(Path(path).parent) for path in artifact["filenames"])
    search_paths = [value for path in sorted(directories) for value in ("-L", "dependency=" + path)]
    run("rustc", "--edition=2024", "--crate-type=cdylib", "--crate-name=zharness_wasm_example",
        "--target=wasm32-unknown-unknown", "-Copt-level=3", "-Cpanic=abort",
        "-Ccodegen-units=1", "-Clto=thin", "-Clink-arg=-zstack-size=65536",
        "--extern", "zharness=" + str(sdk / "libzharness.rlib"),
        *search_paths,
        ROOT / "examples/wasm/guest.rs", "-o", guest, toolchain=options.guest_toolchain)
    if options.build_only:
        print(guest, flush=True)
        return

    checks = {}
    wasm_output = run("cargo", "run", "--locked", "--release", "--example", "wasm_host",
                      "--no-default-features", "--features", "std,wasm", "--", guest,
                      toolchain=options.host_toolchain, capture=True)
    print(wasm_output, flush=True)
    checks["wasm"] = json.loads(wasm_output.splitlines()[-1])
    native_build = run("cargo", "build", "--locked", "--release", "--example", "native_plugin",
                       "--no-default-features", "--features", "std", "--message-format=json",
                       toolchain=options.host_toolchain, capture=True)
    libraries = []
    for line in native_build.splitlines():
        artifact = json.loads(line)
        if artifact.get("reason") == "compiler-artifact" and artifact["target"]["name"] == "native_plugin":
            libraries += [Path(name) for name in artifact["filenames"]
                          if Path(name).suffix in {".so", ".dylib", ".dll"}]
    if len(libraries) != 1:
        raise RuntimeError("Cargo did not report exactly one native plugin library")
    mixed_output = run("cargo", "run", "--locked", "--release", "--example", "mixed_plugin_host",
                       "--no-default-features", "--features", "std,wasm", "--", libraries[0], guest,
                       toolchain=options.host_toolchain, capture=True)
    print(mixed_output, flush=True)
    checks["mixed"] = json.loads(mixed_output.splitlines()[-1])
    assert checks["wasm"]["checks"] == 8 and checks["mixed"]["checks"] == 3
    assert all(result["status"] == "passed" for result in checks.values())
    evidence = {"suite": "zharness-independent-wasm", "status": "passed",
                "platform": platform.platform(), "guest_rustc": guest_rust,
                "host_rustc": host_rust, "guest_target": "wasm32-unknown-unknown",
                "guest_linear_stack_bytes": 65_536, "guest_bytes": guest.stat().st_size,
                "guest_sha256": hashlib.sha256(guest.read_bytes()).hexdigest(),
                "host_checks": checks,
                "elapsed_seconds": round(time.monotonic() - started, 3)}
    artifact = ROOT / "artifacts" / ("wasm-" + label + ".json")
    artifact.parent.mkdir(exist_ok=True)
    artifact.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(evidence, indent=2), flush=True)


if __name__ == "__main__":
    main()
