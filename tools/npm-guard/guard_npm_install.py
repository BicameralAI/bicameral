#!/usr/bin/env python3
"""Claude Code PreToolUse hook: vet `npm/pnpm/yarn/bun install|add` commands.

Reads the hook JSON on stdin. Emits a permission decision on stdout:
  deny  - package does not exist on the registry (likely hallucinated/typosquat bait)
  deny  - requested version is newer than the minimum release age
  deny  - any node/npm/npx/next/vite/... command while the working tree contains a malware signature
  ask   - package is new, little-used, non-registry, or lifecycle scripts are not disabled
  (no output) - nothing suspicious, defer to normal permission rules
"""
import functools
import json
import os
import re
import shlex
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone

MIN_PACKAGE_AGE_DAYS = 7
MIN_VERSION_AGE_DAYS = 7
MIN_WEEKLY_DOWNLOADS = 1000
TIMEOUT = 8

# The package manager must sit at command position (start, or after ; & | ( or a newline),
# optionally behind env assignments or sudo, so quoted text mentioning it is not a match.
INSTALL_RE = re.compile(
    r'(?:^|[;&|(\n])\s*(?:\w+=\S+\s+)*(?:sudo\s+)?(npm|pnpm|yarn|bun)\s+(install|i|add)\b([^\n]*)')
# Anything that executes project code. Refused outright while the working tree carries a malware signature.
RUN_RE = re.compile(
    r'(?:^|[;&|(\n])\s*(?:\w+=\S+\s+)*(?:sudo\s+)?(?:\S*/)?'
    r'(?:node|nodejs|npm|npx|pnpm|pnpx|yarn|bun|bunx|deno|tsx|ts-node|next|vite|nuxt|nodemon|turbo|nx|corepack)\b')
NAME_RE = re.compile(r'^(@[\w.-]+/)?[\w.-]+$')
REDIRECT_RE = re.compile(r'^\d*[<>&]')


def decide(decision, reason, tool_input=None):
    output = {
        'hookEventName': 'PreToolUse',
        'permissionDecision': decision,
        'permissionDecisionReason': reason,
    }
    if tool_input is not None:
        output['updatedInput'] = tool_input
    print(json.dumps({'hookSpecificOutput': output}))
    sys.exit(0)


@functools.lru_cache(maxsize=None)
def fetch_json(url):
    req = urllib.request.Request(url, headers={'Accept': 'application/json'})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
        return json.load(resp)


def age_days(iso):
    when = datetime.fromisoformat(iso.replace('Z', '+00:00'))
    return (datetime.now(timezone.utc) - when).days


def package_name(spec):
    """Return the bare registry name, or None if the spec is not a registry package."""
    if spec.startswith(('.', '/', '~')) or ':' in spec:
        return None
    at = spec.rfind('@')
    name = spec[:at] if at > 0 else spec
    return name if NAME_RE.match(name) else None


def spec_version(spec, name):
    """The version/tag part of a spec ('' when none was given)."""
    return spec[len(name) + 1:] if spec.startswith(name + '@') else ''


def newest_settled_version(meta):
    """Newest stable version published at least MIN_VERSION_AGE_DAYS ago, or None."""
    times = meta.get('time', {})
    settled = [(times[v], v) for v in meta.get('versions', {})
               if '-' not in v and v in times and age_days(times[v]) >= MIN_VERSION_AGE_DAYS]
    return max(settled)[1] if settled else None


def check_package(name, version='', check_latest=True):
    """Return (decision, reason) or None when the package looks fine.

    `version` is the version/tag the user asked for; `check_latest` is False when the caller
    is already swapping an unversioned install for an older, settled version.
    """
    quoted = urllib.parse.quote(name, safe='@')
    try:
        meta = fetch_json(f'https://registry.npmjs.org/{quoted}')
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            return 'deny', f'"{name}" does not exist on the npm registry (hallucinated or mistyped name).'
        return 'ask', f'Could not verify "{name}" (registry HTTP {exc.code}).'
    except (urllib.error.URLError, TimeoutError, ValueError):
        return 'ask', f'Could not verify "{name}" (registry unreachable).'

    problems = []
    too_new = False  # a too-new version is blocked outright; the other problems only ask
    times = meta.get('time', {})
    if times.get('created') and age_days(times['created']) < MIN_PACKAGE_AGE_DAYS:
        problems.append(f'package created {age_days(times["created"])}d ago')
    latest = meta.get('dist-tags', {}).get('latest')
    requested = meta.get('dist-tags', {}).get(version, version)
    if requested and times.get(requested) \
            and age_days(times[requested]) < MIN_VERSION_AGE_DAYS:
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
    try:
        weekly = fetch_json(f'https://api.npmjs.org/downloads/point/last-week/{quoted}').get('downloads', 0)
        if weekly < MIN_WEEKLY_DOWNLOADS:
            problems.append(f'only {weekly} downloads last week')
    except (urllib.error.URLError, TimeoutError, ValueError):
        problems.append('download stats unavailable')
    if too_new:
        return 'deny', f'"{name}": ' + '; '.join(problems) + \
            f'. Versions under {MIN_VERSION_AGE_DAYS} days old are blocked; install an older version instead.'
    if problems:
        return 'ask', f'"{name}": ' + '; '.join(problems) + '. Confirm this is the package you intend.'
    return None


def settled_pin(name, spec):
    """'name@older-version' when an unversioned install would pull a too-new latest, else None."""
    if spec_version(spec, name) not in ('', 'latest'):
        return None
    try:
        meta = fetch_json(f'https://registry.npmjs.org/{urllib.parse.quote(name, safe="@")}')
    except (urllib.error.URLError, TimeoutError, ValueError):
        return None
    latest = meta.get('dist-tags', {}).get('latest')
    times = meta.get('time', {})
    if latest and times.get(latest) and age_days(times[latest]) < MIN_VERSION_AGE_DAYS:
        settled = newest_settled_version(meta)
        return f'{name}@{settled}' if settled else None
    return None


def project_dir(cwd):
    """The npm project npm would use from cwd: the nearest directory with a package.json, else cwd."""
    current = os.path.abspath(cwd)
    while True:
        if os.path.isfile(os.path.join(current, 'package.json')):
            return current
        parent = os.path.dirname(current)
        if parent == current:
            return os.path.abspath(cwd)
        current = parent


def scripts_disabled(args, cwd):
    if '--ignore-scripts' in args:
        return True
    try:
        with open(os.path.join(project_dir(cwd), '.npmrc')) as handle:
            return any(re.match(r'\s*ignore-scripts\s*=\s*true', line) for line in handle)
    except OSError:
        return False


def infected_tree(event):
    """Output of the worktree scan when it finds malware, else None (also None if the guard is not installed)."""
    root = os.environ.get('CLAUDE_PROJECT_DIR') or event.get('cwd') or os.getcwd()
    scanner = os.path.join(root, 'tools', 'npm-guard', 'scan_incoming.py')
    if not os.path.isfile(scanner):
        return None
    try:
        result = subprocess.run([sys.executable, scanner, '--worktree'], cwd=root, capture_output=True, encoding='utf-8', errors='replace', timeout=60)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return result.stderr.strip() or 'malware signature found' if result.returncode == 1 else None


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return
    if event.get('tool_name') != 'Bash':
        return
    command = event.get('tool_input', {}).get('command', '')

    if RUN_RE.search(command):
        found = infected_tree(event)
        if found:
            decide('deny', 'npm-guard: refusing to run node/npm/next/etc. because this repository contains malware.\n'
                           + found + '\nDo not run the project. Tell security; undo the update (git reset --hard ORIG_HEAD).')

    for match in INSTALL_RE.finditer(command):
        tail = re.split(r'&&|\|\||;|\|', match.group(3))[0]
        try:
            args = shlex.split(tail)
        except ValueError:
            decide('ask', 'Could not parse the install command; please review it.')
        specs = [a for a in args if not a.startswith('-') and not REDIRECT_RE.match(a)]

        if not scripts_disabled(args, event.get('cwd') or os.getcwd()):
            decide('ask', 'Dependency lifecycle scripts are enabled. Add --ignore-scripts or set '
                          'ignore-scripts=true in .npmrc so a malicious postinstall cannot run.')
        swaps = {}
        for spec in specs:
            name = package_name(spec)
            if name is None:
                decide('ask', f'"{spec}" is a path/URL/git dependency, not a vetted registry package.')
            pin = settled_pin(name, spec)
            if pin:
                swaps[spec] = pin
            verdict = check_package(name, spec_version(spec, name), check_latest=not pin)
            if verdict:
                decide(*verdict)

        if swaps:
            new_tail = tail
            for spec, pin in swaps.items():
                new_tail = re.sub(r'(?<![\w@/.:-])' + re.escape(spec) + r'(?![\w@/.:-])', pin, new_tail, count=1)
            start = match.start(3)
            tool_input = dict(event['tool_input'])
            tool_input['command'] = command[:start] + new_tail + command[start + len(tail):]
            decide('ask', 'Latest version is under %d days old, so the install was switched to the newest '
                          'older version: %s' % (MIN_VERSION_AGE_DAYS, ', '.join(swaps.values())), tool_input)


if __name__ == '__main__':
    main()
