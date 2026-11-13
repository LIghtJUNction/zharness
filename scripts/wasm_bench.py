#!/usr/bin/env python3
"""Measure Wasm adapter/Executor/router costs against matched zplugin calls.

Three fresh processes retain every batch sample. Quantiles describe batch means,
not individual-request latency. Use --smoke for a real guest-backed CI check.
"""
import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
SIZES = (0, 2, 64, 4096, 1_048_576)
CASES = ({(name, size) for name in ("direct_bytes", "extension_bytes") for size in SIZES}
         | {(name, size) for name in ("direct_text", "executor", "router") for size in SIZES[1:]}
         | {("direct_load_drop", 0), ("extension_load_drop", 0)})


def command(*arguments, env=None):
    result = subprocess.run(list(map(str, arguments)), cwd=ROOT, env=env, check=False,
                            timeout=600, text=True, encoding="utf-8",
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.stderr:
        print(result.stderr, end="", file=sys.stderr, flush=True)
    if result.returncode:
        print(result.stdout, end="", file=sys.stderr)
        raise SystemExit(f"command failed ({result.returncode}): {' '.join(map(str, arguments))}")
    return result.stdout


def parse_samples(output, count):
    headers = {}
    for line in output.splitlines():
        if line.startswith("# ") and "=" in line:
            name, value = line[2:].split("=", 1)
            headers[name] = value
    rows = list(csv.DictReader(line for line in output.splitlines() if not line.startswith("#")))
    found = {(row["name"], int(row["bytes"])) for row in rows}
    if found != CASES:
        raise SystemExit(f"unexpected benchmark cases: {found ^ CASES}")
    summaries = []
    for name, size in sorted(CASES):
        case = [row for row in rows if row["name"] == name and int(row["bytes"]) == size]
        if len(case) != count or {int(row["sample"]) for row in case} != set(range(1, count + 1)):
            raise SystemExit(f"missing or duplicate samples: {name}/{size}")
        if any(int(row["net_live_bytes"]) != 0 for row in case):
            raise SystemExit(f"net live memory growth: {name}/{size}")
        if not name.endswith("load_drop") and any(int(row["allocations"]) for row in case):
            raise SystemExit(f"warmed call allocated: {name}/{size}")
        timings = sorted(float(row["ns_per_op"]) for row in case)
        total = sum(int(row["iterations"]) for row in case)
        summaries.append({
            "name": name, "bytes": size,
            "p50_batch_mean_ns": timings[(count - 1) * 50 // 100],
            "p95_batch_mean_ns": timings[(count - 1) * 95 // 100],
            "p99_batch_mean_ns": timings[(count - 1) * 99 // 100],
            "min_batch_mean_ns": timings[0], "max_batch_mean_ns": timings[-1],
            "allocations_per_op": sum(int(row["allocations"]) for row in case) / total,
            "requested_bytes_per_op": sum(int(row["requested_bytes"]) for row in case) / total,
            "net_live_bytes": sum(int(row["net_live_bytes"]) for row in case),
            "peak_extra_requested_bytes": max(int(row["peak_extra_bytes"]) for row in case),
        })
    return headers, summaries


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--guest", type=Path, help="reuse this independently compiled Wasm guest")
    parser.add_argument("--output", type=Path, default=ROOT / "artifacts/wasm-bench")
    parser.add_argument("--smoke", action="store_true")
    options = parser.parse_args()
    guest = options.guest
    if guest is None:
        built = command(sys.executable, "scripts/wasm.py", "--build-only")
        guest = Path(built.strip().splitlines()[-1])
    guest = guest.resolve()
    if not guest.is_file():
        raise SystemExit(f"missing Wasm guest: {guest}")
    build_command = ["cargo", "bench", "--locked", "--no-run", "--bench", "wasm_plugins",
                     "--no-default-features", "--features", "std,wasm", "--message-format=json"]
    built = command(*build_command)
    executable = None
    compiler_profile = None
    for line in built.splitlines():
        event = json.loads(line)
        if (event.get("reason") == "compiler-artifact"
                and event.get("target", {}).get("name") == "wasm_plugins"
                and event.get("executable")):
            executable = event["executable"]
            compiler_profile = event.get("profile")
    if executable is None:
        raise SystemExit("Cargo did not report the wasm_plugins benchmark executable")

    affinity = None
    affinity_note = "CPU affinity is unavailable on this platform"
    if hasattr(os, "sched_getaffinity") and hasattr(os, "sched_setaffinity"):
        try:
            affinity = min(os.sched_getaffinity(0))
            os.sched_setaffinity(0, {affinity})
            affinity_note = "pinned to the minimum CPU in the inherited allowed set"
        except OSError as error:
            affinity = None
            affinity_note = f"CPU affinity was not applied: {error}"
    env = dict(os.environ, ZHARNESS_WASM_PLUGIN=str(guest))
    if options.smoke:
        env["ZHARNESS_BENCH_SMOKE"] = "1"
        output = command(executable, env=env)
        headers, summaries = parse_samples(output, 3)
        print(json.dumps({"suite": "wasm-benchmark-smoke", "result": "pass",
                          "cases": len(summaries), "batches_per_case": 3,
                          "guest_sha256": hashlib.sha256(guest.read_bytes()).hexdigest(),
                          "headers": headers}))
        return
    env.pop("ZHARNESS_BENCH_SMOKE", None)
    cpu = platform.processor()
    if Path("/proc/cpuinfo").is_file():
        cpu = next((line.split(":", 1)[1].strip() for line in
                    Path("/proc/cpuinfo").read_text().splitlines()
                    if line.startswith("model name")), cpu)
    metadata = {
        "suite": "zharness-wasm-adapter", "processes": 3, "batches_per_case_per_process": 31,
        "quantiles": "per-batch average ns/op, not individual-request tail latency",
        "quantile_method": "sorted[floor(p * (samples - 1))]",
        "source_commit": command("git", "rev-parse", "HEAD").strip(),
        "working_tree": command("git", "status", "--porcelain").strip(),
        "rustc": command("rustc", "-Vv").strip(),
        "cargo": command("cargo", "-V").strip(), "build_command": build_command,
        "platform": platform.platform(), "machine": platform.machine(), "cpu": cpu,
        "affinity_cpu": affinity, "affinity_note": affinity_note,
        "guest_bytes": guest.stat().st_size,
        "guest_sha256": hashlib.sha256(guest.read_bytes()).hexdigest(),
        "benchmark_sha256": hashlib.sha256(Path(executable).read_bytes()).hexdigest(),
        "profile": "bench,opt-level=3,lto=thin,codegen-units=1,panic=unwind",
        "cargo_reported_profile": compiler_profile,
        "profile_environment_overrides": {name: value for name, value in os.environ.items()
                                          if name.startswith(("CARGO_PROFILE_BENCH_", "CARGO_PROFILE_RELEASE_"))},
        "rustflags": os.environ.get("RUSTFLAGS", ""),
        "cargo_encoded_rustflags": os.environ.get("CARGO_ENCODED_RUSTFLAGS", ""),
        "allocation_scope": "Rust System allocator requested Layout sizes; excludes allocator metadata, OS/RSS and internal realloc temporaries",
        "comparison": "same guest, fuel/limits, ownership, serialized calls and copies; direct_text includes the UTF-8 validation used by Executor/Router",
        "excluded_from_warm_timing": ["module loading/binding", "route setup", "JSON argument validation", "input/output allocation"],
        "input_policy": "0B raw calls only; 2B empty JSON object and 64B/4KiB/1MiB JSON objects for all variants",
        "sources": {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
                    for path in sorted(list((ROOT / "src").rglob("*.rs"))
                                       + [ROOT / "benches/wasm_plugins.rs", ROOT / "scripts/wasm_bench.py",
                                          ROOT / "scripts/wasm.py", ROOT / "examples/wasm/guest.rs",
                                          ROOT / "Cargo.toml", ROOT / "Cargo.lock"])},
    }
    destination = options.output.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    summaries = []
    run_headers = []
    for repeat in range(1, 4):
        output = command(executable, env=env)
        headers, run_summary = parse_samples(output, 31)
        run_headers.append(headers)
        name = f"wasm-{platform.system().lower()}-{platform.machine()}-run{repeat}.csv"
        (destination / name).write_text(output, encoding="utf-8")
        summaries.extend({"run": repeat, **row} for row in run_summary)
        print(f"Saved {destination / name}", flush=True)
    metadata["run_headers"] = run_headers
    metadata["summaries"] = summaries
    (destination / "evidence.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    stream = io.StringIO()
    writer = csv.DictWriter(stream, fieldnames=list(summaries[0]), lineterminator="\n")
    writer.writeheader()
    writer.writerows(summaries)
    (destination / "summary.csv").write_text(stream.getvalue(), encoding="utf-8", newline="\n")
    print("name,bytes,median_p50_batch_mean_ns,min_p50_ns,max_p50_ns,allocations_per_op")
    for name, size in sorted(CASES):
        rows = [row for row in summaries if row["name"] == name and row["bytes"] == size]
        values = [row["p50_batch_mean_ns"] for row in rows]
        print(f"{name},{size},{statistics.median(values):.3f},{min(values):.3f},{max(values):.3f},"
              f"{rows[0]['allocations_per_op']:.3f}")


if __name__ == "__main__":
    main()
