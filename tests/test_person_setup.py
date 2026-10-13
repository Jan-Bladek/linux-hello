import importlib.util
import io
import json
from pathlib import Path
import struct
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('person_setup', Path(__file__).parents[1] / 'packaging/kde/configure-person.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class PersonSetup(unittest.TestCase):
    def frame(self, choices=None, secret=b'separate-phrase'):
        data = choices or {'sudo': True, 'login': False, 'unlock': True, 'source': '/private/reference'}
        return json.dumps(data).encode() + b'\n' + struct.pack('!H', len(secret)) + secret

    def test_only_known_boolean_choices_and_bounded_secret_are_accepted(self):
        data, secret = m.request(io.BytesIO(self.frame()))
        self.assertTrue(data['sudo'])
        self.assertEqual(secret, b'separate-phrase')
        for frame in [self.frame()+b'extra', self.frame()[:-2], self.frame(secret=b'short'),
                      self.frame({'sudo': 1, 'login': False, 'unlock': True, 'source': '/private'}),
                      self.frame({'sudo': True, 'login': False, 'unlock': True, 'source': 'relative'}),
                      b'{}\n'+struct.pack('!H',1025)]:
            with self.assertRaises((ValueError, struct.error)): m.request(io.BytesIO(frame))

    def test_partial_configuration_rolls_back_the_old_reference_and_pam(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            state = root / 'state'; state.mkdir()
            profile = state / '1000'; profile.mkdir()
            pam = root / 'pam'; pam.mkdir()
            (state / '.anchor').write_bytes(b'')
            old = {profile/'key': b'old-key', profile/'key.tpm': b'old-sealed-key', profile/'template.lht': b'old-encrypted-face',
                   profile/'config.json': b'old-config', pam/'sudo': b'old-sudo',
                   pam/'kde': b'old-lock', pam/'plasmalogin': b'old-login',
                   profile/'pam-plasmalogin.enabled': b'old-marker'}
            for path, contents in old.items(): path.write_bytes(contents)
            def atomic(path, data, mode): path.write_bytes(data)
            sudo = SimpleNamespace(ROOT=state, PAM=pam/'sudo', protected=lambda path, private=False: path.stat(), atomic_write=atomic)
            kde = SimpleNamespace(SUBSTACK='linuxhello-kde-auth', SERVICES=('plasmalogin','sddm'), detect=lambda:'plasmalogin')
            def failing(args, secret=None):
                if 'enroll-stdin' in args:
                    (profile/'key').write_bytes(b'new-key')
                    (profile/'key.tpm').write_bytes(b'changed-sealed-key')
                    (profile/'template.lht').write_bytes(b'new-face')
                    return
                (pam/'sudo').write_bytes(b'partial-change')
                (profile/'pam-plasmalogin.enabled').unlink()
                raise RuntimeError('failed real verification')
            data={'sudo':True,'login':True,'unlock':True,'source':'/private/reference'}
            with patch.object(m,'PAM_DIR',pam), patch.object(m,'profile_is_private',return_value=True), patch.object(m,'command',side_effect=failing), patch.object(m.time,'sleep'), patch.object(m.subprocess,'run'), patch.object(m,'progress'):
                with self.assertRaises(RuntimeError):
                    m.configure(data,bytearray(b'separate-phrase'),SimpleNamespace(pw_uid=1000,pw_name='owner'),sudo,kde)
            for path, contents in old.items(): self.assertEqual(path.read_bytes(), contents)

    def test_disabling_all_does_not_import_or_remove_another_accounts_sudo_path(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);state=root/'state';state.mkdir();pam=root/'pam';pam.mkdir()
            (state/'.anchor').write_bytes(b'');(pam/'sudo').write_bytes(b'other-users-still-enabled')
            sudo=SimpleNamespace(ROOT=state,PAM=pam/'sudo',protected=lambda p,private=False:p.stat(),atomic_write=lambda p,d,mode:p.write_bytes(d))
            kde=SimpleNamespace(SUBSTACK='linuxhello-kde-auth',SERVICES=('plasmalogin','sddm'))
            calls=[]
            with patch.object(m,'PAM_DIR',pam),patch.object(m,'profile_is_private',return_value=True),patch.object(m,'command',side_effect=lambda args,secret=None:calls.append(args)),patch.object(m,'progress'),patch.object(m,'complete'):
                result=m.configure({'sudo':False,'login':False,'unlock':False,'source':'/unused'},bytearray(),SimpleNamespace(pw_uid=1000,pw_name='owner'),sudo,kde)
            self.assertFalse(any(result.values()))
            self.assertTrue((state/'1000/pam-sudo.disabled').exists())
            self.assertEqual((pam/'sudo').read_bytes(),b'other-users-still-enabled')
            self.assertTrue(all('enroll-stdin' not in args for args in calls))
            # Losing the GUI result pipe after applying choices must restore the
            # old state even when ordinary verification itself succeeded.
            (state/'1000/pam-sudo.disabled').unlink()
            with patch.object(m,'PAM_DIR',pam),patch.object(m,'profile_is_private',return_value=True),patch.object(m,'command'),patch.object(m,'progress'),patch.object(m,'complete',side_effect=BrokenPipeError),patch.object(m.subprocess,'run'):
                with self.assertRaises(BrokenPipeError):
                    m.configure({'sudo':False,'login':False,'unlock':False,'source':'/unused'},bytearray(),SimpleNamespace(pw_uid=1000,pw_name='owner'),sudo,kde)
            self.assertFalse((state/'1000/pam-sudo.disabled').exists())
            self.assertEqual((pam/'sudo').read_bytes(),b'other-users-still-enabled')
