"""Publication preflight: content, attestation, workflow and history rules."""

from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path

import check_publication
from tests.fixtures import REPO_ROOT

SAFE_WORKFLOW = """name: ci
permissions:
  contents: read
'on':
  pull_request:
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
    - run: echo ok
"""


class ContentRuleTests(unittest.TestCase):
    def test_flags_each_kind_of_private_material(self) -> None:
        # Built by concatenation so this file does not match the rules itself.
        cases = {
            "contact: someone.private@" + "gmail.com": "personal email address",
            "see https://example.slack.com/archives/" + "D0AAAAAAAA/p1": "private Slack direct-message link",
            "https://www.notion.so/Page-" + "0" * 32: "Notion page link",
            "https://app.devin.ai/sessions/" + "a" * 32: "agent session URL",
            "https://claude.ai/code/" + "session_01AbCdEf": "agent session URL",
        }
        for line, rule in cases.items():
            with self.subTest(line=line):
                self.assertEqual(check_publication.check_content("doc.md", line), [f"doc.md:1: {rule}"])

    def test_allows_role_addresses_and_public_channels(self) -> None:
        text = "security@bicameral-ai.com\nhttps://acme.slack.com/archives/C01EXAMPLE/p1\n"
        self.assertEqual(check_publication.check_content("doc.md", text), [])

    def test_forbidden_paths(self) -> None:
        for path in ("apps/bot/.gstack/log.jsonl", "service/.env", "keys/server.pem", "a/b.key"):
            with self.subTest(path=path):
                self.assertIsNotNone(check_publication.FORBIDDEN_PATHS.search(path))
        for path in ("apps/bot/.env.example", "docs/keys.md", "src/monkey.py"):
            with self.subTest(path=path):
                self.assertIsNone(check_publication.FORBIDDEN_PATHS.search(path))


class AttestationTests(unittest.TestCase):
    PATH = "services/cloud/.bicameral/factory-attestations/x.json"

    def test_synthetic_placeholder_passes(self) -> None:
        text = '{"fixture_kind": "synthetic-test-placeholder", "version": 1}'
        self.assertEqual(check_publication.check_file(self.PATH, text), [])

    def test_real_session_record_fails(self) -> None:
        text = '{"version": 1, "agent": "codex", "session": "abc", "roadmap": "internal"}'
        self.assertEqual(len(check_publication.check_file(self.PATH, text)), 1)


class WorkflowRuleTests(unittest.TestCase):
    PATH = ".github/workflows/ci.yml"

    def test_safe_workflow_passes(self) -> None:
        self.assertEqual(check_publication.check_workflow(self.PATH, SAFE_WORKFLOW), [])

    def test_rejects_privileged_triggers_secrets_and_writes(self) -> None:
        unsafe = {
            "pull_request_target": SAFE_WORKFLOW.replace("  pull_request:", "  pull_request_target:"),
            "workflow_run": SAFE_WORKFLOW.replace("  pull_request:", "  workflow_run:"),
            "secrets": SAFE_WORKFLOW.replace("echo ok", "echo ${{ secrets.TOKEN }}"),
            "write": SAFE_WORKFLOW.replace("contents: read", "contents: write"),
            "id-token": SAFE_WORKFLOW.replace("contents: read", "contents: read\n  id-token: write"),
            "no permissions": SAFE_WORKFLOW.replace("permissions:\n  contents: read\n", ""),
        }
        for name, text in unsafe.items():
            with self.subTest(name=name):
                self.assertNotEqual(check_publication.check_workflow(self.PATH, text), [])

    def test_component_workflow_copies_are_not_root_workflows(self) -> None:
        text = SAFE_WORKFLOW.replace("echo ok", "echo ${{ secrets.TOKEN }}")
        self.assertEqual(check_publication.check_file("apps/bot/.github/workflows/release.yml", text), [])


class HistoryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

        def git(*args: str) -> None:
            subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True)

        self.git = git
        git("init", "-q")
        git("config", "user.email", "test@example.com")
        git("config", "user.name", "test")
        (self.root / "a.txt").write_text("a\n")
        git("add", "a.txt")
        git("commit", "-q", "-m", "first")

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def test_single_root_commit_passes(self) -> None:
        self.assertEqual(check_publication.check_history(self.root, "HEAD"), [])

    def test_earlier_history_fails(self) -> None:
        (self.root / "a.txt").write_text("b\n")
        self.git("commit", "-q", "-am", "second")
        self.assertEqual(len(check_publication.check_history(self.root, "HEAD")), 1)


class RepositoryTests(unittest.TestCase):
    def test_repository_passes(self) -> None:
        self.assertEqual(check_publication.run(REPO_ROOT), [])


if __name__ == "__main__":
    unittest.main()
