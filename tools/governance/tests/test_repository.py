"""End-to-end checks against an isolated copy of this repository's governed files."""

from __future__ import annotations

import shutil

import check_architecture
import check_structure
from tests.fixtures import REPO_ROOT, TempRepoTestCase


class RepositoryCopyTests(TempRepoTestCase):
    def setUp(self) -> None:
        super().setUp()
        for rel in check_structure.candidate_files(REPO_ROOT):
            source = REPO_ROOT / rel
            if not source.is_file() or source.is_symlink():
                continue
            target = self.root / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)

    def test_copy_passes_without_modification(self) -> None:
        before = self.snapshot()
        self.assertEqual(check_structure.run(self.root), [])
        self.assertEqual(check_architecture.run(self.root), [])
        self.assertEqual(before, self.snapshot())

    def test_write_mode_is_idempotent(self) -> None:
        before = self.snapshot()
        self.assertEqual(check_structure.run(self.root, write=True), [])
        self.assertEqual(check_architecture.run(self.root, write=True), [])
        self.assertEqual(before, self.snapshot())

    def test_write_mode_repairs_stale_outputs(self) -> None:
        expected = self.snapshot()
        for rel in (".github/CODEOWNERS", "docs/architecture/INDEX.md", "docs/architecture/index.json"):
            self.write(rel, "stale\n")
        self.assertNotEqual(check_structure.run(self.root), [])
        self.assertNotEqual(check_architecture.run(self.root), [])
        self.assertEqual(check_structure.run(self.root, write=True), [])
        self.assertEqual(check_architecture.run(self.root, write=True), [])
        self.assertEqual(expected, self.snapshot())

    def test_every_reserved_directory_contains_only_its_readme(self) -> None:
        manifest = check_structure.load_manifest(self.root)
        files = check_structure.candidate_files(self.root)
        for entry in manifest.directories.values():
            if entry.status != "reserved":
                continue
            with self.subTest(directory=entry.path):
                inside = [f for f in files if f.startswith(entry.path + "/")]
                self.assertEqual(inside, [f"{entry.path}/README.md"])
                readme = self.read(f"{entry.path}/README.md")
                self.assertIn("bicameral-factory/issues/969", readme)
