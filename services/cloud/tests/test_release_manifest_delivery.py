from __future__ import annotations

import importlib.util
import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).parents[1]
SCRIPT = ROOT / "scripts" / "release_manifest_delivery.py"
SPEC = importlib.util.spec_from_file_location("release_manifest_delivery", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def descriptor(component: str, commit: str, spa: str) -> dict[str, object]:
    payload: dict[str, object] = {
        "schema_version": 1,
        "component": component,
        "commit": commit,
        "artifacts": {
            "hosted_spa": spa,
            f"{component}_runtime": "sha256:"
            + ("b" if component == "bot" else "c") * 64,
        },
        "interfaces": {"hosted_bridge": "sha256:" + "d" * 64},
    }
    payload["descriptor_digest"] = MODULE.canonical_digest(payload)
    return payload


class ReleaseManifestDeliveryTests(unittest.TestCase):
    def test_manifest_binds_the_complete_exact_hosted_closure(self) -> None:
        spa = "sha256:" + "a" * 64
        bot = descriptor("bot", "1" * 40, spa)
        cloud = descriptor("cloud", "2" * 40, spa)

        manifest = MODULE.build_manifest(bot, cloud)

        self.assertEqual("hosted-dashboard-v1-" + "2" * 40, manifest["release_id"])
        self.assertEqual(["hosted_dashboard_browser_v1"], manifest["required_journeys"])
        closure = manifest["closures"]["hosted_dashboard_browser_v1"]
        self.assertEqual(["bot", "cloud"], closure["components"])
        expected = MODULE.canonical_digest(
            {"components": manifest["components"], "profile_version": "v1"}
        )
        self.assertEqual(expected, closure["closure_digest"])

        mismatched = json.loads(json.dumps(bot))
        mismatched["artifacts"]["hosted_spa"] = "sha256:" + "e" * 64
        mismatched["descriptor_digest"] = MODULE.canonical_digest(
            {
                key: value
                for key, value in mismatched.items()
                if key != "descriptor_digest"
            }
        )
        with self.assertRaisesRegex(ValueError, "hosted SPA"):
            MODULE.build_manifest(mismatched, cloud)

    def test_journey_receipt_is_derived_only_from_matching_passed_raw_evidence(
        self,
    ) -> None:
        spa = "sha256:" + "a" * 64
        manifest = MODULE.build_manifest(
            descriptor("bot", "1" * 40, spa),
            descriptor("cloud", "2" * 40, spa),
        )
        raw = {
            "schema_version": 1,
            "status": "passed",
            "components": {"bot": "1" * 40, "cloud": "2" * 40},
            "dashboard": {"bundle_sha256": "a" * 64},
            "terminal_evidence_complete": True,
        }

        receipt = MODULE.bind_journey_receipt(manifest, raw)
        self.assertEqual(
            {
                "schema_version": 1,
                "release_id": manifest["release_id"],
                "journey_id": "hosted_dashboard_browser_v1",
                "closure_digest": manifest["closures"]["hosted_dashboard_browser_v1"][
                    "closure_digest"
                ],
                "profile_version": "v1",
                "outcome": "passed",
            },
            receipt,
        )

        raw["components"]["bot"] = "3" * 40
        with self.assertRaisesRegex(ValueError, "component revisions"):
            MODULE.bind_journey_receipt(manifest, raw)

    def test_served_identity_projects_descriptor_bound_release_facts(self) -> None:
        spa = "sha256:" + "a" * 64
        manifest = MODULE.build_manifest(
            descriptor("bot", "1" * 40, spa),
            descriptor("cloud", "2" * 40, spa),
        )
        identity = MODULE.build_served_identity(manifest)
        self.assertEqual(manifest["release_id"], identity["release_id"])
        self.assertEqual(
            {
                name: {
                    "commit": component["commit"],
                    "descriptor_digest": component["descriptor_digest"],
                    "artifacts": component["artifacts"],
                    "interfaces": component["interfaces"],
                }
                for name, component in manifest["components"].items()
            },
            identity["components"],
        )

    def test_public_repo_does_not_carry_deploy_or_journey_workflows(self) -> None:
        self.assertFalse((ROOT / ".github/workflows/terminal-hosted-product-journey.yml").exists())
        self.assertFalse((ROOT / ".github/workflows/deploy-cloud-run.yml").exists())
        descriptor_script = (ROOT / "scripts/emit_release_descriptor.py").read_text()
        self.assertIn("verify_immutable_dashboard", descriptor_script)
        self.assertIn("served-dashboard-provenance.json", descriptor_script)


if __name__ == "__main__":
    unittest.main()
