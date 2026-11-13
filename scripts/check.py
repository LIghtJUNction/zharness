#!/usr/bin/env python3
"""Run feature, contract and tool checks without publishing or external services."""

import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
FEATURES = ((), ("std",), ("wasm",), ("std", "wasm"))


def feature_args(features):
    return ["--no-default-features", *(["--features", ",".join(features)] if features else [])]


def main():
    started = time.monotonic()
    env = dict(os.environ, ZHARNESS_BENCH_SMOKE="1", RUSTDOCFLAGS="-D warnings", PYTHONUTF8="1")

    def run(*args, capture=False):
        result = subprocess.run(args, cwd=ROOT, env=env, check=True, text=True,
                                encoding="utf-8", stdout=subprocess.PIPE if capture else None)
        return result.stdout.strip() if capture else None

    rustc = run("rustc", "-Vv", capture=True)
    run("cargo", "fmt", "--all", "--", "--check")
    run("cargo", "clippy", "--locked", "--all-targets", "--all-features", "--", "-D", "warnings")
    run("cargo", "test", "--locked", "--all-targets")
    run("cargo", "test", "--locked", "--all-targets", "--all-features")
    run("cargo", "test", "--locked", "--all-targets", "--all-features", "--release")
    run("cargo", "test", "--locked", "--doc", "--all-features")
    run("cargo", "doc", "--locked", "--no-deps", "--all-features")
    for features in FEATURES:
        run("cargo", "check", "--locked", *feature_args(features))
    run("cargo", "run", "--locked", "--no-default-features", "--example", "skills")
    run(sys.executable, "-m", "unittest", "discover", "-s", "scripts", "-p", "test_*.py", "-v")
    run(sys.executable, "scripts/audit.py")
    run(sys.executable, "scripts/examples.py")
    # Real Wasm plugins are also exercised by integration tests; this runner
    # verifies the independent example build and explicit mixed-backend host.
    run(sys.executable, "scripts/wasm.py")
    smoke = json.loads(run(sys.executable, "scripts/wasm_bench.py", "--smoke", capture=True))
    target = ROOT / "target/evidence"
    target.mkdir(parents=True, exist_ok=True)
    record = {"result": "pass", "rustc": rustc, "platform": platform.platform(),
              "source_commit": run("git", "rev-parse", "HEAD", capture=True),
              "feature_configurations": len(FEATURES), "real_native_plugins": True,
              "real_wasm_plugins": True, "wasm_benchmark_smoke": smoke,
              "elapsed_seconds": round(time.monotonic() - started, 3)}
    (target / "check.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(record))


if __name__ == "__main__":
    main()
