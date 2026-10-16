# SPDX-License-Identifier: Apache-2.0
"""OSV connector package."""

from .connector import OsvConnector, parse_vuln

__all__ = ["OsvConnector", "parse_vuln"]
