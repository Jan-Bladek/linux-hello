"""Desktop PAM mutations, subject opt-in and Noctalia config: synthetic inputs."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT/path)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module
manager = load('desktop_manager', 'packaging/kde/manage-kde.py')
setup = load('desktop_setup', 'packaging/kde/configure-person.py')
account = load('desktop_account', 'packaging/kde/manage-person.py')
desktop = load('desktop_user', 'packaging/kde/activate-desktop.py')

class DesktopUnlock(unittest.TestCase):
    def test_login_and_hyprlock_roundtrip_preserve_every_other_pam_line(self):
        source = '#%PAM-1.0\nauth substack system-auth\nauth include postlogin\naccount required pam_nologin.so\naccount include system-auth\npassword include system-auth\nsession required pam_selinux.so close\nsession include system-auth\n'
        for service in ('login','hyprlock'):
            changed = manager.changed(source, True, service)
            self.assertEqual(manager.changed(changed, True, service), changed)
            self.assertEqual(manager.changed(changed, False, service), source)
            self.assertEqual(changed[changed.index('auth include postlogin'):], source[source.index('auth include postlogin'):])
            self.assertIn(f'auth        substack      linuxhello-{service}-auth\n', changed)
            self.assertTrue(manager.auth_stack(service).endswith('auth substack system-auth\n'))
            with self.assertRaises(RuntimeError): manager.changed(source.replace('system-auth','custom-auth'), True, service)
            with self.assertRaises(RuntimeError): manager.changed(changed.replace('# Linux Hello original password line: auth substack system-auth','# Linux Hello original password line: auth sufficient pam_permit.so'), False, service)
        with self.assertRaises(RuntimeError): manager.layout('greetd')

    def test_request_only_allows_reviewed_locker_services(self):
        for service in ('login','hyprlock','kde','sshd','../login',None,1000):
            request = {'sudo':False,'login':False,'unlock':True,'source':'/private','unlock_service':service}
            frame = json.dumps(request).encode()+b'\n'+struct.pack('!H',14)+b'separatephrase'
            if service in ('login','hyprlock','kde'):
                data, _ = setup.request(io.BytesIO(frame)); self.assertEqual(data['unlock_service'],service)
            else:
                with self.assertRaises(ValueError): setup.request(io.BytesIO(frame))

    def test_noctalia_overlay_is_user_only_atomic_and_never_enables_grace(self):
        with tempfile.TemporaryDirectory() as tmp:
            config = Path(tmp)/'noctalia';config.mkdir()
            unrelated = config/'custom.toml';unrelated.write_text('[theme]\nmode="dark"\n')
            target = config/'linux-hello.toml'
            def run(args,**kwargs):
                text = '[lockscreen]\nallow_empty_password = '+('true' if target.exists() else 'false')+'\n'
                return subprocess.CompletedProcess(args,0,text,'')
            with patch.dict(os.environ,{'NOCTALIA_CONFIG_HOME':tmp}),patch.object(desktop.os,'getuid',return_value=1000),patch.object(desktop.os,'geteuid',return_value=1000),patch.object(desktop.subprocess,'check_output',return_value='noctalia v5.3.0'),patch.object(desktop.subprocess,'run',side_effect=run):
                desktop.configure_noctalia()
                self.assertEqual(target.read_text(),desktop.OVERLAY)
                self.assertEqual(target.stat().st_mode & 0o777,0o600)
                self.assertNotIn('grace',target.read_text())
                self.assertEqual(unrelated.read_text(),'[theme]\nmode="dark"\n')
                desktop.configure_noctalia() # idempotent
            with patch.object(desktop.os,'getuid',return_value=0):
                with self.assertRaises(RuntimeError): desktop.configure_noctalia()

    def test_noctalia_override_conflict_rolls_back_only_owned_overlay(self):
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp)/'noctalia/linux-hello.toml'
            with patch.dict(os.environ,{'NOCTALIA_CONFIG_HOME':tmp}),patch.object(desktop.os,'getuid',return_value=1000),patch.object(desktop.os,'geteuid',return_value=1000),patch.object(desktop.subprocess,'check_output',return_value='noctalia v5.3.0'),patch.object(desktop.subprocess,'run',return_value=subprocess.CompletedProcess([],0,'[lockscreen]\nallow_empty_password=false\n','')):
                with self.assertRaisesRegex(RuntimeError,'override'): desktop.configure_noctalia()
                self.assertFalse(target.exists())
                target.write_text('custom = true\n')
                with self.assertRaisesRegex(RuntimeError,'local changes'): desktop.configure_noctalia()
                self.assertEqual(target.read_text(),'custom = true\n')

    def test_pause_revokes_all_locker_opt_ins_and_keeps_other_accounts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); state = root/'state';state.mkdir();pam=root/'pam';pam.mkdir()
            profile=state/'1000';profile.mkdir();other=state/'1001';other.mkdir()
            for service in manager.LOCK_SERVICES:
                for p in (profile,other): (p/f'pam-{service}.enabled').write_bytes(account.MAGIC)
            sudo=SimpleNamespace(ROOT=state,PAM=pam/'sudo',protected=lambda p,private=False:p.stat(),
                                 operation_journal=manager.sudo.operation_journal,bump_epoch=lambda:None,atomic_write=lambda p,b,mode:(p.write_bytes(b),p.chmod(mode)),
                                 child_environment=lambda:{},child_lock_options=lambda:{})
            calls=[]
            def run(args,**kwargs):
                service=args[-1];calls.append(service);(profile/f'pam-{service}.enabled').unlink()
            with patch.object(account,'private_profile',return_value=True),patch.object(account,'PAM_DIR',pam),patch.object(account.subprocess,'run',side_effect=run):
                result=account.operate('pause',SimpleNamespace(pw_uid=1000,pw_name='owner'),sudo,manager)
            self.assertFalse(result['unlock'])
            self.assertEqual(set(calls),set(manager.LOCK_SERVICES))
            for service in manager.LOCK_SERVICES:
                self.assertEqual((other/f'pam-{service}.enabled').read_bytes(),account.MAGIC)

    def test_created_hyprlock_shared_opt_in_and_exact_file_removal(self):
        for administrator_change in (False, True):
            with self.subTest(administrator_change=administrator_change), tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);state=root/'state';state.mkdir();profile=state/'1000';profile.mkdir()
                other=state/'1001';other.mkdir();pam=root/'pam';pam.mkdir();vendor=root/'vendor';vendor.mkdir()
                policy=state/'policy';policy.write_bytes(b'policy')
                fake=SimpleNamespace(auth_lines=manager.sudo.auth_lines,bump_epoch=lambda:None,PREAUTH=manager.sudo.PREAUTH,
                                     protected=lambda p,private=False:SimpleNamespace(st_mode=0o100644),
                                     check_password_policy=lambda name:(policy,b'policy'),
                                     atomic_write=lambda p,b,mode:(p.write_bytes(b),p.chmod(mode)))
                real_path=Path
                def paths(value):
                    if value=='/etc/pam.d': return pam
                    if value=='/usr/lib/pam.d': return vendor
                    return real_path(value)
                def probe(user, content, service):
                    self.assertEqual(service,'hyprlock')
                    self.assertFalse((profile/'pam-hyprlock.enabled').exists())
                    self.assertIn(b'linuxhello-hyprlock-auth',content)
                argv=['manage-kde.py','enable','--user','owner','--service','hyprlock']
                with patch.object(manager,'ROOT',state),patch.object(manager,'sudo',fake),patch.object(manager,'Path',side_effect=paths),patch.object(manager,'probe',side_effect=probe),patch.object(manager.os,'geteuid',return_value=0),patch.object(manager.subprocess,'run'),patch('pwd.getpwnam',return_value=SimpleNamespace(pw_uid=1000)),patch('sys.argv',argv),contextlib.redirect_stdout(io.StringIO()):
                    manager.main()
                    self.assertTrue((profile/'pam-hyprlock.enabled').exists())
                    self.assertTrue(json.loads((state/'kde-hyprlock-original.json').read_text())['created_service'])
                    (other/'pam-hyprlock.enabled').write_bytes(account.MAGIC)
                    argv[1]='disable';manager.main()
                    self.assertFalse((profile/'pam-hyprlock.enabled').exists())
                    self.assertIn('linuxhello-hyprlock-auth',(pam/'hyprlock').read_text())
                    (other/'pam-hyprlock.enabled').unlink()
                    if administrator_change:
                        with (pam/'hyprlock').open('a') as stream: stream.write('# Administrator addition\n')
                    manager.main()
                    self.assertEqual((pam/'hyprlock').exists(),administrator_change)
                    if administrator_change:
                        self.assertEqual((pam/'hyprlock').read_text(),manager.LOCKER_DEFAULT+'# Administrator addition\n')

    def test_activation_reports_partial_pam_commit_and_overlay_only_retry(self):
        for failure in (RuntimeError('override conflict'), subprocess.TimeoutExpired('noctalia', 10)):
            with patch.object(desktop.os,'getuid',return_value=1000),patch.object(desktop.os,'geteuid',return_value=1000),patch.object(desktop.subprocess,'run') as root,patch.object(desktop,'configure_noctalia',side_effect=failure),patch('sys.argv',['activate-desktop.py','--service','noctalia']),contextlib.redirect_stdout(io.StringIO()) as output,contextlib.redirect_stderr(io.StringIO()) as errors:
                self.assertEqual(desktop.main(),2)
                result=json.loads(next(line for line in output.getvalue().splitlines() if line.startswith('{')))
                self.assertTrue(result['pam_activated']);self.assertFalse(result['user_config_applied']);self.assertTrue(result['manual_step_required'])
                self.assertIn('PAM activation succeeded',errors.getvalue());self.assertIn('--configure-noctalia',errors.getvalue())
                self.assertEqual(root.call_count,1)
        with patch.object(desktop.os,'getuid',return_value=1000),patch.object(desktop.os,'geteuid',return_value=1000),patch.object(desktop.subprocess,'run') as root,patch.object(desktop,'configure_noctalia'),patch('sys.argv',['activate-desktop.py','--configure-noctalia']),contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(desktop.main(),0)
            self.assertIsNone(json.loads(output.getvalue())['pam_activated'])
            root.assert_not_called()
