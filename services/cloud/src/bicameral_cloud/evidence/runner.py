"""Command execution boundary for evidence collectors.

The runner resolves ``${ENV}`` placeholders from the process environment at execution
time only — placeholders (never resolved secrets) are what live in the committed
:class:`~bicameral_cloud.evidence.bundle.CommandSpec`. Failures are classified into a
deterministic :class:`~bicameral_cloud.evidence.outcomes.InconclusiveReason` so the
collector can emit a typed ``environment_inconclusive`` outcome instead of crashing or
falsely passing.
"""

from __future__ import annotations

import os
import re
import subprocess  # nosec B404 — fixed CLI argv, never a shell string
from dataclasses import dataclass
from typing import Protocol

from bicameral_cloud.evidence.outcomes import InconclusiveReason

_PLACEHOLDER = re.compile(r"\$\{([A-Z0-9_]+)\}")


class MissingEnvError(RuntimeError):
    """Raised when a required ``${ENV}`` placeholder is unset."""

    def __init__(self, name: str) -> None:
        super().__init__(name)
        self.name = name


@dataclass(frozen=True)
class RunOutput:
    returncode: int
    stdout: str
    stderr: str
    tool_missing: bool = False


class CommandRunner(Protocol):
    def run(self, command: list[str]) -> RunOutput: ...


def resolve(command: list[str], environ: dict[str, str]) -> list[str]:
    """Substitute ``${ENV}`` placeholders, raising if any required value is unset."""

    def _sub(token: str) -> str:
        def repl(match: re.Match[str]) -> str:
            name = match.group(1)
            if name not in environ:
                raise MissingEnvError(name)
            return environ[name]

        return _PLACEHOLDER.sub(repl, token)

    return [_sub(token) for token in command]


class SubprocessRunner:
    """Executes collector commands as real subprocesses (owner/live use)."""

    def __init__(self, environ: dict[str, str] | None = None, timeout: float = 60.0):
        self._environ = dict(environ if environ is not None else os.environ)
        self._timeout = timeout

    def run(self, command: list[str]) -> RunOutput:
        resolved = resolve(command, self._environ)
        try:
            completed = subprocess.run(  # nosec B603 — argv list, shell=False
                resolved,
                check=False,
                text=True,
                capture_output=True,
                timeout=self._timeout,
            )
        except FileNotFoundError:
            return RunOutput(returncode=127, stdout="", stderr="", tool_missing=True)
        except subprocess.TimeoutExpired:
            return RunOutput(returncode=124, stdout="", stderr="timed out")
        return RunOutput(
            returncode=completed.returncode,
            stdout=completed.stdout,
            stderr=completed.stderr,
        )


# stderr fragments that map to a deterministic inconclusive reason.
_REASON_PATTERNS: tuple[tuple[re.Pattern[str], InconclusiveReason], ...] = (
    (
        re.compile(
            r"(?i)could not find default credentials|reauthentication"
            r"|not logged in|please run.*login|invalid_grant"
        ),
        InconclusiveReason.NOT_AUTHENTICATED,
    ),
    (
        re.compile(
            r"(?i)permission.*denied|forbidden|does not have"
            r"|iam\.serviceaccounts|403"
        ),
        InconclusiveReason.PERMISSION_DENIED,
    ),
    (
        re.compile(
            r"(?i)could not resolve host|network is unreachable"
            r"|connection refused|timed out|temporary failure in name resolution"
        ),
        InconclusiveReason.NETWORK_UNAVAILABLE,
    ),
)


def classify_failure(output: RunOutput) -> InconclusiveReason:
    if output.tool_missing:
        return InconclusiveReason.TOOL_NOT_FOUND
    haystack = f"{output.stderr}\n{output.stdout}"
    for pattern, reason in _REASON_PATTERNS:
        if pattern.search(haystack):
            return reason
    return InconclusiveReason.UNKNOWN
