"""Run every standalone example; no external services, credentials or sibling repos."""
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
for example in sorted((root / 'examples').glob('*.rs')):
    subprocess.run(['cargo', 'run', '--locked', '--example', example.stem], cwd=root, check=True)
print('All standalone examples executed')
