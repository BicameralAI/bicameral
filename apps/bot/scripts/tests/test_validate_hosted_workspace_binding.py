import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).parents[1] / "validate_hosted_workspace_binding.py"
SPEC = importlib.util.spec_from_file_location("hosted_workspace_binding", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

BROWSER_WITNESS = "browser_renders_code_grounding_ready_after_reconnect"


def _secret_shaped(prefix: str, body: str) -> str:
    """Build a secret-shaped literal at runtime so the repo secret scan never
    sees a committed credential-looking string."""

    return prefix + body


class LocalFolderProductBindingRunnerAuthorityTest(unittest.TestCase):
    def test_runner_rejects_github_actions_before_topology_execution(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            trace_path = Path(temporary_directory) / "trace.json"
            completed = subprocess.run(
                [
                    sys.executable,
                    str(MODULE_PATH),
                    "--trace",
                    str(trace_path),
                ],
                env={
                    **os.environ,
                    "BICAMERAL_TOPOLOGY_EXECUTION_PROVIDER": "devin",
                    "GITHUB_ACTIONS": "true",
                },
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertEqual(completed.returncode, 2)
            self.assertIn(
                "topology production execution is restricted to direct Devin",
                completed.stderr,
            )

    def test_runner_requires_direct_devin_provider_identity(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            trace_path = Path(temporary_directory) / "trace.json"
            env = {
                key: value
                for key, value in os.environ.items()
                if key
                not in {
                    "BICAMERAL_TOPOLOGY_EXECUTION_PROVIDER",
                    "GITHUB_ACTIONS",
                }
            }
            completed = subprocess.run(
                [
                    sys.executable,
                    str(MODULE_PATH),
                    "--trace",
                    str(trace_path),
                ],
                env=env,
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertEqual(completed.returncode, 2)
            self.assertIn(
                "BICAMERAL_TOPOLOGY_EXECUTION_PROVIDER must identify direct Devin",
                completed.stderr,
            )
            # The retained trace carries the corrected journey identity and a
            # sanitized error, never the local trace directory.
            trace = json.loads(trace_path.read_text(encoding="utf-8"))
            self.assertEqual(trace["journey"], "mcp_local_folder_product_binding")
            self.assertEqual(trace["supersedes"], "hosted_workspace_binding")
            self.assertEqual(trace["status"], "error")
            self.assertFalse(trace["terminal_user_journey_acceptance"])
            self.assertNotIn(temporary_directory, json.dumps(trace))
            self.assertEqual(trace["criteria"][BROWSER_WITNESS]["gating"], False)
            self.assertTrue(
                trace["criteria"]["confirm_reaches_daemon_and_persists"]["gating"]
            )


class HostedWorkspaceBindingReadinessTest(unittest.TestCase):
    def test_pairing_client_protocol_matches_pending_pairing_contract(self) -> None:
        contract_path = (
            MODULE_PATH.parents[1] / "protocol" / "hosted-bridge" / "contract.json"
        )
        contract = json.loads(contract_path.read_text(encoding="utf-8"))

        self.assertEqual(
            MODULE.HOSTED_BRIDGE_PROTOCOL_VERSION,
            contract["pending_pairing_minimum_protocol_version"],
        )

    def test_accepts_redacted_project_list_readiness_facts(self) -> None:
        self.assertTrue(
            MODULE._project_reports_code_grounding_ready(
                {
                    "has_workspace_binding": True,
                    "event_store_healthy": True,
                    "setup_state": "ready",
                }
            )
        )

    def test_rejects_binding_when_event_store_is_unhealthy(self) -> None:
        self.assertFalse(
            MODULE._project_reports_code_grounding_ready(
                {
                    "has_workspace_binding": True,
                    "event_store_healthy": False,
                }
            )
        )

    def test_accepts_explicit_selected_project_readiness(self) -> None:
        self.assertTrue(
            MODULE._project_reports_code_grounding_ready({"code_grounding_ready": True})
        )

    def test_binding_rehydration_does_not_require_code_grounding_projection(
        self,
    ) -> None:
        # Restart/replay evidence is the daemon reporting the binding itself; an
        # unhealthy downstream grounding projection must not fail the binding.
        self.assertTrue(
            MODULE._project_reports_binding_rehydrated(
                {"has_workspace_binding": True, "event_store_healthy": False}
            )
        )
        self.assertTrue(
            MODULE._project_reports_binding_rehydrated(
                {"workspace_readiness": {"binding_state": "local_workspace_bound"}}
            )
        )
        self.assertFalse(
            MODULE._project_reports_binding_rehydrated(
                {"code_grounding_ready": True, "has_workspace_binding": False}
            )
        )


class DownstreamWitnessIsNonGatingTest(unittest.TestCase):
    def _all_required_passed(self) -> dict:
        criteria = {
            name: MODULE.Criterion(name, gating=name in MODULE.REQUIRED_CRITERIA)
            for name in MODULE.CRITERIA
        }
        for name in MODULE.REQUIRED_CRITERIA:
            criteria[name].status = "passed"
        return criteria

    def test_browser_witness_is_not_a_required_criterion(self) -> None:
        self.assertNotIn(BROWSER_WITNESS, MODULE.REQUIRED_CRITERIA)
        self.assertIn(BROWSER_WITNESS, MODULE.DOWNSTREAM_WITNESS_CRITERIA)
        self.assertIn(BROWSER_WITNESS, MODULE.CRITERIA)
        # The corrected #788 contract keeps every binding/restart/privacy
        # criterion required.
        for required in (
            "propose_without_persisting_authority",
            "confirm_reaches_daemon_and_persists",
            "ssh_https_remotes_converge",
            "binding_survives_restart_replay",
            "cancellation_leaves_no_binding_or_pending",
            "negative_paths_create_no_binding",
            "failed_rebind_preserves_previous",
            "no_local_path_in_cloud_browser_payloads_logs",
            "trace_carries_component_fingerprints",
        ):
            self.assertIn(required, MODULE.REQUIRED_CRITERIA)

    def test_failed_browser_witness_does_not_block_journey_acceptance(self) -> None:
        criteria = self._all_required_passed()
        criteria[BROWSER_WITNESS].status = "failed"
        criteria[BROWSER_WITNESS].detail = "browser did not render readiness"

        summary = MODULE.summarize_criteria(criteria)

        self.assertEqual(summary["status"], "passed")
        self.assertTrue(summary["terminal_user_journey_acceptance"])
        self.assertEqual(summary["failed_criteria"], [])
        self.assertEqual(summary["blocked_criteria"], [])
        self.assertEqual(summary["downstream_witnesses_not_passed"], [BROWSER_WITNESS])
        self.assertEqual(
            summary["downstream_witnesses"][BROWSER_WITNESS]["status"], "failed"
        )
        self.assertFalse(summary["downstream_witnesses"][BROWSER_WITNESS]["gating"])

    def test_blocked_browser_witness_does_not_block_journey_acceptance(self) -> None:
        criteria = self._all_required_passed()
        criteria[BROWSER_WITNESS].status = "blocked"

        summary = MODULE.summarize_criteria(criteria)

        self.assertEqual(summary["status"], "passed")
        self.assertTrue(summary["terminal_user_journey_acceptance"])

    def test_passed_browser_witness_cannot_rescue_failed_required_criterion(
        self,
    ) -> None:
        criteria = self._all_required_passed()
        criteria["confirm_reaches_daemon_and_persists"].status = "failed"
        criteria[BROWSER_WITNESS].status = "passed"

        summary = MODULE.summarize_criteria(criteria)

        self.assertEqual(summary["status"], "failed")
        self.assertFalse(summary["terminal_user_journey_acceptance"])
        self.assertEqual(
            summary["failed_criteria"], ["confirm_reaches_daemon_and_persists"]
        )

    def test_blocked_required_criterion_reports_blocked_not_passed(self) -> None:
        criteria = self._all_required_passed()
        criteria["binding_survives_restart_replay"].status = "blocked"

        summary = MODULE.summarize_criteria(criteria)

        self.assertEqual(summary["status"], "blocked")
        self.assertFalse(summary["terminal_user_journey_acceptance"])

    def test_untested_required_criterion_never_passes(self) -> None:
        criteria = self._all_required_passed()
        criteria["negative_paths_create_no_binding"].status = "untested"

        summary = MODULE.summarize_criteria(criteria)

        self.assertEqual(summary["status"], "failed")
        self.assertFalse(summary["terminal_user_journey_acceptance"])


class SubprocessDiagnosticsRetainedTest(unittest.TestCase):
    def test_run_failure_retains_bounded_sanitized_diagnostics(self) -> None:
        program = (
            "import sys;"
            "print('stdout says hello from /home/operator/repo');"
            "print('stderr: workspace.bind rejected: unregistered_project', file=sys.stderr);"
            "sys.exit(3)"
        )
        with self.assertRaises(MODULE.ValidationError) as caught:
            MODULE._run(
                [sys.executable, "-c", program],
                label="real MCP workspace.bind",
                timeout=60,
            )

        error = caught.exception
        self.assertEqual(str(error), "real MCP workspace.bind failed with exit code 3")
        diagnostics = error.diagnostics
        self.assertIsNotNone(diagnostics)
        assert diagnostics is not None
        self.assertEqual(diagnostics["label"], "real MCP workspace.bind")
        self.assertEqual(diagnostics["returncode"], 3)
        self.assertFalse(diagnostics["timed_out"])
        self.assertTrue(diagnostics["sanitized"])
        self.assertIn("unregistered_project", diagnostics["stderr_tail"])
        self.assertIn("stdout says hello from <local-path>", diagnostics["stdout_tail"])
        self.assertNotIn("/home/operator", diagnostics["stdout_tail"])
        # Only the executable basename is retained, never its path or arguments.
        self.assertEqual(diagnostics["executable"], Path(sys.executable).name)
        self.assertNotIn(program, json.dumps(diagnostics))
        self.assertEqual(
            diagnostics["bounds"],
            {
                "max_chars": MODULE.DIAGNOSTIC_MAX_CHARS,
                "max_lines": MODULE.DIAGNOSTIC_MAX_LINES,
            },
        )

    def test_run_timeout_retains_partial_output_and_flags_timeout(self) -> None:
        program = (
            "import sys,time;print('partial output before hang');"
            "sys.stdout.flush();time.sleep(30)"
        )
        with self.assertRaises(MODULE.ValidationError) as caught:
            MODULE._run(
                [sys.executable, "-c", program],
                label="slow command",
                timeout=2,
            )

        self.assertEqual(str(caught.exception), "slow command timed out after 2s")
        diagnostics = caught.exception.diagnostics
        assert diagnostics is not None
        self.assertTrue(diagnostics["timed_out"])
        self.assertIsNone(diagnostics["returncode"])
        self.assertIn("partial output before hang", diagnostics["stdout_tail"])

    def test_diagnostics_are_bounded_in_size_and_deterministic(self) -> None:
        noisy = "\n".join(
            f"line {index} with some payload text" for index in range(5000)
        )
        first = MODULE.build_subprocess_diagnostics(
            label="noisy",
            command=["tool"],
            returncode=1,
            stdout=noisy,
            stderr=noisy.encode("utf-8"),
        )
        second = MODULE.build_subprocess_diagnostics(
            label="noisy",
            command=["tool"],
            returncode=1,
            stdout=noisy,
            stderr=noisy.encode("utf-8"),
        )

        self.assertEqual(first, second)
        for stream in ("stdout_tail", "stderr_tail"):
            self.assertLessEqual(len(first[stream]), MODULE.DIAGNOSTIC_MAX_CHARS)
            self.assertLessEqual(
                first[stream].count("\n") + 1, MODULE.DIAGNOSTIC_MAX_LINES
            )
            self.assertTrue(first[stream].endswith("line 4999 with some payload text"))
        self.assertTrue(first["stdout_truncated"])
        self.assertTrue(first["stderr_truncated"])
        self.assertLessEqual(
            len(json.dumps(first)), 2 * MODULE.DIAGNOSTIC_MAX_CHARS + 1024
        )

    def test_empty_output_yields_empty_untruncated_tails(self) -> None:
        diagnostics = MODULE.build_subprocess_diagnostics(
            label="quiet", command=["tool"], returncode=1, stdout=None, stderr=""
        )
        self.assertEqual(diagnostics["stdout_tail"], "")
        self.assertEqual(diagnostics["stderr_tail"], "")
        self.assertFalse(diagnostics["stdout_truncated"])
        self.assertFalse(diagnostics["stderr_truncated"])


class DiagnosticSanitizationTest(unittest.TestCase):
    def test_registered_roots_are_replaced_with_placeholders(self) -> None:
        work_root = Path("/home/operator/.cache/bicameral-bot/wsbinding-runs/run-1")
        MODULE.register_local_root(work_root, "work-root")
        MODULE.register_local_root(work_root / "workspace", "workspace")

        rendered = work_root.as_posix()
        text = (
            f"daemon init failed for {rendered}/workspace/.bicameral/config.yaml "
            f"under {rendered}"
        )
        sanitized = MODULE.sanitize_diagnostic_text(text)

        self.assertNotIn(rendered, sanitized)
        self.assertNotIn("/home/operator", sanitized)
        self.assertIn("<workspace>/.bicameral/config.yaml", sanitized)
        self.assertIn("under <work-root>", sanitized)
        self.assertIn("<workspace>", MODULE.registered_root_placeholders())

    def test_generic_local_paths_are_redacted(self) -> None:
        samples = {
            "posix home": "cannot open /home/alice/src/repo/file.rs",
            "macos": "cannot open /Users/alice/src/repo/file.rs",
            "tmp": "wrote /tmp/wsbinding/workspace/x.json",
            "windows": r"cannot open C:\Users\alice\src\repo\file.rs",
            "windows forward": "cwd=D:/work/repo/dashboard",
            "unc": r"\\fileserver\share\repo\file.rs",
            "tilde": "config at ~/.bicameral/config.yaml",
        }
        for name, sample in samples.items():
            with self.subTest(name):
                sanitized = MODULE.sanitize_diagnostic_text(sample)
                self.assertIn("<local-path>", sanitized)
                for fragment in ("alice", "fileserver", "/tmp/", "D:/work", "~/"):
                    self.assertNotIn(fragment, sanitized)

    def test_urls_and_loopback_endpoints_are_preserved(self) -> None:
        text = (
            "GET http://127.0.0.1:43123/health ok; "
            "remote https://github.com/example-org/demo-project.git"
        )
        self.assertEqual(MODULE.sanitize_diagnostic_text(text), text)

    def test_secret_shaped_values_are_redacted(self) -> None:
        github = _secret_shaped("ghp_", "A" * 40)
        aws = _secret_shaped("AKIA", "ABCDEFGHIJKLMNOP")
        samples = [
            "Authorization: Bearer abc.def-ghi_jkl",
            "x-bridge-session-token: bridge-value-1234",
            '{"session_token": "sess-value-1234", "poll_proof": "pp-9"}',
            "BICAMERAL_API_KEY=key-value-1234",
            "token = " + repr(github),
            f"aws {aws} leaked",
            _secret_shaped("-----BEGIN RSA ", "PRIVATE KEY-----\nMIIabc\n-----END RSA ")
            + "PRIVATE KEY-----",
            "local_confirmation_nonce=nonce-value-1234",
        ]
        for sample in samples:
            with self.subTest(sample[:24]):
                sanitized = MODULE.sanitize_diagnostic_text(sample)
                self.assertIn("<redacted", sanitized)
                for fragment in (
                    "abc.def-ghi_jkl",
                    "bridge-value-1234",
                    "sess-value-1234",
                    "pp-9",
                    "key-value-1234",
                    github,
                    aws,
                    "MIIabc",
                    "nonce-value-1234",
                ):
                    self.assertNotIn(fragment, sanitized)

    def test_diagnostic_error_kinds_survive_sanitization(self) -> None:
        text = (
            "workspace.bind rejected: error_kind=unregistered_project "
            "bound=False workspace_binding_state=local_workspace_unbound"
        )
        self.assertEqual(MODULE.sanitize_diagnostic_text(text), text)

    def test_ansi_and_control_characters_are_stripped(self) -> None:
        text = "\x1b[31mERROR\x1b[0m\x07 daemon exited\r\n"
        self.assertEqual(MODULE.sanitize_diagnostic_text(text), "ERROR daemon exited\n")

    def test_sanitize_tree_covers_nested_trace_strings(self) -> None:
        tree = {
            "error": "checkout not found at /home/alice/src/bicameral-mcp",
            "nested": [{"detail": "wrote C:/Users/alice/trace.json"}, 3, None],
        }
        sanitized = MODULE.sanitize_tree(tree)
        self.assertEqual(sanitized["nested"][1], 3)
        self.assertIsNone(sanitized["nested"][2])
        self.assertNotIn("alice", json.dumps(sanitized))

    def test_topology_receipt_evidence_retains_file_name_not_path(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            receipt = Path(temporary_directory) / "cloud-135-receipt.json"
            receipt.write_text(
                json.dumps({"run_id": "r-1", "status": "passed"}), encoding="utf-8"
            )
            evidence = MODULE._load_topology_receipt(receipt)
        self.assertTrue(evidence["consumed"])
        self.assertEqual(evidence["file_name"], "cloud-135-receipt.json")
        self.assertNotIn("path", evidence)
        self.assertNotIn(temporary_directory, json.dumps(evidence))


if __name__ == "__main__":
    unittest.main()
