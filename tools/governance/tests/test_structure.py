"""Tests for tools/governance/check_structure.py."""

from __future__ import annotations

import copy
import json
import os
import unittest

import check_structure
from tests.fixtures import BASE_MANIFEST, OWNERS, TempRepoTestCase


def manifest_errors(manifest: dict) -> list[str]:
    try:
        check_structure.parse_manifest(json.dumps(manifest))
    except check_structure.ManifestError as exc:
        return exc.errors
    return []


def with_directory(manifest: dict, entry: dict, index: int | None = None) -> dict:
    result = copy.deepcopy(manifest)
    if index is None:
        result["directories"].append(entry)
    else:
        result["directories"].insert(index, entry)
    return result


class ManifestSchemaTests(unittest.TestCase):
    def assertManifestError(self, manifest: dict, fragment: str) -> None:
        errors = manifest_errors(manifest)
        self.assertTrue(
            any(fragment in error for error in errors),
            f"expected an error containing {fragment!r}, got {errors}",
        )

    def test_base_manifest_is_valid(self) -> None:
        self.assertEqual(manifest_errors(BASE_MANIFEST), [])

    def test_real_manifest_is_valid(self) -> None:
        from tests.fixtures import REPO_ROOT

        text = (REPO_ROOT / check_structure.MANIFEST_PATH).read_text(encoding="utf-8")
        manifest = check_structure.parse_manifest(text)
        self.assertEqual(list(manifest.default_owners), OWNERS)
        self.assertEqual(manifest.architecture_records, "docs/architecture/records")

    def test_unknown_top_level_key(self) -> None:
        manifest = dict(copy.deepcopy(BASE_MANIFEST), extra=True)
        self.assertManifestError(manifest, "unknown keys ['extra']")

    def test_missing_top_level_key(self) -> None:
        manifest = copy.deepcopy(BASE_MANIFEST)
        del manifest["directories"]
        self.assertManifestError(manifest, "missing keys ['directories']")

    def test_duplicate_json_key_rejected(self) -> None:
        text = '{"version": 1, "version": 1}'
        with self.assertRaises(check_structure.ManifestError) as ctx:
            check_structure.parse_manifest(text)
        self.assertIn("duplicate JSON key", ctx.exception.errors[0])

    def test_invalid_owner_handle(self) -> None:
        manifest = copy.deepcopy(BASE_MANIFEST)
        manifest["default_owners"] = ["@jinhongkuan", "kevin"]
        self.assertManifestError(manifest, "invalid owner handle 'kevin'")

    def test_invalid_owner_in_directory(self) -> None:
        manifest = copy.deepcopy(BASE_MANIFEST)
        manifest["directories"][1]["owners"] = ["@bad--name"]
        self.assertManifestError(manifest, "invalid owner handle '@bad--name'")

    def test_team_owner_handle_allowed(self) -> None:
        manifest = copy.deepcopy(BASE_MANIFEST)
        manifest["directories"][1]["owners"] = ["@BicameralAI/maintainers"]
        self.assertEqual(manifest_errors(manifest), [])

    def test_empty_default_owners(self) -> None:
        manifest = copy.deepcopy(BASE_MANIFEST)
        manifest["default_owners"] = []
        self.assertManifestError(manifest, "owners must not be empty")

    def test_non_normalized_paths(self) -> None:
        for bad in ("docs/../x", "/abs", "docs/", "docs//x", "./docs", "a\\b", ".git"):
            with self.subTest(path=bad):
                manifest = with_directory(
                    BASE_MANIFEST, {"path": bad, "purpose": "p", "children": "open"}
                )
                self.assertManifestError(manifest, "normalized relative path")

    def test_duplicate_directory(self) -> None:
        manifest = with_directory(
            BASE_MANIFEST, {"path": "docs", "purpose": "p", "children": "declared"}
        )
        self.assertManifestError(manifest, "duplicate directory")

    def test_duplicate_root_file(self) -> None:
        manifest = copy.deepcopy(BASE_MANIFEST)
        manifest["root_files"].append({"path": "README.md", "purpose": "again"})
        self.assertManifestError(manifest, "duplicate root file")

    def test_parent_must_be_declared_before_child(self) -> None:
        manifest = with_directory(
            BASE_MANIFEST, {"path": "apps/dashboard", "purpose": "p", "children": "open"}
        )
        manifest = with_directory(
            manifest, {"path": "apps", "purpose": "p", "children": "declared"}
        )
        self.assertManifestError(manifest, "parent 'apps' must be declared before its children")

    def test_children_only_under_declared_parent(self) -> None:
        manifest = with_directory(
            BASE_MANIFEST, {"path": "tools/governance/sub", "purpose": "p", "children": "open"}
        )
        self.assertManifestError(manifest, "only 'declared' parents may declare children")

    def test_invalid_child_policy(self) -> None:
        manifest = with_directory(BASE_MANIFEST, {"path": "x", "purpose": "p", "children": "any"})
        self.assertManifestError(manifest, "children must be one of")

    def test_invalid_status(self) -> None:
        manifest = with_directory(
            BASE_MANIFEST, {"path": "x", "purpose": "p", "children": "open", "status": "frozen"}
        )
        self.assertManifestError(manifest, "status must be one of")

    def test_unknown_directory_key(self) -> None:
        manifest = with_directory(
            BASE_MANIFEST, {"path": "x", "purpose": "p", "children": "open", "color": "red"}
        )
        self.assertManifestError(manifest, "unknown keys ['color']")

    def test_missing_purpose(self) -> None:
        manifest = with_directory(BASE_MANIFEST, {"path": "x", "purpose": " ", "children": "open"})
        self.assertManifestError(manifest, "purpose must be a non-empty string")

    def test_readme_must_be_boolean(self) -> None:
        manifest = with_directory(
            BASE_MANIFEST, {"path": "x", "purpose": "p", "children": "open", "readme": "no"}
        )
        self.assertManifestError(manifest, "readme must be a boolean")

    def test_source_only_for_reserved(self) -> None:
        manifest = with_directory(
            BASE_MANIFEST, {"path": "x", "purpose": "p", "children": "open", "source": "A/b"}
        )
        self.assertManifestError(manifest, "source is only meaningful for reserved directories")

    def test_forbidden_name_cannot_be_declared(self) -> None:
        manifest = with_directory(
            BASE_MANIFEST, {"path": "docs/adr", "purpose": "p", "children": "open"}
        )
        self.assertManifestError(manifest, "forbidden decision-directory name 'adr'")

    def test_root_file_must_be_in_root(self) -> None:
        manifest = copy.deepcopy(BASE_MANIFEST)
        manifest["root_files"].append({"path": "docs/x.md", "purpose": "p"})
        self.assertManifestError(manifest, "root files must be directly in the repository root")

    def test_root_file_required_must_be_boolean(self) -> None:
        manifest = copy.deepcopy(BASE_MANIFEST)
        manifest["root_files"].append({"path": "NOTICE", "purpose": "p", "required": "yes"})
        self.assertManifestError(manifest, "required must be a boolean")

    def test_root_file_unknown_key(self) -> None:
        manifest = copy.deepcopy(BASE_MANIFEST)
        manifest["root_files"].append({"path": "NOTICE", "purpose": "p", "status": "active"})
        self.assertManifestError(manifest, "unknown keys ['status']")

    def test_canonical_records_must_be_declared(self) -> None:
        manifest = copy.deepcopy(BASE_MANIFEST)
        manifest["canonical"]["architecture_records"] = "docs/records"
        self.assertManifestError(manifest, "must be a declared directory")

    def test_canonical_unknown_key(self) -> None:
        manifest = copy.deepcopy(BASE_MANIFEST)
        manifest["canonical"]["other"] = "x"
        self.assertManifestError(manifest, "canonical: unknown keys ['other']")


class CodeownersRenderTests(unittest.TestCase):
    def test_render_is_deterministic_and_ordered(self) -> None:
        manifest = check_structure.parse_manifest(json.dumps(BASE_MANIFEST))
        first = check_structure.render_codeowners(manifest)
        self.assertEqual(first, check_structure.render_codeowners(manifest))
        lines = [line for line in first.splitlines() if line and not line.startswith("#")]
        self.assertEqual(lines[0], "* @jinhongkuan @Knapp-Kevin")
        self.assertIn("/LICENSE @jinhongkuan @Knapp-Kevin", lines)
        self.assertIn("/docs/architecture/ @jinhongkuan @Knapp-Kevin", lines)

    def test_parent_lines_precede_children(self) -> None:
        manifest_data = copy.deepcopy(BASE_MANIFEST)
        manifest_data["directories"][1]["owners"] = ["@docs-owner"]
        manifest = check_structure.parse_manifest(json.dumps(manifest_data))
        text = check_structure.render_codeowners(manifest)
        self.assertLess(text.index("/docs/ "), text.index("/docs/architecture/ "))


class RepositoryLayoutTests(TempRepoTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.manifest = self.build_structure_fixture()
        self.assertEqual(check_structure.run(self.root, write=True), [])

    def assertLayoutError(self, fragment: str) -> None:
        errors = check_structure.run(self.root)
        self.assertTrue(
            any(fragment in error for error in errors),
            f"expected an error containing {fragment!r}, got {errors}",
        )

    def test_valid_fixture_passes(self) -> None:
        self.assertEqual(check_structure.run(self.root), [])

    def test_stray_root_file(self) -> None:
        self.write("notes.txt")
        self.assertLayoutError("notes.txt: undeclared root file")

    def test_undeclared_top_level_directory(self) -> None:
        self.write("scratch/file.py")
        self.assertLayoutError("scratch/: undeclared top-level directory")

    def test_undeclared_nested_directory(self) -> None:
        self.write("docs/guides/intro.md")
        self.assertLayoutError("docs/guides/: undeclared directory under docs/")

    def test_file_directly_in_declared_directory_allowed(self) -> None:
        self.write("docs/overview.md")
        self.assertEqual(check_structure.run(self.root), [])

    def test_open_directory_allows_nested_content(self) -> None:
        self.write("tools/governance/lib/helper.py")
        self.write(".github/workflows/ci.yml")
        self.assertEqual(check_structure.run(self.root), [])

    def test_forbidden_adr_directory(self) -> None:
        self.write("tools/governance/adr/0001.md")
        self.assertLayoutError("forbidden decision-directory name 'adr'")

    def test_forbidden_names_are_case_insensitive(self) -> None:
        self.write("tools/governance/Design-Docs/x.md")
        self.assertLayoutError("forbidden decision-directory name 'Design-Docs'")

    def test_empty_forbidden_directory_detected(self) -> None:
        (self.root / "docs" / "decisions").mkdir()
        self.assertLayoutError("docs/decisions/: forbidden decision-directory name")

    def test_empty_undeclared_directory_detected(self) -> None:
        (self.root / "docs" / "empty").mkdir()
        self.assertLayoutError("docs/empty/: undeclared directory under docs/")

    def test_ignored_directories_are_not_governed(self) -> None:
        self.write("build/output.bin")
        (self.root / "tools" / "governance" / "__pycache__").mkdir()
        self.assertEqual(check_structure.run(self.root), [])

    def test_missing_readme(self) -> None:
        os.remove(self.root / "docs" / "README.md")
        self.assertLayoutError("docs/README.md: required directory README is missing")

    def test_readme_not_required_when_disabled(self) -> None:
        self.assertFalse((self.root / ".github" / "README.md").exists())
        self.assertEqual(check_structure.run(self.root), [])

    def test_missing_root_file(self) -> None:
        os.remove(self.root / "LICENSE")
        self.assertLayoutError("LICENSE: required root file is missing")

    def test_reserved_directory_rejects_code(self) -> None:
        self.write("packages/mcp/server.py")
        self.assertLayoutError("packages/mcp/ is reserved")

    def test_reserved_directory_rejects_nested_code(self) -> None:
        self.write("packages/mcp/src/server.py")
        self.assertLayoutError("packages/mcp/ is reserved")

    def test_reserved_readme_must_name_source(self) -> None:
        self.write("packages/mcp/README.md", "Reserved.\n")
        self.assertLayoutError("must name its intended source repository BicameralAI/bicameral-mcp")

    def test_invalid_owner_fails_whole_check(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["default_owners"] = ["@ok", "not-a-handle"]
        self.write_manifest(manifest)
        self.assertLayoutError("invalid owner handle 'not-a-handle'")

    def test_stale_codeowners(self) -> None:
        self.write(".github/CODEOWNERS", "* @someone-else\n")
        self.assertLayoutError(".github/CODEOWNERS: stale")

    def test_missing_codeowners(self) -> None:
        os.remove(self.root / ".github" / "CODEOWNERS")
        self.assertLayoutError(".github/CODEOWNERS: missing")

    def test_codeowners_stale_after_manifest_change_and_write_repairs(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["directories"][4]["owners"] = ["@jinhongkuan"]
        self.write_manifest(manifest)
        self.assertLayoutError(".github/CODEOWNERS: stale")
        self.assertEqual(check_structure.run(self.root, write=True), [])
        self.assertIn("/governance/ @jinhongkuan\n", self.read(".github/CODEOWNERS"))
        self.assertEqual(check_structure.run(self.root), [])

    def test_normal_mode_does_not_modify_files(self) -> None:
        self.write(".github/CODEOWNERS", "stale\n")
        before = self.snapshot()
        check_structure.run(self.root)
        self.assertEqual(before, self.snapshot())

    def test_noncanonical_architecture_file_location(self) -> None:
        self.write("docs/ARCH-0003-misplaced.md")
        self.assertLayoutError("architecture records must live directly in docs/architecture/records/")

    def test_architecture_file_nested_in_records_subdirectory(self) -> None:
        self.write("docs/architecture/records/old/ARCH-0003-nested.md")
        self.assertLayoutError("architecture records must live directly in docs/architecture/records/")

    def test_architecture_file_in_canonical_location_allowed(self) -> None:
        self.write("docs/architecture/records/ARCH-0003-ok.md")
        self.assertEqual(check_structure.run(self.root), [])

    @unittest.skipUnless(hasattr(os, "symlink"), "symlinks unsupported")
    def test_symlinked_file_rejected(self) -> None:
        try:
            os.symlink("/etc/hostname", self.root / "docs" / "escape.md")
        except OSError:
            self.skipTest("cannot create symlinks")
        self.assertLayoutError("docs/escape.md: symbolic links are not allowed")

    @unittest.skipUnless(hasattr(os, "symlink"), "symlinks unsupported")
    def test_symlinked_directory_rejected(self) -> None:
        try:
            os.symlink("/tmp", self.root / "tools" / "governance" / "outside")
        except OSError:
            self.skipTest("cannot create symlinks")
        self.assertLayoutError("tools/governance/outside: symbolic links are not allowed")

    def _with_legacy(self, legacy: list) -> None:
        manifest = copy.deepcopy(self.manifest)
        for entry in manifest["directories"]:
            if entry["path"] == "tools/governance":
                entry["legacy_decision_dirs"] = legacy
        self.write_manifest(manifest)

    def test_declared_legacy_decision_dir_allowed(self) -> None:
        self._with_legacy(["docs/adr"])
        self.write("tools/governance/docs/adr/0001-old.md")
        self.assertEqual(check_structure.run(self.root), [])

    def test_undeclared_decision_dir_still_rejected_beside_legacy(self) -> None:
        self._with_legacy(["docs/adr"])
        self.write("tools/governance/other/adr/0001-old.md")
        self.assertLayoutError("forbidden decision-directory name 'adr'")

    def test_legacy_decision_dirs_require_open_directory(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["directories"][1]["legacy_decision_dirs"] = ["adr"]
        self.write_manifest(manifest)
        self.assertLayoutError("legacy_decision_dirs requires an open directory")

    def test_tracked_files_are_checked(self) -> None:
        self.write("stray.cfg")
        import subprocess

        subprocess.run(["git", "-C", str(self.root), "add", "stray.cfg"], check=True)
        self.assertLayoutError("stray.cfg: undeclared root file")


if __name__ == "__main__":
    unittest.main()
