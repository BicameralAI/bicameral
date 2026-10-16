import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/rust.yml"
QUICKSTART_VALIDATOR = ROOT / "scripts/validate_quickstart_golden_path.py"


class RustWorkflowPolicyTests(unittest.TestCase):
    def test_generic_quickstart_journey_is_not_a_required_ci_job(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")

        self.assertNotIn("quickstart-golden-path", workflow)
        self.assertNotIn("validate_quickstart_golden_path.py", workflow)
        self.assertFalse(
            QUICKSTART_VALIDATOR.exists(),
            "the obsolete local-daemon journey validator must not be release evidence",
        )


if __name__ == "__main__":
    unittest.main()
