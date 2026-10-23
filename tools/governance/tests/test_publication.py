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

    def test_installer_pages_may_publish_the_install_site(self) -> None:
        text = """name: Deploy install site
permissions:
  contents: read
  pages: write
  id-token: write
'on':
  push:
    branches: [main]
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
    - uses: actions/deploy-pages@v4
"""
        self.assertEqual(
            check_publication.check_workflow(".github/workflows/installer-pages.yml", text),
            [],
        )

    def test_installer_pages_rejects_other_writes(self) -> None:
        text = """name: Deploy install site
permissions:
  contents: write
  pages: write
  id-token: write
'on':
  push:
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
    - uses: actions/deploy-pages@v4
"""
        self.assertNotEqual(
            check_publication.check_workflow(".github/workflows/installer-pages.yml", text),
            [],
        )

    def test_installer_pages_rejects_other_publish_actions(self) -> None:
        text = """name: Deploy install site
permissions:
  contents: read
  pages: write
  id-token: write
'on':
  push:
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
    - run: cargo publish
"""
        self.assertNotEqual(
            check_publication.check_workflow(".github/workflows/installer-pages.yml", text),
            [],
        )

    def test_component_workflow_copies_are_not_root_workflows(self) -> None:
        text = SAFE_WORKFLOW.replace("echo ok", "echo ${{ secrets.TOKEN }}")
        self.assertEqual(check_publication.check_file("apps/bot/.github/workflows/rust.yml", text), [])

    def test_security_events_write_is_allowed_for_codeql(self) -> None:
        text = SAFE_WORKFLOW.replace("contents: read", "contents: read\n  security-events: write")
        self.assertEqual(check_publication.check_workflow(".github/workflows/codeql.yml", text), [])

    def test_non_codeql_cannot_request_code_scanning_upload_privilege(self) -> None:
        text = SAFE_WORKFLOW.replace("contents: read", "contents: read\n  security-events: write")
        self.assertIn("reserved for CodeQL", " ".join(check_publication.check_workflow(self.PATH, text)))

    def test_installer_pages_cannot_be_triggered_by_pull_request(self) -> None:
        pages = """name: Deploy install site
permissions:
  contents: read
  pages: write
  id-token: write
on:
  push:
    branches: [main]
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/deploy-pages@v4
"""
        self.assertEqual(
            check_publication.check_workflow(".github/workflows/installer-pages.yml", pages), []
        )
        for edit in (
            pages.replace("  push:\n", "  pull_request:\n  push:\n"),
            pages.replace("on:\n  push:\n    branches: [main]\n", "on: [pull_request, push]\n"),
        ):
            with self.subTest(edit=edit):
                self.assertIn(
                    "Pages publisher cannot run on pull_request",
                    " ".join(check_publication.check_workflow(".github/workflows/installer-pages.yml", edit)),
                )

    def test_rejects_deploy_and_publish_actions(self) -> None:
        for snippet in (
            "uses: google-github-actions/auth@v2",
            "uses: actions/deploy-pages@v4",
            "run: cargo publish",
            "run: npm publish",
        ):
            with self.subTest(snippet=snippet):
                text = SAFE_WORKFLOW.replace("echo ok", snippet)
                self.assertNotEqual(check_publication.check_workflow(self.PATH, text), [])

    def test_removed_workflow_paths_fail_when_present(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            rel = check_publication.REMOVED_WORKFLOWS[0]
            path = root / rel
            path.parent.mkdir(parents=True)
            path.write_text("name: deploy\n")
            self.assertEqual(
                check_publication.check_removed_workflows(root),
                [f"{rel}: privileged workflow must not be in the public repository"],
            )


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
