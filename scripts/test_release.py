import unittest
import json
import os
import tempfile
from pathlib import Path
from unittest.mock import patch
from release import next_version, parse_version, precedence, release_request


class VersionTests(unittest.TestCase):
    def test_bumps(self):
        self.assertEqual(next_version('1.2.3'), '1.2.4')
        self.assertEqual(next_version('1.2.3', 'minor'), '1.3.0')
        self.assertEqual(next_version('1.2.3', 'major'), '2.0.0')

    def test_manual(self):
        self.assertEqual(next_version('0.1.0', manual='0.1.0'), '0.1.0')
        self.assertEqual(next_version('0.1.0', manual='0.2.0-rc.1+build.2'), '0.2.0-rc.1+build.2')
        with self.assertRaises(ValueError):
            next_version('1.0.0', manual='0.9.9')

    def test_precedence(self):
        versions = ['1.0.0-alpha', '1.0.0-alpha.1', '1.0.0-alpha.beta', '1.0.0-beta', '1.0.0-beta.2', '1.0.0-beta.11', '1.0.0-rc.1', '1.0.0']
        self.assertEqual(sorted(versions, key=precedence), versions)

    def test_reject(self):
        for version in ['', '1.0', '01.0.0', '1.0.0-01', '1.0.0;', '1.0.0\n', '1.0.0+bad space', '18446744073709551616.0.0']:
            with self.subTest(version=version), self.assertRaises(ValueError):
                parse_version(version)
        with self.assertRaises(ValueError):
            next_version('1.0.0', 'arbitrary')


class ReleaseRequestTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.root_patch = patch('release.ROOT', self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)
        (self.root / '.github').mkdir()
        (self.root / 'Cargo.toml').write_text('''[package]
name = "zharness"
version = "0.1.1"
[dependencies]
zcall = { version = "=0.1.2", default-features = false }
''')
        self.valid = {'crate': 'zharness', 'version': '0.1.1', 'zcall': '=0.1.2', 'publish': True}
        self.trusted = {'GITHUB_REF': 'refs/heads/main', 'GITHUB_REPOSITORY': 'LIghtJUNction/zharness'}

    def write(self, payload):
        (self.root / '.github/release-request.json').write_text(json.dumps(payload))

    def test_valid(self):
        self.write(self.valid)
        with patch.dict(os.environ, self.trusted):
            self.assertEqual(release_request(), '0.1.1')

    def test_fail_closed_mismatches(self):
        for data in [
            {**self.valid, 'version': '0.1.2'},
            {**self.valid, 'zcall': '=0.1.1'},
            {**self.valid, 'crate': 'zcall'},
            {**self.valid, 'publish': False},
            {**self.valid, 'publish': 1},
            {**self.valid, 'extra': True},
            {'version': '0.1.1'},
            ['zharness'],
        ]:
            with self.subTest(data=data), patch.dict(os.environ, self.trusted):
                self.write(data)
                with self.assertRaises(ValueError):
                    release_request()

    def test_wrong_origin(self):
        self.write(self.valid)
        for changes in [
            {'GITHUB_REF': 'refs/heads/not-main'},
            {'GITHUB_REPOSITORY': 'someone/zharness'},
        ]:
            with self.subTest(changes=changes), patch.dict(os.environ, {**self.trusted, **changes}):
                with self.assertRaises(ValueError):
                    release_request()


if __name__ == '__main__':
    unittest.main()
