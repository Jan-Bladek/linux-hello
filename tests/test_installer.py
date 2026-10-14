import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

REPO = Path(__file__).parents[1]


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), REPO / 'packaging/installer' / (name + '.py'))
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


installer = load('install')
root = load('install-root')


class InstallerTests(unittest.TestCase):
    def test_unsupported_distribution_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'os-release'
            for content in ['ID=debian\nVERSION_ID="13"\n', 'ID=fedora\nVERSION_ID=43\n']:
                path.write_text(content)
                with self.assertRaises(RuntimeError): installer.fedora(path)
            path.write_text('ID=fedora\nVERSION_ID="44"\n')
            self.assertEqual(installer.fedora(path)['ID'], 'fedora')

    def bundle(self, stage):
        files = {name: b'fixture' for name in set(root.FILES) - {'linuxhello_tpm.pp'}}
        files['models/test.onnx'] = b'pinned'
        (stage / 'models').mkdir()
        for name, data in files.items(): (stage / name).write_bytes(data)
        manifest = {'version': 1, 'commit': 'fixture', 'files': {name: hashlib.sha256(data).hexdigest() for name, data in files.items()}}
        (stage / 'manifest.json').write_text(json.dumps(manifest))
        return manifest

    def test_bundle_rejects_changed_files_and_unpinned_models(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(root, 'MODELS', {'test.onnx': (6, hashlib.sha256(b'pinned').hexdigest())}):
            stage = Path(tmp); manifest = self.bundle(stage)
            root.read_bundle(stage, os.getuid())
            (stage / 'hello').write_bytes(b'changed')
            with self.assertRaises(RuntimeError): root.read_bundle(stage, os.getuid())
            (stage / 'hello').write_bytes(b'fixture')
            (stage / 'models/test.onnx').write_bytes(b'wrong!')
            manifest['files']['models/test.onnx'] = hashlib.sha256(b'wrong!').hexdigest()
            (stage / 'manifest.json').write_text(json.dumps(manifest))
            with self.assertRaises(RuntimeError): root.read_bundle(stage, os.getuid())

    def test_bundle_rejects_symlinks_hardlinks_and_path_injection(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(root, 'MODELS', {'test.onnx': (6, hashlib.sha256(b'pinned').hexdigest())}):
            stage = Path(tmp); manifest = self.bundle(stage)
            (stage / 'hello').unlink(); (stage / 'hello').symlink_to(stage / 'hello-sudo')
            with self.assertRaises(OSError): root.read_bundle(stage, os.getuid())
            (stage / 'hello').unlink(); os.link(stage / 'hello-sudo', stage / 'hello')
            with self.assertRaises(RuntimeError): root.read_bundle(stage, os.getuid())
            (stage / 'hello').unlink(); (stage / 'hello').write_bytes(b'fixture')
            manifest['files']['../outside'] = 'anything'
            (stage / 'manifest.json').write_text(json.dumps(manifest))
            with self.assertRaises(RuntimeError): root.read_bundle(stage, os.getuid())

    def test_failed_install_restores_previous_module_and_helper(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp); base = folder / 'base'; state = folder / 'state'; pam = folder / 'pam.so'; bindir = folder / 'bin'; desktop = folder / 'apps/hello.desktop'
            base.mkdir(); state.mkdir(mode=0o700)
            (base / 'hello-sudo').write_bytes(b'old-helper'); pam.write_bytes(b'old-pam')
            def protected(path, private=False): return path.lstat() if path.exists() else None
            def directory(path, mode): path.mkdir(mode=mode, parents=True, exist_ok=True)
            def run(args):
                if '-R' in args: raise subprocess.CalledProcessError(1, args)
            with patch.multiple(root, BASE=base, STATE=state, PAM=pam, BIN=bindir, DESKTOP=desktop), patch.object(root, 'protected', side_effect=protected), patch.object(root, 'directory', side_effect=directory), patch.object(root, 'run', side_effect=run), patch.object(root.subprocess, 'check_output', return_value=''):
                with self.assertRaises(subprocess.CalledProcessError):
                    root.install({'version': 1}, {'hello-sudo': b'new-helper', 'pam_linuxhello.so': b'new-pam'})
            self.assertEqual((base / 'hello-sudo').read_bytes(), b'old-helper')
            self.assertEqual(pam.read_bytes(), b'old-pam')
            self.assertFalse((bindir / 'linux-hello').exists())
            self.assertFalse(desktop.exists())

    def test_dry_run_invokes_no_external_commands(self):
        with patch.object(installer, 'run', side_effect=AssertionError('side effect')), patch.object(installer, 'fedora', return_value={'ID':'fedora'}), patch.object(installer.os, 'geteuid', return_value=1000), patch('sys.argv', ['install.py', '--dry-run', '--with-tpm']):
            installer.main()

    def test_loaded_policy_contents_are_checked_and_admin_overrides_refused(self):
        with patch.object(root,'loaded_policy',return_value=b'old-reviewed'):
            changes=root.checked_policies({'linuxhello_kde.pp':b'new'}, {'linuxhello_kde'},
                {root.BASE/'linuxhello_kde.pp':(b'old-reviewed',0o644)})
            self.assertEqual(changes,[('linuxhello_kde',b'old-reviewed')])
            with self.assertRaisesRegex(RuntimeError,'administrator override'):
                root.checked_policies({'linuxhello_kde.pp':b'new'}, {'linuxhello_kde'},
                    {root.BASE/'linuxhello_kde.pp':(b'different',0o644)})
        with patch.object(root,'loaded_policy',return_value=b'new'):
            self.assertEqual(root.checked_policies({'linuxhello_kde.pp':b'new'}, {'linuxhello_kde'}, {}),[])
