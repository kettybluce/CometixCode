#!/usr/bin/env python3
"""Sequential, offline CLI startup measurements; run via limited-run.sh."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import signal
import shutil
import statistics
import subprocess
import tempfile
import time
from datetime import datetime, timezone


CASES = {"version": ["--version"], "help": ["--help"], "mcp-list": ["mcp", "list"]}


def fingerprint(path):
    resolved = Path(path).resolve(strict=True)
    digest = hashlib.sha256()
    with resolved.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return {
        "path": str(resolved),
        "sha256": digest.hexdigest(),
        "bytes": resolved.stat().st_size,
    }


def isolated_env(root):
    env = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith(("ANTHROPIC_", "CLAUDE_", "COMETIX_"))
    }
    env.update(
        HOME=str(root),
        CLAUDE_CONFIG_DIR=str(root / "config"),
        XDG_CONFIG_HOME=str(root / "xdg"),
        CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1",
        DISABLE_TELEMETRY="1",
        DISABLE_AUTOUPDATER="1",
        COMETIX_WRITE_ENABLED="1",
        NO_COLOR="1",
        TERM="dumb",
    )
    return env


def measure(binary, args, cwd, env, stats_path):
    memory = dict(
        line.split(":", 1) for line in Path("/proc/meminfo").read_text().splitlines()
    )
    if int(memory["MemAvailable"].split()[0]) < 4 * 1024 * 1024:
        raise RuntimeError("Available memory fell below 4 GiB; deferred benchmark")
    started = time.perf_counter_ns()
    process = subprocess.Popen(
        ["/usr/bin/time", "-f", "%M %U %S", "-o", str(stats_path), str(binary), *args],
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    try:
        stdout, stderr = process.communicate(timeout=30)
    except BaseException:
        # GNU time launches a child: stop the whole owned process group.
        os.killpg(process.pid, signal.SIGKILL)
        process.communicate()
        raise
    elapsed = (time.perf_counter_ns() - started) / 1_000_000
    if process.returncode:
        # Do not retain an accidental credential-bearing diagnostic in a report.
        raise RuntimeError(f"{Path(binary).name} {args} exited {process.returncode}")
    if not stdout.strip():
        raise RuntimeError(f"{Path(binary).name} {args} returned empty stdout")
    rss, user, system = stats_path.read_text().strip().split()
    return {
        "wallMs": elapsed,
        "peakRssKiB": int(rss),
        "userSeconds": float(user),
        "systemSeconds": float(system),
        "exitCode": process.returncode,
        "stdoutSha256": hashlib.sha256(stdout).hexdigest(),
        "stdoutBytes": len(stdout),
        "stderrBytes": len(stderr),
    }


def summary(samples):
    walls = sorted(sample["wallMs"] for sample in samples)
    return {
        "samples": len(samples),
        "medianMs": statistics.median(walls),
        "p95Ms": walls[math.ceil(len(walls) * 0.95) - 1],
        "minMs": walls[0],
        "maxMs": walls[-1],
        "medianPeakRssKiB": statistics.median(s["peakRssKiB"] for s in samples),
        "maxPeakRssKiB": max(s["peakRssKiB"] for s in samples),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--original", default=shutil.which("claude"))
    parser.add_argument("--cometix", default="target/release/cometix")
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--warmup", type=int, default=3)
    parser.add_argument("--cases", nargs="+", choices=CASES, default=list(CASES))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.samples < 20 or args.warmup < 3:
        parser.error("Use at least 20 samples and 3 warmups per case")
    if not args.original:
        parser.error("Original Claude Code binary was not found")
    if args.output.exists():
        parser.error("Output exists; use a new attempt path to preserve evidence")

    binaries = {
        "original": Path(args.original).resolve(strict=True),
        "cometix": Path(args.cometix).resolve(strict=True),
    }
    report = {
        "at": datetime.now(timezone.utc).isoformat(),
        "binaries": {name: fingerprint(path) for name, path in binaries.items()},
        "machine": {"cpuCount": os.cpu_count(), "loadBefore": os.getloadavg()},
        "method": {
            "warmups": args.warmup,
            "samples": args.samples,
            "order": "alternating binaries, sequential processes",
            "wallIncludes": "process launch, /usr/bin/time wrapper, stdout/stderr collection",
            "cache": "OS caches retained; process restarts are not cold-cache measurements",
            "scope": "offline CLI startup only; not model latency or functional parity",
        },
        "status": "running",
        "cases": {},
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    try:
        with tempfile.TemporaryDirectory(prefix="cometix-compare-") as scratch:
            root = Path(scratch)
            cwd = root / "project"
            cwd.mkdir()
            envs = {}
            for name in binaries:
                home = root / name
                home.mkdir()
                envs[name] = isolated_env(home)
            stats_path = root / "time.txt"
            for case in args.cases:
                results = {name: [] for name in binaries}
                report["cases"][case] = {"args": CASES[case], "raw": results}
                for index in range(args.warmup + args.samples):
                    names = list(binaries)
                    if index % 2:
                        names.reverse()
                    for name in names:
                        sample = measure(binaries[name], CASES[case], cwd, envs[name], stats_path)
                        if index >= args.warmup:
                            results[name].append(sample)
                summaries = {name: summary(samples) for name, samples in results.items()}
                report["cases"][case]["summary"] = summaries
                report["cases"][case]["cometixToOriginalMedianRatio"] = (
                    summaries["cometix"]["medianMs"] / summaries["original"]["medianMs"]
                )
                print(case, json.dumps(summaries, ensure_ascii=False), flush=True)
        report["status"] = "completed"
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        report["status"] = "failed"
        report["errorType"] = type(error).__name__
        raise
    finally:
        report["machine"]["loadAfter"] = os.getloadavg()
        args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
