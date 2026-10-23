"""Secret scan ignores known fixtures and still fails on a different value."""

from __future__ import annotations

import unittest

import secret_scan


class SecretScanTests(unittest.TestCase):
    def test_known_fixtures_are_not_findings(self) -> None:
        text = "\n".join(
            [
                "AKIAIOSFODNN7EXAMPLE",
                "AKIAABCDEFGHIJKLMNOP",
                "ghp_0123456789abcdefghijklmnopqrstuvwxyz",
                "-----BEGIN RSA PRIVATE KEY-----",
            ]
        )
        self.assertEqual(secret_scan.findings_in_text(text), [])

    def test_a_different_value_on_the_same_line_still_fails(self) -> None:
        other = "AKIA" + ("Z" * 16)
        text = f"AKIAIOSFODNN7EXAMPLE {other}"
        self.assertEqual(secret_scan.findings_in_text(text), [(1, "aws access key")])

    def test_private_key_material_fails(self) -> None:
        body = "A" * 40
        text = f"-----BEGIN RSA PRIVATE KEY-----\n{body}\n"
        self.assertEqual(secret_scan.findings_in_text(text), [(1, "private key")])


if __name__ == "__main__":
    unittest.main()
