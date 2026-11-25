#!/usr/bin/env bash
# Disposable hosted runner verification; the local entry remains limited-run.sh.
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
mkdir -p ci-evidence
git rev-parse HEAD > ci-evidence/source-commit.txt
git status --porcelain > ci-evidence/source-status.txt
rustc --version > ci-evidence/rustc.txt
cargo nextest --version > ci-evidence/nextest.txt
free -b > ci-evidence/runner-memory.txt
sha256sum Cargo.toml Cargo.lock > ci-evidence/dependencies.sha256

# Explicit integration targets do not silently omit binary-level CLI coverage,
# and do not pull the monolithic --lib test executable into this small gate.
targets=()
for source in tests/*.rs; do
    name=${source##*/}
    targets+=(--test "${name%.rs}")
done
if (( ${#targets[@]} == 0 )); then
    printf 'No integration test targets found.\n' >&2
    exit 1
fi
printf '%s\n' "${targets[@]}" > ci-evidence/test-targets.txt

set +e
/usr/bin/time -v -o ci-evidence/tests.resources \
    cargo nextest run --locked --no-fail-fast --test-threads=1 \
    "${targets[@]}" 2>&1 | tee ci-evidence/tests.log
status=${PIPESTATUS[0]}
set -e
printf '%s\n' "$status" > ci-evidence/tests.exit-code

# Even after a failed test, retain a completed build for local diagnosis.
if [[ -f target/debug/cometix ]]; then
    sha256sum target/debug/cometix > ci-evidence/cometix.sha256
    stat --printf='%s\n' target/debug/cometix > ci-evidence/cometix.bytes
fi
exit "$status"
