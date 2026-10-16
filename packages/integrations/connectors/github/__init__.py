# SPDX-License-Identifier: Apache-2.0
"""GitHub connector package."""

from .connector import GitHubConnector, parse_pull_request

__all__ = ["GitHubConnector", "parse_pull_request"]
