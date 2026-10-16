from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "release_rehearsal.py"
SCHEMA = ROOT / "schemas" / "release-rehearsal.schema.json"
POLICY = ROOT / "registries" / "bot-release-closure-policy.v1.json"


def canonical_digest(payload: dict[str, object]) -> str:
    encoded = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def release_plan() -> dict[str, object]:
    return {
        "schema_version": 1,
        "plan_id": "bot-v0.1.12-rehearsal",
        "repository": "BicameralAI/bicameral-bot",
        "target_branch": "dev",
        "version": "0.1.12",
        "status": "frozen",
        "mode": "enforced",
        "bot_anchor": {"cargo_manifest": "Cargo.toml"},
        "release_units": [
            "bot_daemon",
            "hosted_dashboard",
            "integration_connectors",
            "mcp_surface",
            "installer_updater",
        ],
        "closure_policy_digest": canonical_digest(
            json.loads(POLICY.read_text(encoding="utf-8"))
        ),
        "created_by": "release-owner:jinhongkuan",
        "created_at": "2026-07-22T12:00:00Z",
    }


def rehearsal(plan: dict[str, object]) -> dict[str, object]:
    return {
        "schema_version": 1,
        "rehearsal_id": "v0.1.12-release-rehearsal-1",
        "release_plan_id": plan["plan_id"],
        "release_plan_digest": canonical_digest(plan),
        "version": "0.1.12",
        "component_commits": {
            "bot": "a" * 40,
            "cloud": "b" * 40,
            "mcp": "c" * 40,
            "integrations": "d" * 40,
        },
        "evaluated_under": {
            "factory_commit": "e" * 40,
            "closure_policy_digest": plan["closure_policy_digest"],
        },
        "required_closures": [
            "hosted_dashboard_browser_v1",
            "integration_bot_mcp_v1",
            "installer_updater_v1",
        ],
        "artifact_destination": {
            "kind": "github_actions_artifact",
            "name": "v0.1.12-release-rehearsal-1",
        },
        "publication": {
            "git_tag": False,
            "github_release": False,
            "production_r2_latest": False,
            "cloud_deploy": False,
        },
        "created_by": "release-owner:jinhongkuan",
        "created_at": "2026-07-22T12:00:00Z",
    }


class ReleaseRehearsalCliTest(unittest.TestCase):
    def test_installer_updater_is_an_independently_selectable_closure(self) -> None:
        policy = json.loads(POLICY.read_text(encoding="utf-8"))

        self.assertEqual(
            {
                "requires_release_units": [],
                "required_closures": ["installer_updater_v1"],
                "component_descriptors": ["bot"],
            },
            policy["release_units"]["installer_updater"],
        )
        self.assertEqual(
            {"profile_version": "v1", "components": ["bot"]},
            policy["closures"]["installer_updater_v1"],
        )

    def test_contract_schema_is_closed_and_forbids_publication(self) -> None:
        schema = json.loads(SCHEMA.read_text(encoding="utf-8"))

        self.assertFalse(schema["additionalProperties"])
        self.assertEqual(
            schema["properties"]["component_commits"]["required"],
            ["bot", "cloud", "mcp", "integrations"],
        )
        self.assertNotIn(
            "factory", schema["properties"]["component_commits"]["properties"]
        )
        self.assertIn("evaluated_under", schema["required"])
        self.assertEqual(
            schema["properties"]["evaluated_under"]["required"],
            ["factory_commit", "closure_policy_digest"],
        )
        publication = schema["properties"]["publication"]
        self.assertFalse(publication["additionalProperties"])
        for field in (
            "git_tag",
            "github_release",
            "production_r2_latest",
            "cloud_deploy",
        ):
            self.assertEqual(publication["properties"][field], {"const": False})

    def run_validate(
        self, plan: dict[str, object], contract: dict[str, object]
    ) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            plan_path = root / "release-plan.json"
            contract_path = root / "rehearsal.json"
            policy_path = root / "closure-policy.json"
            plan_path.write_text(json.dumps(plan), encoding="utf-8")
            contract_path.write_text(json.dumps(contract), encoding="utf-8")
            policy_path.write_text(POLICY.read_text(encoding="utf-8"), encoding="utf-8")
            return subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "validate",
                    str(contract_path),
                    "--release-plan",
                    str(plan_path),
                    "--closure-policy",
                    str(policy_path),
                ],
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
            )

    def test_frozen_enforced_plan_can_prepare_a_non_publishing_rehearsal(self) -> None:
        plan = release_plan()
        result = self.run_validate(plan, rehearsal(plan))

        self.assertEqual(result.returncode, 0, result.stderr)
        summary = json.loads(result.stdout)
        self.assertEqual(summary["state"], "REHEARSAL_READY")
        self.assertEqual(summary["version"], "0.1.12")
        self.assertFalse(summary["authorizes_publish"])

    def test_factory_is_policy_provenance_not_a_release_component(self) -> None:
        plan = release_plan()
        contract = rehearsal(plan)

        self.assertNotIn("factory", contract["component_commits"])
        result = self.run_validate(plan, contract)

        self.assertEqual(result.returncode, 0, result.stderr)
        contract["evaluated_under"]["closure_policy_digest"] = (  # type: ignore[index]
            "sha256:" + "f" * 64
        )
        result = self.run_validate(plan, contract)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("evaluated_under.closure_policy_digest", result.stderr)

    def test_any_production_publication_authority_is_rejected(self) -> None:
        for field in (
            "git_tag",
            "github_release",
            "production_r2_latest",
            "cloud_deploy",
        ):
            with self.subTest(field=field):
                plan = release_plan()
                contract = rehearsal(plan)
                contract["publication"][field] = True  # type: ignore[index]

                result = self.run_validate(plan, contract)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn(
                    f"publication.{field} must be false for a rehearsal",
                    result.stderr,
                )

    def test_rehearsal_is_bound_to_the_exact_frozen_plan(self) -> None:
        plan = release_plan()
        contract = rehearsal(plan)
        contract["release_plan_digest"] = "sha256:" + "f" * 64

        result = self.run_validate(plan, contract)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release_plan_digest does not match", result.stderr)

    def test_rehearsal_requires_exact_closure_projection(self) -> None:
        plan = release_plan()
        plan["closure_policy_digest"] = canonical_digest(
            json.loads(POLICY.read_text(encoding="utf-8"))
        )
        contract = rehearsal(plan)
        contract["required_closures"] = ["hosted_dashboard_browser_v1"]

        result = self.run_validate(plan, contract)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("required_closures do not match", result.stderr)

    def test_unknown_contract_fields_fail_closed(self) -> None:
        plan = release_plan()
        contract = rehearsal(plan)
        contract["publish_after_success"] = True

        result = self.run_validate(plan, contract)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown rehearsal contract fields", result.stderr)


if __name__ == "__main__":
    unittest.main()
