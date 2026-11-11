"""Fail closed on any additional published-crate dependency or source override."""
import json
import pathlib
import subprocess
import tomllib

root = pathlib.Path(__file__).resolve().parents[1]
manifest = tomllib.loads((root / 'Cargo.toml').read_text())
assert not (root / 'build.rs').exists(), 'build.rs is forbidden'
assert list(manifest.get('dependencies', {})) == ['zcall']
dep = manifest['dependencies']['zcall']
assert dep['version'] == '=0.1.2' and dep.get('default-features') is False
assert not any(k in dep for k in ('path', 'git', 'optional'))
assert not manifest.get('dev-dependencies') and not manifest.get('build-dependencies')
assert not manifest.get('target'), 'target-specific dependency sections require review'
m = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1'], cwd=root))
assert {p['name'] for p in m['packages']} == {'zharness', 'zcall', 'zconv'}
package = next(p for p in m['packages'] if p['name'] == 'zharness')
assert len(package['dependencies']) == 1
assert package['dependencies'][0]['name'] == 'zcall'
assert package['dependencies'][0]['req'] == '=0.1.2'
assert not package['dependencies'][0]['uses_default_features']
assert package['version'] == manifest['package']['version']
zcall = next(p for p in m['packages'] if p['name'] == 'zcall')
zconv = next(p for p in m['packages'] if p['name'] == 'zconv')
assert zcall['version'] == '0.1.2' and zconv['version'] == '0.2.2'
for package in m['packages']:
    if package['name'] != 'zharness':
        assert package['source'] == 'registry+https://github.com/rust-lang/crates.io-index'
print('Dependency audit passed: zharness -> zcall 0.1.1 -> zconv 0.2.2')
