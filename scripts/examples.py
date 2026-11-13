"""Run every standalone example; no external services, credentials or sibling repos."""
from pathlib import Path
import json
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
build = subprocess.run(
    ['cargo', 'build', '--locked', '--example', 'native_plugin', '--message-format=json'],
    cwd=root, capture_output=True, text=True,
)
if build.returncode:
    print(build.stdout)
    print(build.stderr, file=sys.stderr)
    build.check_returncode()
libraries = []
for line in build.stdout.splitlines():
    artifact = json.loads(line)
    if artifact.get('reason') == 'compiler-artifact' and artifact['target']['name'] == 'native_plugin':
        libraries.extend(Path(name) for name in artifact['filenames']
                         if Path(name).suffix in {'.so', '.dylib', '.dll'})
if len(libraries) != 1:
    raise RuntimeError('Cargo did not report exactly one native plugin library')
for example in sorted((root / 'examples').glob('*.rs')):
    # The Wasm runner builds its independent guest and gives both hosts the
    # required artifact paths; they are not standalone no-argument programs.
    if example.stem in {'native_plugin', 'wasm_host', 'mixed_plugin_host'}:
        continue
    command = ['cargo', 'run', '--locked', '--example', example.stem]
    if example.stem == 'plugin_host':
        command += ['--', str(libraries[0])]
    subprocess.run(command, cwd=root, check=True)
print('All standalone examples executed')
