#!/usr/bin/env python3
"""Run every repository governance check.

    python3 tools/governance/check.py          # validate; never modifies the working tree
    python3 tools/governance/check.py --write  # regenerate CODEOWNERS and the architecture
                                               # index, then validate

Steps: structure validation, architecture validation, unittest discovery in
tools/governance/tests. Exits nonzero if any step fails.
"""

from __future__ import annotations

import sys

# Importing the validators must not leave __pycache__ directories behind.
sys.dont_write_bytecode = True

import argparse  # noqa: E402
import unittest  # noqa: E402
from pathlib import Path  # noqa: E402

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import check_architecture  # noqa: E402
import check_structure  # noqa: E402


def _report(name: str, errors: list[str]) -> bool:
    if errors:
        print(f"[FAIL] {name}", file=sys.stderr)
        for error in errors:
            print(f"       - {error}", file=sys.stderr)
        return False
    print(f"[ ok ] {name}")
    return True


def run_tests() -> bool:
    loader = unittest.TestLoader()
    suite = loader.discover(
        start_dir=str(TOOLS_DIR / "tests"), top_level_dir=str(TOOLS_DIR), pattern="test_*.py"
    )
    result = unittest.TextTestRunner(verbosity=1, stream=sys.stderr).run(suite)
    ok = result.wasSuccessful() and result.testsRun > 0
    if result.testsRun == 0:
        print("[FAIL] unit tests: no tests were discovered", file=sys.stderr)
    else:
        print(f"[{' ok ' if ok else 'FAIL'}] unit tests ({result.testsRun} run)")
    return ok


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Run all governance checks.")
    parser.add_argument(
        "--write", action="store_true", help="regenerate tracked generated outputs before validating"
    )
    args = parser.parse_args(argv)
    root = TOOLS_DIR.parents[1]

    results = []
    if args.write:
        # Regenerate first, then validate both again from the written state.
        check_structure.run(root, write=True)
        check_architecture.run(root, write=True)
    results.append(_report("structure", check_structure.run(root)))
    results.append(_report("architecture", check_architecture.run(root)))
    results.append(run_tests())

    if all(results):
        print("governance: all checks passed")
        return 0
    print("governance: FAILED", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
