import hashlib
import io
import json
import tarfile
import unittest
from pathlib import Path

from scripts.verify_production_cloud_contract import (
    canonical_digest,
    content_tree_digest,
    verify,
)

ROOT = Path(__file__).resolve().parents[2]
EXPECTED = (ROOT / "protocol/hosted-bridge/contract.sha256").read_text().split()[0]
EXPECTED_COMMIT = "a" * 40


class _Response(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *_args):
        self.close()


def _immutable_artifact() -> tuple[bytes, dict[str, str]]:
    files = {
        "index.html": b"<html>new</html>",
        "assets/app.js": b"console.log('new')",
    }
    hosted_spa = content_tree_digest(files.items())
    descriptor = {
        "schema_version": 1,
        "component": "bot",
        "commit": EXPECTED_COMMIT,
        "artifacts": {
            "daemon": "sha256:" + "d" * 64,
            "hosted_spa": hosted_spa,
        },
        "interfaces": {
            "hosted_bridge": "sha256:" + "e" * 64,
            "integration_ingress": "sha256:" + "f" * 64,
            "mcp_tool_request": "sha256:" + "1" * 64,
        },
    }
    descriptor["descriptor_digest"] = canonical_digest(descriptor)
    descriptor_bytes = (json.dumps(descriptor, sort_keys=True) + "\n").encode()

    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
        entries = {
            **{f"dashboard/dist/{name}": body for name, body in files.items()},
            "release-dashboard/bot-release-descriptor.json": descriptor_bytes,
        }
        for name, body in entries.items():
            info = tarfile.TarInfo(name)
            info.size = len(body)
            archive.addfile(info, io.BytesIO(body))
    artifact = buffer.getvalue()
    return artifact, {
        "artifact_sha256": "sha256:" + hashlib.sha256(artifact).hexdigest(),
        "descriptor_digest": descriptor["descriptor_digest"],
        "hosted_spa": hosted_spa,
    }


def _valid_payloads() -> tuple[bytes, dict, dict, dict[str, str]]:
    artifact, proof = _immutable_artifact()
    version = {
        "bot_commit": EXPECTED_COMMIT,
        "hosted_bridge_contract_sha256": EXPECTED,
        "artifact_sha256": proof["artifact_sha256"].removeprefix("sha256:"),
        "bot_descriptor_digest": proof["descriptor_digest"],
        "bundle_sha256": proof["hosted_spa"].removeprefix("sha256:"),
    }
    identity = {
        "schema_version": 1,
        "release_id": "hosted-dashboard-v1-" + "c" * 40,
        "components": {
            "bot": {
                "commit": EXPECTED_COMMIT,
                "descriptor_digest": proof["descriptor_digest"],
                "artifacts": {"hosted_spa": proof["hosted_spa"]},
                "interfaces": {},
            },
            "cloud": {
                "commit": "c" * 40,
                "descriptor_digest": "sha256:" + "2" * 64,
                "artifacts": {
                    "hosted_spa": proof["hosted_spa"],
                    "immutable_bot_dashboard": proof["artifact_sha256"],
                    "bot_release_descriptor": proof["descriptor_digest"],
                },
                "interfaces": {},
            },
        },
    }
    return artifact, version, identity, proof


def _opener(version: dict, identity: dict):
    def open_url(url: str, *, timeout: int):
        assert f"release_contract={EXPECTED}" in url
        assert f"release_bot_commit={EXPECTED_COMMIT}" in url
        assert "release_dashboard=sha256%3A" in url
        assert timeout == 10
        if "release-identity.json" in url:
            return _Response(json.dumps(identity).encode())
        if "version.json" in url:
            return _Response(json.dumps(version).encode())
        raise AssertionError(f"unexpected URL {url}")

    return open_url


class ProductionCloudContractTest(unittest.TestCase):
    def test_accepts_byte_bound_cloud_identity(self) -> None:
        artifact, version, identity, _proof = _valid_payloads()
        errors = verify(
            ROOT,
            expected_bot_commit=EXPECTED_COMMIT,
            immutable_bot_artifact_bytes=artifact,
            opener=_opener(version, identity),
        )
        self.assertEqual(errors, [])

    def test_fails_closed_when_production_cloud_bridge_is_stale(self) -> None:
        artifact, version, identity, _proof = _valid_payloads()
        stale = "c2f132aaae1dc1be1616c7e7b3a9c60593700a4498579e3ef444f16ea5b3082c"
        version["hosted_bridge_contract_sha256"] = stale

        errors = verify(
            ROOT,
            expected_bot_commit=EXPECTED_COMMIT,
            immutable_bot_artifact_bytes=artifact,
            opener=_opener(version, identity),
        )

        self.assertTrue(any(EXPECTED in error and stale in error for error in errors))

    def test_rejects_matching_bridge_from_stale_bot_commit(self) -> None:
        artifact, version, identity, _proof = _valid_payloads()
        version["bot_commit"] = "b" * 40

        errors = verify(
            ROOT,
            expected_bot_commit=EXPECTED_COMMIT,
            immutable_bot_artifact_bytes=artifact,
            opener=_opener(version, identity),
        )

        self.assertTrue(any(EXPECTED_COMMIT in error and "b" * 40 in error for error in errors))

    def test_rejects_v0126_metadata_only_relabel(self) -> None:
        artifact, version, identity, _proof = _valid_payloads()
        stale_spa = "sha256:" + "9" * 64
        relabeled_descriptor = "sha256:" + "8" * 64
        version["bundle_sha256"] = stale_spa.removeprefix("sha256:")
        version["bot_descriptor_digest"] = relabeled_descriptor
        identity["components"]["bot"]["descriptor_digest"] = relabeled_descriptor
        identity["components"]["bot"]["artifacts"]["hosted_spa"] = stale_spa
        identity["components"]["cloud"]["artifacts"]["hosted_spa"] = stale_spa
        identity["components"]["cloud"]["artifacts"][
            "bot_release_descriptor"
        ] = relabeled_descriptor

        errors = verify(
            ROOT,
            expected_bot_commit=EXPECTED_COMMIT,
            immutable_bot_artifact_bytes=artifact,
            opener=_opener(version, identity),
        )

        self.assertTrue(any("bundle digest" in error for error in errors))
        self.assertTrue(any("descriptor" in error for error in errors))
        self.assertTrue(any("hosted_spa" in error for error in errors))

    def test_rejects_cloud_identity_without_immutable_artifact_proof(self) -> None:
        artifact, version, identity, _proof = _valid_payloads()
        del identity["components"]["cloud"]["artifacts"]["immutable_bot_dashboard"]

        errors = verify(
            ROOT,
            expected_bot_commit=EXPECTED_COMMIT,
            immutable_bot_artifact_bytes=artifact,
            opener=_opener(version, identity),
        )

        self.assertTrue(any("immutable Bot dashboard artifact proof" in error for error in errors))

    def test_rejects_invalid_immutable_artifact_before_cloud_metadata(self) -> None:
        _artifact, version, identity, _proof = _valid_payloads()
        errors = verify(
            ROOT,
            expected_bot_commit=EXPECTED_COMMIT,
            immutable_bot_artifact_bytes=b"not-a-tarball",
            opener=_opener(version, identity),
        )
        self.assertEqual(len(errors), 1)
        self.assertIn("immutable Bot dashboard artifact", errors[0])


class ReleaseWorkflowContractGateTest(unittest.TestCase):
    def test_release_entrypoints_use_shared_exact_producer_gate(self) -> None:
        for name in ("release.yml", "tag-release-on-main-version.yml"):
            workflow = (ROOT / ".github" / "workflows" / name).read_text()
            self.assertIn(
                "uses: ./.github/actions/verify-production-cloud-contract",
                workflow,
            )

    def test_shared_gate_reconstructs_exact_producer_artifact(self) -> None:
        action = (
            ROOT / ".github/actions/verify-production-cloud-contract/action.yml"
        ).read_text()
        self.assertIn("cargo build --release --locked --bin bicameral", action)
        self.assertIn("npm ci --silent && npm run build", action)
        self.assertIn("RELEASE_SOURCE_COMMIT=\"${bot_sha}\"", action)
        self.assertIn("--verify-hosted-bundle", action)
        self.assertIn("tar --sort=name --mtime=@0", action)
        self.assertIn(
            "--immutable-bot-artifact release-dashboard/bicameral-dashboard.tar.gz",
            action,
        )

    def test_shared_gate_pins_producer_reconstruction_toolchain(self) -> None:
        action = (
            ROOT / ".github/actions/verify-production-cloud-contract/action.yml"
        ).read_text()
        self.assertIn("node-version: 22.23.2", action)
        self.assertIn("toolchain: 1.97.1", action)
        self.assertNotIn("node-version: 22\n", action)
        self.assertNotIn("toolchain: stable", action)

    def test_release_build_waits_for_production_cloud_conformance(self) -> None:
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn("production-cloud-conformance:", workflow)
        self.assertIn("build:\n    needs: production-cloud-conformance", workflow)

    def test_version_tag_is_not_created_before_production_conformance(self) -> None:
        workflow = (ROOT / ".github/workflows/tag-release-on-main-version.yml").read_text()
        verify_step = workflow.find("Verify production Cloud contract")
        tag_step = workflow.find("Create release tag")
        self.assertGreaterEqual(verify_step, 0)
        self.assertGreater(tag_step, verify_step)

    def test_verifier_requires_local_artifact_and_served_identity(self) -> None:
        script = (ROOT / "scripts/verify_production_cloud_contract.py").read_text()
        self.assertIn("--immutable-bot-artifact", script)
        self.assertNotIn("BICAMERAL_DOWNLOAD_BASE_URL", script)
        self.assertIn("release-identity.json", script)
        self.assertIn("immutable_bot_dashboard", script)
        self.assertIn("bot_release_descriptor", script)


if __name__ == "__main__":
    unittest.main()
