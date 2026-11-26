#!/usr/bin/env bash
# Run development work without competing with the machine's existing services.
set -euo pipefail

if [[ $# -eq 0 ]]; then
    printf 'Usage: bash scripts/limited-run.sh <command> [args...]\n' >&2
    exit 2
fi

if ! command -v systemd-run >/dev/null || ! systemctl --user show-environment >/dev/null 2>&1; then
    printf 'A user systemd session is required to enforce CPU and memory limits.\n' >&2
    exit 1
fi

if ! command -v taskset >/dev/null; then
    printf 'taskset is required to keep child processes on one logical CPU.\n' >&2
    exit 1
fi
# User cgroups do not always delegate the CPU controller: systemd may accept
# CPUQuota while cpu.max is absent. Pin the process tree as well, so runtime
# worker pools inherit one available CPU even on those hosts.
allowed_cpus=$(awk '/^Cpus_allowed_list:/ { print $2 }' /proc/self/status)
cpu_id=${allowed_cpus%%[-,]*}
if [[ ! $cpu_id =~ ^[0-9]+$ ]]; then
    printf 'Cannot determine an allowed CPU; refusing an unbounded run.\n' >&2
    exit 1
fi

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
lock_dir=${XDG_RUNTIME_DIR:-/tmp}/cometix-dev-${UID}
mkdir -p -- "$lock_dir"
chmod 700 -- "$lock_dir"
exec 9>"$lock_dir/limited-run.lock"
if ! flock --nonblock 9; then
    printf 'Another resource-limited Cometix development command is running.\n' >&2
    exit 1
fi

available_kib=$(awk '/^MemAvailable:/ { print $2 }' /proc/meminfo)
if (( available_kib < 4 * 1024 * 1024 )); then
    printf 'Less than 4 GiB is available; defer this command until memory recovers.\n' >&2
    exit 1
fi

export PATH="${HOME}/.cargo/bin:${PATH}"
export CARGO_BUILD_JOBS=1
export NEXTEST_TEST_THREADS=1
export RUST_TEST_THREADS=1
export RAYON_NUM_THREADS=1
export CARGO_TERM_COLOR=never
# The monolithic crate's debug/incremental metadata otherwise dominates RAM.
# These affect development artifacts only, never the release benchmark profile.
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0
cd -- "$repo_root"

memory_high=${COMETIX_MEMORY_HIGH:-3G}
memory_max=${COMETIX_MEMORY_MAX:-3G}
memory_swap_max=${COMETIX_MEMORY_SWAP_MAX:-0}

printf 'Limits: 1 logical CPU, %s memory high, %s memory maximum, %s swap; one task at a time.\n' \
    "$memory_high" "$memory_max" "$memory_swap_max" >&2
systemd-run --user --scope --quiet --unit="cometix-dev-$$" \
    -p CPUQuota=100% -p MemoryHigh="$memory_high" -p MemoryMax="$memory_max" \
    -p MemorySwapMax="$memory_swap_max" -p IOWeight=10 \
    taskset --cpu-list "$cpu_id" nice -n 15 ionice -c 3 "$@"
