"""Fail closed on extra dependencies, source overrides or sibling-repository coupling."""
import json
import pathlib
import subprocess
import tomllib

root = pathlib.Path(__file__).resolve().parents[1]
manifest = tomllib.loads((root / 'Cargo.toml').read_text())
assert not (root / 'build.rs').exists(), 'build.rs is forbidden'
assert not (root / 'integration').exists(), 'MCP integration belongs to its own repository'
pins = {'zcall': '=0.2.0', 'zplugin': '=0.1.0'}
assert set(manifest.get('dependencies', {})) == set(pins)
for name, version in pins.items():
    dep = manifest['dependencies'][name]
    assert dep['version'] == version and dep.get('default-features') is False
    assert not any(k in dep for k in ('path', 'git', 'optional'))
assert manifest['dependencies']['zplugin']['features'] == ['macros']
assert manifest['features']['std'] == ['zplugin/std']
assert not manifest.get('dev-dependencies') and not manifest.get('build-dependencies')
assert not manifest.get('target'), 'target-specific dependency sections require review'
for workflow in (root / '.github/workflows').glob('*.yml'):
    content = workflow.read_text()
    assert 'integration/mcp' not in content, workflow.name
    assert 'repository: LIghtJUNction/zmcp' not in content, workflow.name
m = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1'], cwd=root))
runtime_names = {'zharness', 'zcall', 'zconv', 'zplugin'}
macro_names = {'zplugin-macros', 'syn', 'quote', 'proc-macro2', 'unicode-ident'}
assert {p['name'] for p in m['packages']} == runtime_names | macro_names
package = next(p for p in m['packages'] if p['name'] == 'zharness')
assert len(package['dependencies']) == len(pins)
for dep in package['dependencies']:
    assert dep['req'] == pins[dep['name']]
    assert not dep['uses_default_features']
assert package['version'] == manifest['package']['version']
zcall = next(p for p in m['packages'] if p['name'] == 'zcall')
zconv = next(p for p in m['packages'] if p['name'] == 'zconv')
assert zcall['version'] == '0.2.0' and zconv['version'] == '0.3.0'
for name in ('zplugin', 'zplugin-macros'):
    assert next(p for p in m['packages'] if p['name'] == name)['version'] == '0.1.0'
for package in m['packages']:
    if package['name'] != 'zharness':
        assert package['source'] == 'registry+https://github.com/rust-lang/crates.io-index'

# A normal dependency on a proc-macro package executes on the compiler host,
# not in the application's runtime. Check that boundary instead of hiding the
# macro's compilation dependencies in the runtime claim.
packages = {p['id']: p for p in m['packages']}
nodes = {n['id']: n for n in m['resolve']['nodes']}
seen = set()
pending = [m['resolve']['root']]
while pending:
    identifier = pending.pop()
    if identifier in seen:
        continue
    seen.add(identifier)
    for dep in nodes[identifier]['deps']:
        target = packages[dep['pkg']]
        if any('proc-macro' in t['kind'] for t in target['targets']):
            continue
        if any(kind['kind'] is None for kind in dep['dep_kinds']):
            pending.append(dep['pkg'])
assert {packages[identifier]['name'] for identifier in seen} == runtime_names
print(f"Dependency audit passed: zharness {manifest['package']['version']}; "
      "runtime zcall 0.2.0 / zconv 0.3.0 / zplugin 0.1.0; "
      "zplugin-macros and its parser dependencies are compilation-only")
