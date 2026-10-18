"""Synthetic audit regression tests. No camera, root state or PAM changes."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module
sudo = load('audit_sudo', 'packaging/sudo/manage.py')
kde = load('audit_kde', 'packaging/kde/manage-kde.py')
setup = load('audit_setup', 'packaging/kde/configure-person.py')
account = load('audit_account', 'packaging/kde/manage-person.py')

class AuditRegressions(unittest.TestCase):
    def test_mfa_in_parent_or_password_stack_is_refused(self):
        policy='auth required pam_env.so\nauth required pam_faildelay.so delay=2000000\nauth sufficient pam_unix.so nullok\nauth required pam_deny.so\n'
        sudo.supported_password_policy(policy)
        for extra in ['auth required pam_example_mfa.so\n', 'auth required pam_faillock.so preauth\n', 'auth include custom-policy\n']:
            with self.assertRaises(RuntimeError): sudo.supported_password_policy(policy+extra)
            with self.assertRaises(RuntimeError): sudo.changed('auth include system-auth\n'+extra,True)
        enabled=sudo.changed('auth include system-auth\n',True)
        self.assertLess(enabled.index('pam_env.so'),enabled.index('pam_linuxhello.so'))
        self.assertLess(kde.AUTH_STACK.index('pam_faildelay.so'),kde.AUTH_STACK.index('pam_linuxhello.so'))

    def test_partial_header_body_and_missing_eof_share_one_deadline(self):
        request=json.dumps({'sudo':True,'login':False,'unlock':False,'source':'/private'}).encode()+b'\n'
        complete=request+struct.pack('!H',14)+b'separatephrase'
        for payload in [b'', b'{', request+b'\0', complete[:-2], complete]:
            read,write=os.pipe()
            try:
                os.write(write,payload)
                with os.fdopen(read,'rb') as stream:
                    started=time.monotonic()
                    with self.assertRaises(TimeoutError): setup.request(setup.DeadlineInput(stream,0.04))
                    self.assertLess(time.monotonic()-started,0.5)
            finally: os.close(write)

    def test_failed_validation_never_claims_enrolled_or_enabled(self):
        with tempfile.TemporaryDirectory() as tmp:
            state=Path(tmp); profile=state/'1000'; profile.mkdir()
            (profile/'config.json').write_text('{"version":1,"uid":1000,"threshold":0.8}')
            (profile/'template.lht').write_bytes(b'corrupt')
            (profile/'key.tpm').write_bytes(b'corrupt')
            manager=SimpleNamespace(ROOT=state,protected=lambda p,private=False:p.stat())
            with patch.object(account,'private_profile',return_value=True),patch.object(account,'reference_valid',return_value=False):
                result=account.status(SimpleNamespace(pw_uid=1000),manager,kde)
            self.assertFalse(result['enrolled'])
            self.assertFalse(any(result[k] for k in ('sudo','login','unlock')))

    def test_rollback_continues_after_write_and_label_failures(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths=[Path(tmp)/str(i) for i in range(3)]
            for p in paths:p.write_bytes(b'new')
            def write(p,data,mode):
                if p==paths[0]:raise OSError('injected write failure')
                p.write_bytes(data)
            with patch.object(sudo.subprocess,'run',side_effect=RuntimeError('injected label failure')):
                with self.assertRaisesRegex(RuntimeError,'Rollback incomplete'):
                    sudo.restore_snapshots({p:(b'old',0o600) for p in paths},write)
            self.assertEqual(paths[1].read_bytes(),b'old')
            self.assertEqual(paths[2].read_bytes(),b'old')

    def test_failed_kde_enable_has_no_production_marker_during_probe(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);state=root/'state';state.mkdir();profile=state/'1000';profile.mkdir()
            (state/'.anchor').touch();(profile/'config.json').write_text('{}')
            pam=root/'pam';pam.mkdir();service=pam/'plasmalogin'
            before=b'auth substack password-auth\n';service.write_bytes(before)
            marker=profile/'pam-plasmalogin.enabled'
            def atomic(path,data,mode):path.write_bytes(data);path.chmod(mode)
            def probe(*args):
                self.assertFalse(marker.exists())
                # Simulate an administrator edit after successful live checks.
                service.write_bytes(b'auth required administrator_change.so\n')
            real_path=kde.Path
            def paths(value):
                return pam if value=='/etc/pam.d' else real_path(value)
            fake=SimpleNamespace(auth_lines=sudo.auth_lines,bump_epoch=lambda:None,ROOT=state,PREAUTH=sudo.PREAUTH,protected=lambda p,private=False:SimpleNamespace(st_mode=0o100644),
                atomic_write=atomic,check_password_policy=lambda name:(profile/'config.json',b'{}'))
            with patch.object(kde,'sudo',fake),patch.object(kde,'ROOT',state),patch.object(kde,'Path',side_effect=paths),\
                 patch.object(kde,'detect',return_value='plasmalogin'),patch.object(kde,'probe',side_effect=probe),\
                 patch('pwd.getpwnam',return_value=SimpleNamespace(pw_uid=1000)),\
                 patch('sys.argv',['manage-kde.py','enable','--user','owner']),\
                 patch.object(kde.os,'geteuid',return_value=0),contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaisesRegex(RuntimeError,'changed during verification'):kde.main()
            self.assertFalse(marker.exists())
            self.assertEqual(service.read_bytes(),b'auth required administrator_change.so\n')

    def test_late_activation_write_failure_restores_previous_marker_bytes(self):
        with tempfile.TemporaryDirectory() as tmp:
            state=Path(tmp);profile=state/'1000';profile.mkdir()
            marker=profile/'pam-kde.enabled';marker.write_bytes(b'previous-opt-in')
            companion=state/'companion';companion.write_bytes(b'previous-policy')
            def atomic(path,data,mode):path.write_bytes(data);path.chmod(mode)
            with patch.object(sudo,'ROOT',state),patch.object(sudo,'protected',side_effect=lambda p,private=False:p.stat()),\
                 patch.object(sudo,'atomic_write',side_effect=atomic),patch.object(sudo.subprocess,'run'):
                with self.assertRaisesRegex(RuntimeError,'late label failure'):
                    with sudo.write_transaction():
                        sudo.atomic_write(companion,b'new-policy',0o644)
                        sudo.atomic_write(marker,b'linux-hello-login-v1\n',0o600)
                        raise RuntimeError('late label failure')
            self.assertEqual(marker.read_bytes(),b'previous-opt-in')
            self.assertEqual(companion.read_bytes(),b'previous-policy')
