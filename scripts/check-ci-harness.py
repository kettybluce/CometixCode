#!/usr/bin/env python3
"""Check CI shell plumbing with isolated fake commands, never product tests."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def run_case(base, name, cargo_status, tee_status=0, has_target=True):
    root = base / name
    for directory in ("scripts", "tests", "bin"):
        (root / directory).mkdir(parents=True, exist_ok=True)
    source = Path(__file__).resolve().with_name("ci-cli-tests.sh")
    shutil.copyfile(source, root / "scripts/ci-cli-tests.sh")
    for filename in ("Cargo.toml", "Cargo.lock"):
        (root / filename).write_text("# isolated CI plumbing fixture\n")
    if has_target:
        (root / "tests/contract.rs").write_text("// target enumeration fixture\n")
    (root / "bin/rustc").write_text(
        '#!/usr/bin/env bash\nprintf "HARNESS_ONLY compiler version\\n"\n'
    )
    (root / "bin/cargo").write_text(
        "#!/usr/bin/env bash\n"
        'if [[ "$*" == "nextest --version" ]]; then '
        "printf 'HARNESS_ONLY nextest version\\n'; exit 0; fi\n"
        'if [[ "$1 $2" != "nextest run" ]]; then exit 99; fi\n'
        "printf 'HARNESS_ONLY controlled execution\\n'\n"
        "mkdir -p target/debug\n"
        "printf 'HARNESS_ONLY_NOT_A_PRODUCT_BINARY' > target/debug/cometix\n"
        f"exit {cargo_status}\n"
    )
    if tee_status:
        (root / "bin/tee").write_text(
            f"#!/usr/bin/env bash\ncat >/dev/null\nexit {tee_status}\n"
        )
    for command in (root / "bin").iterdir():
        command.chmod(0o755)
    env = os.environ.copy()
    env["PATH"] = str(root / "bin") + os.pathsep + env["PATH"]
    expected = (cargo_status or tee_status) if has_target else 1
    result = subprocess.run(
        ["bash", "scripts/ci-cli-tests.sh"],
        cwd=root,
        env=env,
        capture_output=True,
        text=True,
        timeout=15,
    )
    if result.returncode != expected:
        raise AssertionError(
            f"{name}: expected {expected}, got {result.returncode}: {result.stderr}"
        )
    evidence = root / "ci-evidence"
    if has_target:
        assert int((evidence / "tests.exit-code").read_text()) == expected, name
        assert (evidence / "test-targets.txt").read_text() == "--test\ncontract\n"
        assert (evidence / "cometix.sha256").is_file(), name
    else:
        assert not (root / "target/debug/cometix").exists(), name
        assert "No integration test targets found." in result.stderr, name
    return {"case": name, "expectedExit": expected, "actualExit": result.returncode}


def main():
    repo = Path(__file__).resolve().parent.parent
    # Stay below the real repository so the production git fingerprint commands
    # run normally. All fake commands/assets are confined to this temporary dir.
    target = repo / "target"
    target.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="ci-harness-check-", dir=target) as path:
        base = Path(path)
        cases = [
            run_case(base, "success", 0),
            run_case(base, "test-failure", 42),
            run_case(base, "no-targets", 0, has_target=False),
            run_case(base, "log-failure", 0, tee_status=23),
            run_case(base, "test-and-log-failure", 42, tee_status=23),
        ]
    print(json.dumps({"harnessOnly": True, "cases": cases, "cleaned": True}, indent=2))


if __name__ == "__main__":
    main()
