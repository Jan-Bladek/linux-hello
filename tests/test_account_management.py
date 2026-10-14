import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('account_management', Path(__file__).parents[1] / 'packaging/kde/manage-person.py')
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
helper_spec = importlib.util.spec_from_file_location('audit_recovery', Path(__file__).parents[1] / 'packaging/sudo/manage.py')
recovery = importlib.util.module_from_spec(helper_spec); helper_spec.loader.exec_module(recovery)


class AccountManagementTests(unittest.TestCase):
    def setUp(self):
        validator = patch.object(m, 'reference_valid', return_value=True)
        validator.start(); self.addCleanup(validator.stop)

    def test_subject_is_authorized_normal_account_only(self):
        for environment in [{}, {'PKEXEC_UID':'0'}, {'PKEXEC_UID':'-1'}, {'SUDO_UID':'garbage'}, {'PKEXEC_UID':'0','SUDO_UID':'1000'}]:
            with self.assertRaises(RuntimeError): m.subject(environment)
        with patch.object(m.pwd, 'getpwuid', return_value='authorized') as lookup:
            self.assertEqual(m.subject({'PKEXEC_UID':'1000','SUDO_UID':'1001'}), 'authorized')
            lookup.assert_called_once_with(1000)

    def fixture(self, folder):
        state=folder/'state';state.mkdir();pam=folder/'pam';pam.mkdir()
        profile=state/'1000';profile.mkdir();other=state/'1001';other.mkdir()
        account=SimpleNamespace(pw_uid=1000,pw_name='owner')
        (profile/'config.json').write_text(json.dumps({'uid':1000,'version':1,'threshold':0.93}))
        (profile/'template.lht').write_bytes(b'encrypted-face-secret')
        (profile/'key.tpm').write_bytes(b'encrypted-key-secret')
        for owner in (profile,other):
            for service in ('plasmalogin','kde'): (owner/f'pam-{service}.enabled').write_bytes(m.MAGIC)
        sudo=SimpleNamespace(bump_epoch=lambda:None,ROOT=state,PAM=pam/'sudo',MARKER='# sudo\n',LINE='auth sufficient face\n',
            child_environment=lambda:{'LANG':'C.UTF-8'},child_lock_options=lambda:{},
            protected=lambda p,private=False:p.stat(),atomic_write=lambda p,b,mode:p.write_bytes(b),
            restore_snapshots=lambda snapshots: recovery.restore_snapshots(snapshots, lambda p,b,mode:p.write_bytes(b)))
        kde=SimpleNamespace(SERVICES=('plasmalogin','sddm'),SUBSTACK='face-substack',MARKER='# kde\n',LINE='auth substack face\n')
        (pam/'sudo').write_text(sudo.MARKER+sudo.LINE)
        for service in ('plasmalogin','kde'): (pam/service).write_text(kde.MARKER+kde.LINE)
        def command(args,**kwargs):
            if '--service' in args: (profile/f'pam-{args[-1]}.enabled').unlink(missing_ok=True)
        return state,pam,profile,other,account,sudo,kde,command

    def test_pause_keeps_reference_and_other_accounts_enabled(self):
        with tempfile.TemporaryDirectory() as tmp:
            state,pam,profile,other,account,sudo,kde,command=self.fixture(Path(tmp))
            before={p:p.read_bytes() for p in other.iterdir()};global_pam=(pam/'sudo').read_bytes()
            with patch.object(m,'PAM_DIR',pam),patch.object(m,'private_profile',side_effect=lambda p,uid:p.exists()),patch.object(m.subprocess,'run',side_effect=command):
                status=m.operate('pause',account,sudo,kde)
            self.assertTrue(status['enrolled']);self.assertEqual(status['key_backend'],'tpm')
            self.assertFalse(any(status[s] for s in ('sudo','login','unlock')))
            self.assertEqual((profile/'template.lht').read_bytes(),b'encrypted-face-secret')
            self.assertEqual((pam/'sudo').read_bytes(),global_pam)
            for p,data in before.items():self.assertEqual(p.read_bytes(),data)
            report=json.dumps(status)
            self.assertNotIn('encrypted-face-secret',report);self.assertNotIn('encrypted-key-secret',report)

    def test_remove_requires_completed_disable_and_rollback_survives_lost_ack(self):
        with tempfile.TemporaryDirectory() as tmp:
            state,pam,profile,other,account,sudo,kde,command=self.fixture(Path(tmp))
            before={p:p.read_bytes() for p in profile.iterdir()}
            def broken_ack(_): raise BrokenPipeError('lost application')
            with patch.object(m,'PAM_DIR',pam),patch.object(m,'private_profile',side_effect=lambda p,uid:p.exists()),patch.object(m.subprocess,'run',side_effect=command):
                with self.assertRaises(BrokenPipeError):m.operate('remove',account,sudo,kde,broken_ack)
                for p,data in before.items():self.assertEqual(p.read_bytes(),data)
                self.assertFalse((profile/'pam-sudo.disabled').exists())
                status=m.operate('remove',account,sudo,kde)
            self.assertFalse(status['enrolled']);self.assertEqual(status['key_backend'],'none')
            self.assertFalse((profile/'template.lht').exists());self.assertFalse((profile/'key.tpm').exists())
            self.assertTrue((other/'pam-kde.enabled').exists())

    def test_failed_kde_disable_restores_every_own_reference_and_marker(self):
        with tempfile.TemporaryDirectory() as tmp:
            state,pam,profile,other,account,sudo,kde,command=self.fixture(Path(tmp))
            before={p:p.read_bytes() for p in profile.iterdir()}
            def failing(args,**kwargs):
                if '--service' in args: raise RuntimeError('manager failed')
            with patch.object(m,'PAM_DIR',pam),patch.object(m,'private_profile',side_effect=lambda p,uid:p.exists()),patch.object(m.subprocess,'run',side_effect=failing):
                with self.assertRaises(RuntimeError):m.operate('remove',account,sudo,kde)
            for p,data in before.items():self.assertEqual(p.read_bytes(),data)
            self.assertFalse((profile/'pam-sudo.disabled').exists())
