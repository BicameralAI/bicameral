#!/usr/bin/env python3
"""Regression checks for factory signal-context change classification."""

from __future__ import annotations

import importlib.util
import io
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

SCRIPT_PATH = Path(__file__).with_name("validate_factory_signal_context.py")
SPEC = importlib.util.spec_from_file_location("factory_signal_context", SCRIPT_PATH)
assert SPEC and SPEC.loader
validator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(validator)


class FactorySignalContextTests(unittest.TestCase):
    def run_validator(self) -> int:
        with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            return validator.main()

    def test_ancestry_only_change_is_exempt(self) -> None:
        with (
            patch.object(validator, "changed_files", return_value=[]),
            patch.object(validator, "changed_attestation_paths") as attestations,
        ):
            self.assertEqual(self.run_validator(), 0)
            attestations.assert_not_called()

    def test_product_change_still_requires_attestation(self) -> None:
        with (
            patch.object(validator, "changed_files", return_value=["crates/core/src/lib.rs"]),
            patch.object(validator, "changed_attestation_paths", return_value=[]),
        ):
            self.assertEqual(self.run_validator(), 1)

    def test_diff_failure_is_not_exempt(self) -> None:
        with (
            patch.object(
                validator,
                "changed_files",
                side_effect=RuntimeError("Unable to determine changed files."),
            ),
            patch.object(validator, "changed_attestation_paths") as attestations,
        ):
            self.assertEqual(self.run_validator(), 1)
            attestations.assert_not_called()


if __name__ == "__main__":
    unittest.main()
