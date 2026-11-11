"""Fail-closed registry preflight and bounded SemVer version selection."""
import json
import os
import pathlib
import re
import subprocess
import sys
import tomllib
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
SEMVER = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?\Z', re.ASCII)


def parse_version(text):
    match = SEMVER.fullmatch(text)
    if not match or len(text) > 128:
        raise ValueError('Expected legal SemVer')
    core = tuple(int(match[i]) for i in range(1, 4))
    if any(n >= 2**64 for n in core):
        raise ValueError('Cargo version component exceeds u64')
    pre = match[4]
    if pre and any(s.isdigit() and len(s) > 1 and s[0] == '0' for s in pre.split('.')):
        raise ValueError('Numeric prerelease identifier has a leading zero')
    return core, pre


def precedence(text):
    core, pre = parse_version(text)
    parts = tuple((0, int(s)) if s.isdigit() else (1, s) for s in pre.split('.')) if pre else ()
    return core, pre is None, parts


def next_version(current, bump='patch', manual=''):
    core, _ = parse_version(current)
    if manual:
        parse_version(manual)
        if precedence(manual) < precedence(current):
            raise ValueError('Version must not decrease')
        return manual
    if bump not in ('patch', 'minor', 'major'):
        raise ValueError('Unknown bump mode')
    values = list(core)
    position = {'major': 0, 'minor': 1, 'patch': 2}[bump]
    values[position] += 1
    for i in range(position + 1, 3):
        values[i] = 0
    result = '.'.join(map(str, values))
    parse_version(result)
    return result


def manifest():
    return tomllib.loads((ROOT / 'Cargo.toml').read_text())


def get_json(path, allow_missing=False):
    req = urllib.request.Request('https://crates.io/api/v1/' + path, headers={'User-Agent': 'zharness-release-preflight/0.1 (https://github.com/LIghtJUNction/zharness)'})
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        if allow_missing and error.code == 404:
            return None
        raise


def preflight(version):
    data = manifest()
    package = data['package']
    repository = os.environ['GITHUB_REPOSITORY']
    if package['repository'].rstrip('/') != 'https://github.com/' + repository:
        raise ValueError('Untrusted package repository')
    name = package['name']
    if get_json('crates/' + name, allow_missing=True):
        owners = get_json('crates/' + name + '/owners')
        logins = {u.get('login', '').casefold() for u in owners.get('users', [])}
        if repository.split('/')[0].casefold() not in logins:
            raise ValueError('Existing package is not owned by this repository owner')
    if get_json('crates/' + name + '/' + version, allow_missing=True):
        raise ValueError('Version already exists on crates.io')
    for name, dep in data.get('dependencies', {}).items():
        if not isinstance(dep, dict) or 'git' in dep or 'path' in dep or not dep.get('version', '').startswith('='):
            raise ValueError('Dependency is not an exact registry version')
        release = get_json('crates/' + name + '/' + dep['version'][1:])
        if release['version'].get('yanked'):
            raise ValueError('Pinned dependency is yanked')


def release_request():
    """Allow an explicit exact-version release only from trusted main."""
    if os.environ.get('GITHUB_REF') != 'refs/heads/main':
        raise ValueError('Release requests must originate on main')
    if os.environ.get('GITHUB_REPOSITORY') != 'LIghtJUNction/zharness':
        raise ValueError('Untrusted release repository')
    data = json.loads((ROOT / '.github/release-request.json').read_text())
    if not isinstance(data, dict) or set(data) != {'crate', 'version', 'zcall', 'publish'}:
        raise ValueError('Release request has unexpected fields')
    package = manifest()
    expected = {
        'crate': package['package']['name'],
        'version': package['package']['version'],
        'zcall': package['dependencies']['zcall']['version'],
        'publish': True,
    }
    if data.get('publish') is not True or data != expected:
        raise ValueError('Release request does not match the exact crate, version and dependency')
    parse_version(data['version'])
    return data['version']


def apply_version(version):
    path = ROOT / 'Cargo.toml'
    current = manifest()['package']['version']
    next_version(current, manual=version)
    text = path.read_text()
    needle = 'version = "' + current + '"'
    if needle not in text:
        raise ValueError('Package version declaration not found')
    path.write_text(text.replace(needle, 'version = "' + version + '"', 1))
    subprocess.run(['cargo', 'generate-lockfile'], cwd=ROOT, check=True)
    if manifest()['package']['version'] != version:
        raise ValueError('Version update failed')


def main():
    command = sys.argv[1]
    if command == 'plan':
        version = (release_request() if os.environ.get('GITHUB_EVENT_NAME') == 'push'
                   else next_version(manifest()['package']['version'], os.environ.get('BUMP', 'patch'), os.environ.get('MANUAL_VERSION', '')))
        if os.environ.get('PUBLISH') == 'true':
            preflight(version)
        print('Planned version:', version)
        if os.environ.get('GITHUB_OUTPUT'):
            with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
                output.write('version=' + version + '\n')
    elif command == 'apply':
        apply_version(os.environ['VERSION'])
    elif command == 'preflight':
        if os.environ.get('GITHUB_EVENT_NAME') == 'push':
            release_request()
        preflight(manifest()['package']['version'])
    elif command == 'request':
        print('Validated trusted release request:', release_request())
    else:
        raise ValueError('Unknown release command')


if __name__ == '__main__':
    main()
