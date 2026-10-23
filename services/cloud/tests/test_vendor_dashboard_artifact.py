from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).parents[1]


class VendorDashboardTests(unittest.TestCase):
    def test_public_repo_does_not_carry_vendor_workflow(self) -> None:
        self.assertFalse((ROOT / ".github" / "workflows" / "vendor-dashboard-bundle.yml").exists())

    def test_prebuilt_dashboard_is_vendored_byte_for_byte(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            self._assert_prebuilt_dashboard_is_vendored_byte_for_byte(Path(raw_tmp))

    def _assert_prebuilt_dashboard_is_vendored_byte_for_byte(
        self, tmp_path: Path
    ) -> None:
        cloud = tmp_path / "cloud"
        bot = tmp_path / "bot"
        script = cloud / "scripts" / "vendor_dashboard.sh"
        script.parent.mkdir(parents=True)
        shutil.copy2(ROOT / "scripts" / "vendor_dashboard.sh", script)

        dashboard = bot / "dashboard"
        dist = dashboard / "dist"
        (dist / "assets").mkdir(parents=True)
        (dist / "index.html").write_text("<main>real SPA</main>\n", encoding="utf-8")
        javascript = b"const whitespace = ` \t\n`;\n"
        (dist / "assets" / "app.js").write_bytes(javascript)

        contract_dir = bot / "protocol" / "hosted-bridge"
        contract_dir.mkdir(parents=True)
        contract = b'{"version":1}\n'
        (contract_dir / "contract.json").write_bytes(contract)
        contract_digest = hashlib.sha256(contract).hexdigest()
        (contract_dir / "contract.sha256").write_text(
            f"{contract_digest}  contract.json\n", encoding="utf-8"
        )

        descriptor_dir = bot / "release-dashboard"
        descriptor_dir.mkdir()
        descriptor = {
            "schema_version": 1,
            "component": "bot",
            "commit": "a" * 40,
            "artifacts": {"hosted_spa": "sha256:" + "b" * 64},
            "interfaces": {"hosted_bridge": "sha256:" + "c" * 64},
        }
        descriptor_digest = (
            "sha256:"
            + hashlib.sha256(
                json.dumps(descriptor, sort_keys=True, separators=(",", ":")).encode()
            ).hexdigest()
        )
        descriptor["descriptor_digest"] = descriptor_digest
        (descriptor_dir / "bot-release-descriptor.json").write_text(
            json.dumps(descriptor), encoding="utf-8"
        )

        env = os.environ.copy()
        env.update(
            {
                "DASHBOARD_ALREADY_BUILT": "1",
                "DASHBOARD_BOT_SHA": "a" * 40,
                "HOSTED_BRIDGE_CONTRACT_SHA256": contract_digest,
                "BOT_DESCRIPTOR_DIGEST": descriptor_digest,
            }
        )
        subprocess.run([str(script), str(dashboard)], check=True, env=env)

        destination = cloud / "src" / "bicameral_cloud" / "static" / "dashboard"
        self.assertEqual(javascript, (destination / "assets" / "app.js").read_bytes())
        version = json.loads((destination / "version.json").read_text(encoding="utf-8"))
        self.assertEqual("a" * 40, version["bot_commit"])
        self.assertEqual(descriptor_digest, version["bot_descriptor_digest"])
        copied_descriptor = cloud / "release-inputs" / "bot-release-descriptor.json"
        self.assertEqual(descriptor, json.loads(copied_descriptor.read_text()))


if __name__ == "__main__":
    unittest.main()
