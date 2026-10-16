import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
VALIDATOR = ROOT / "scripts/validate_figma_references.py"


class ValidateFigmaReferencesTest(unittest.TestCase):
    def run_validator(self, root: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(VALIDATOR), "--root", str(root)],
            check=False,
            capture_output=True,
            text=True,
        )

    def test_rejects_checked_in_spec_raster(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            asset = root / "docs/specs/assets/operator-home/screen.png"
            asset.parent.mkdir(parents=True)
            asset.write_bytes(b"not-a-real-png")

            result = self.run_validator(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("screen.png", result.stderr)
            self.assertIn("Link to the live Figma node", result.stderr)

    def test_rejects_figma_url_without_node_id(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            spec = root / "docs/specs/journey.md"
            spec.parent.mkdir(parents=True)
            spec.write_text(
                "Design: https://www.figma.com/design/file-key/Product-Journey\n",
                encoding="utf-8",
            )

            result = self.run_validator(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("node-specific", result.stderr)
            self.assertIn("journey.md:1", result.stderr)

    def test_rejects_unlinked_figma_reference(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            spec = root / "docs/specs/journey.md"
            spec.parent.mkdir(parents=True)
            spec.write_text("Canonical Figma: Product journey.\n", encoding="utf-8")

            result = self.run_validator(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Figma references must be Markdown links", result.stderr)
            self.assertIn("journey.md:1", result.stderr)

    def test_accepts_node_specific_figma_markdown_link(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            spec = root / "docs/specs/journey.md"
            spec.parent.mkdir(parents=True)
            spec.write_text(
                "Canonical [Figma Product journey](https://www.figma.com/design/file-key/Product-Journey?node-id=12-34).\n",
                encoding="utf-8",
            )

            result = self.run_validator(root)

            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
