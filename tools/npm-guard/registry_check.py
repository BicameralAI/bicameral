#!/usr/bin/env python3
"""Registry vetting for npm packages, used by check_dependency_changes.py.

check_package() asks registry.npmjs.org and api.npmjs.org whether a package exists, how old the package
and the requested version are, and how many weekly downloads it has. These are heuristics, not a malware
scan of the package.
"""
import functools
import json
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone

MIN_PACKAGE_AGE_DAYS = 7
MIN_VERSION_AGE_DAYS = 7
MIN_WEEKLY_DOWNLOADS = 1000
TIMEOUT = 8


@functools.lru_cache(maxsize=None)
def fetch_json(url):
    req = urllib.request.Request(url, headers={'Accept': 'application/json'})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
        return json.load(resp)


def age_days(iso):
    when = datetime.fromisoformat(iso.replace('Z', '+00:00'))
    return (datetime.now(timezone.utc) - when).days


def newest_settled_version(meta):
    """Newest stable version published at least MIN_VERSION_AGE_DAYS ago, or None."""
    times = meta.get('time', {})
    settled = [(times[v], v) for v in meta.get('versions', {})
               if '-' not in v and v in times and age_days(times[v]) >= MIN_VERSION_AGE_DAYS]
    return max(settled)[1] if settled else None


def check_package(name, version='', check_latest=True, check_downloads=True):
    """Return (decision, reason) or None when the package looks fine.

    `version` is the version, range or tag being added ('' for none). Only a request without one is
    judged by the age of the latest release. `check_downloads` is False for transitive lockfile entries.
    A reason that starts with "Could not verify" means the lookup failed, not that the package is bad.
    """
    quoted = urllib.parse.quote(name, safe='@')
    try:
        meta = fetch_json(f'https://registry.npmjs.org/{quoted}')
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            return 'deny', f'"{name}" does not exist on the npm registry (hallucinated or mistyped name).'
        return 'ask', f'Could not verify "{name}" (registry HTTP {exc.code}).'
    except (OSError, ValueError):
        return 'ask', f'Could not verify "{name}" (registry unreachable).'

    problems = []
    too_new = False  # a too-new version is blocked outright; the other problems only ask
    times = meta.get('time', {})
    if times.get('created') and age_days(times['created']) < MIN_PACKAGE_AGE_DAYS:
        problems.append(f'package created {age_days(times["created"])}d ago')
    latest = meta.get('dist-tags', {}).get('latest')
    requested = meta.get('dist-tags', {}).get(version, version)
    if requested and times.get(requested) and age_days(times[requested]) < MIN_VERSION_AGE_DAYS:
        problem = f'requested version {requested} published {age_days(times[requested])}d ago'
        settled = newest_settled_version(meta)
        if settled:
            problem += f' (newest version older than {MIN_VERSION_AGE_DAYS}d: {name}@{settled})'
        problems.append(problem)
        too_new = True
    if check_latest and not version and latest and times.get(latest) and age_days(times[latest]) < MIN_VERSION_AGE_DAYS:
        problem = f'latest version {latest} published {age_days(times[latest])}d ago'
        settled = newest_settled_version(meta)
        if settled:
            problem += f' (newest version older than {MIN_VERSION_AGE_DAYS}d: {name}@{settled})'
        problems.append(problem)
    downloads_unknown = False
    if check_downloads:
        try:
            weekly = fetch_json(f'https://api.npmjs.org/downloads/point/last-week/{quoted}').get('downloads', 0)
            if weekly < MIN_WEEKLY_DOWNLOADS:
                problems.append(f'only {weekly} downloads last week')
        except (OSError, ValueError):
            downloads_unknown = True
    if too_new:
        return 'deny', f'"{name}": ' + '; '.join(problems) + \
            f'. Versions under {MIN_VERSION_AGE_DAYS} days old are blocked; install an older version instead.'
    if problems:
        return 'ask', f'"{name}": ' + '; '.join(problems) + '. Confirm this is the package you intend.'
    if downloads_unknown:
        return 'ask', f'Could not verify "{name}" download count (download stats unavailable).'
    return None
