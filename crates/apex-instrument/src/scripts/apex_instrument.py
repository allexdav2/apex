#!/usr/bin/env python3
"""
APEX instrumentation helper script.

Usage:
    python apex_instrument.py <test_command> [args...]

Runs the provided test command under coverage.py in branch mode, then emits
a JSON file `.apex_coverage.json` in the current working directory.

The JSON file has this structure:
{
    "files": {
        "<rel_path>": {
            "executed_branches": [[from_line, to_line], ...],
            "missing_branches": [[from_line, to_line], ...],
            "all_branches": [[from_line, to_line], ...]
        }
    }
}
"""
import sys
import os
import json
import subprocess
import tempfile

DATA_FILE = ".apex_coverage"
JSON_OUT = ".apex_coverage.json"


def main():
    if len(sys.argv) < 2:
        print("Usage: apex_instrument.py <test_command> [args...]", file=sys.stderr)
        sys.exit(1)

    cmd = sys.argv[1:]

    # Run under coverage.py
    run_result = subprocess.run(
        [sys.executable, "-m", "coverage", "run", "--branch",
         f"--data-file={DATA_FILE}"] + cmd,
        capture_output=False,
    )

    # Export to JSON
    subprocess.run(
        [sys.executable, "-m", "coverage", "json",
         f"--data-file={DATA_FILE}", "-o", JSON_OUT],
        check=False,
    )

    # Read and reshape the JSON for APEX consumption
    try:
        with open(JSON_OUT) as f:
            raw = json.load(f)
    except (FileNotFoundError, json.JSONDecodeError) as e:
        print(f"APEX: could not read coverage JSON: {e}", file=sys.stderr)
        sys.exit(run_result.returncode)

    apex_data = {"files": {}}
    for filepath, fdata in raw.get("files", {}).items():
        executed = fdata.get("executed_branches", [])
        missing = fdata.get("missing_branches", [])
        all_branches = executed + missing

        apex_data["files"][filepath] = {
            "executed_branches": executed,
            "missing_branches": missing,
            "all_branches": all_branches,
        }

    with open(JSON_OUT, "w") as f:
        json.dump(apex_data, f, indent=2)

    sys.exit(run_result.returncode)


if __name__ == "__main__":
    main()
