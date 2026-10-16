"""Regression tests for release tuple policy."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("release_manifest.py")
SPEC = importlib.util.spec_from_file_location("release_manifest", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def descriptor(name: str, marker: str) -> dict:
    payload = {
        "schema_version": 1,
        "component": name,
        "commit": marker * 40,
        "artifacts": {"binary": "sha256:" + marker * 64},
        "interfaces": {"api": "sha256:" + marker * 64},
    }
    payload["descriptor_digest"] = MODULE.digest(payload)
    return payload


def manifest() -> tuple[dict, dict, dict]:
    bot = descriptor("bot", "a")
    cloud = descriptor("cloud", "b")
    components = {
        name: {key: value for key, value in value.items() if key not in {"schema_version", "component"}}
        for name, value in {"bot": bot, "cloud": cloud}.items()
    }
    closure = {"components": ["bot", "cloud"], "profile_version": "v1"}
    closure["closure_digest"] = MODULE.digest(
        {"components": components, "profile_version": "v1"}
    )
    return (
        {
            "schema_version": 1,
            "release_id": "release-001",
            "components": components,
            "closures": {"hosted_dashboard_browser_v1": closure},
            "required_journeys": ["hosted_dashboard_browser_v1"],
        },
        bot,
        cloud,
    )


class ReleaseManifestTests(unittest.TestCase):
    def test_complete_tuple_passes(self) -> None:
        release, bot, cloud = manifest()
        receipt = {
            "schema_version": 1,
            "release_id": "release-001",
            "journey_id": "hosted_dashboard_browser_v1",
            "closure_digest": release["closures"]["hosted_dashboard_browser_v1"]["closure_digest"],
            "profile_version": "v1",
            "outcome": "passed",
        }
        self.assertEqual([], MODULE.validate_manifest(release))
        self.assertEqual([], MODULE.validate_descriptor("bot", release["components"]["bot"], bot))
        self.assertEqual([], MODULE.validate_descriptor("cloud", release["components"]["cloud"], cloud))
        self.assertEqual([], MODULE.validate_receipt_set(release, [receipt]))

    def test_component_change_invalidates_closure(self) -> None:
        release, _, _ = manifest()
        release["components"]["bot"]["artifacts"]["binary"] = "sha256:" + "c" * 64
        self.assertTrue(any("closure_digest" in error for error in MODULE.validate_manifest(release)))

    def test_missing_required_receipt_is_rejected(self) -> None:
        release, _, _ = manifest()
        self.assertIn(
            "receipts.hosted_dashboard_browser_v1: requires exactly one matching receipt",
            MODULE.validate_receipt_set(release, []),
        )

    def test_stale_served_identity_is_rejected(self) -> None:
        release, _, _ = manifest()
        identity = {
            "schema_version": 1,
            "release_id": "release-000",
            "components": {
                name: {
                    "commit": component["commit"],
                    "artifacts": component["artifacts"],
                }
                for name, component in release["components"].items()
            },
        }
        self.assertTrue(MODULE.validate_served_identity(release, identity))

    def test_exact_served_identity_contract_passes(self) -> None:
        release, _, _ = manifest()
        identity = {
            "schema_version": 1,
            "release_id": release["release_id"],
            "components": {
                name: {
                    "commit": component["commit"],
                    "artifacts": component["artifacts"],
                }
                for name, component in release["components"].items()
            },
        }
        self.assertEqual([], MODULE.validate_served_identity(release, identity))

    def test_ambiguous_served_identity_shape_is_rejected(self) -> None:
        release, _, _ = manifest()
        identity = {
            "release_id": release["release_id"],
            "components": {
                name: {
                    "commit": component["commit"],
                    "artifacts": component["artifacts"],
                }
                for name, component in release["components"].items()
            },
            "claimed_available": True,
        }
        errors = MODULE.validate_served_identity(release, identity)
        self.assertTrue(any("keys must be exactly" in error for error in errors))

    def test_undeclared_served_component_is_rejected(self) -> None:
        release, _, _ = manifest()
        identity = {
            "schema_version": 1,
            "release_id": release["release_id"],
            "components": {
                name: {
                    "commit": component["commit"],
                    "artifacts": component["artifacts"],
                }
                for name, component in release["components"].items()
            },
        }
        identity["components"]["unknown"] = identity["components"]["bot"]
        errors = MODULE.validate_served_identity(release, identity)
        self.assertTrue(any("component names" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
