#!/usr/bin/env python3
"""Read kernel-enforced limits from inside scripts/limited-run.sh."""
import json
import os
from datetime import datetime
from pathlib import Path
import subprocess
import sys


def main():
    own_cgroup = next(
        line.split("::", 1)[1]
        for line in Path("/proc/self/cgroup").read_text().splitlines()
        if line.startswith("0::")
    )
    root = Path("/sys/fs/cgroup") / own_cgroup.lstrip("/")
    values = {}
    for key in ["cpu.max", "memory.high", "memory.max", "memory.swap.max"]:
        path = root / key
        values[key] = path.read_text().strip() if path.is_file() else None
    affinity = sorted(os.sched_getaffinity(0))
    io_priority = subprocess.run(
        ["ionice", "-p", str(os.getpid())], capture_output=True, text=True, check=True,
    ).stdout.strip()
    nested = subprocess.run(
        ["bash", "scripts/limited-run.sh", "true"], capture_output=True, text=True,
        cwd=Path(__file__).resolve().parent.parent,
    )
    checks = {
        "singleCpuAffinity": len(affinity) == 1,
        "memoryHigh3GiB": values["memory.high"] == str(3 * 1024**3),
        "memoryMax3GiB": values["memory.max"] == str(3 * 1024**3),
        "noSwap": values["memory.swap.max"] == "0",
        "nice15": os.getpriority(os.PRIO_PROCESS, 0) == 15,
        "idleIoPriority": io_priority.startswith("idle"),
        "cargoSingleJob": os.environ.get("CARGO_BUILD_JOBS") == "1",
        "testsSingleThread": os.environ.get("NEXTEST_TEST_THREADS") == "1",
        "rustTestsSingleThread": os.environ.get("RUST_TEST_THREADS") == "1",
        "rayonSingleThread": os.environ.get("RAYON_NUM_THREADS") == "1",
        "nestedRunRejected": nested.returncode == 1 and "Another resource-limited" in nested.stderr,
    }
    print(json.dumps({"at": datetime.now().astimezone().isoformat(), "kernelLimits": values,
                      "cpuAffinity": affinity, "ioPriority": io_priority, "checks": checks}, indent=2))
    return 0 if all(checks.values()) else 1


if __name__ == "__main__":
    sys.exit(main())
