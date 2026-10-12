import importlib.util
from pathlib import Path
import unittest
spec=importlib.util.spec_from_file_location('kde_management',Path(__file__).parents[1]/'packaging/kde/manage-kde.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
class KdeManagement(unittest.TestCase):
    def test_preserves_selinux_wallet_account_session_and_exact_rollback(self):
        original='auth [success=done ignore=ignore default=bad] pam_selinux_permit.so\nauth substack password-auth\n-auth optional pam_kwallet5.so\nauth include postlogin\naccount required pam_nologin.so\naccount include password-auth\nsession required pam_selinux.so open\n'
        changed=m.changed(original,True)
        self.assertEqual(m.changed(changed,False),original)
        self.assertEqual(m.changed(changed,True),changed)
        self.assertTrue(changed.startswith(original.splitlines(keepends=True)[0]))
        self.assertIn(m.LINE+'auth substack password-auth\n-auth optional pam_kwallet5.so',changed)
        self.assertTrue(changed.endswith('account include password-auth\nsession required pam_selinux.so open\n'))
    def test_refuses_unknown_stack_and_malformed_jump(self):
        for value in ['auth sufficient pam_unix.so\n','auth substack password-auth\nauth substack password-auth\n',m.MARKER,m.LINE,m.MARKER+'other\n'+m.LINE]:
            with self.assertRaises(RuntimeError):m.changed(value,True)
    def test_disable_preserves_unrelated_configuration(self):
        source='auth required other.so\n'
        self.assertEqual(m.changed(source,False),source)
