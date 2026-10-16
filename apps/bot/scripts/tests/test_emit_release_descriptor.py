from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "emit_release_descriptor.py"
SPEC = importlib.util.spec_from_file_location("emit_release_descriptor", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class ReleaseDescriptorTests(unittest.TestCase):
    def test_tree_digest_is_content_sensitive(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "asset.js").write_text("one")
            first = MODULE.tree_digest(root)
            (root / "asset.js").write_text("two")
            self.assertNotEqual(first, MODULE.tree_digest(root))

    def test_descriptor_digest_rejects_mutation(self) -> None:
        payload = {
            "schema_version": 1,
            "component": "bot",
            "commit": "a" * 40,
            "artifacts": {"daemon": "sha256:" + "b" * 64, "hosted_spa": "sha256:" + "c" * 64},
            "interfaces": {
                "hosted_bridge": "sha256:" + "d" * 64,
                "integration_ingress": "sha256:" + "e" * 64,
                "mcp_tool_request": "sha256:" + "f" * 64,
            },
        }
        payload["descriptor_digest"] = MODULE.canonical_digest(payload)
        self.assertEqual([], MODULE.validate_descriptor(payload))
        payload["artifacts"]["daemon"] = "sha256:" + "0" * 64
        self.assertIn(
            "descriptor_digest does not bind the descriptor",
            MODULE.validate_descriptor(payload),
        )

    def test_invalid_descriptor_is_rejected(self) -> None:
        self.assertTrue(MODULE.validate_descriptor({}))


if __name__ == "__main__":
    unittest.main()
