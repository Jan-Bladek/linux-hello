import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch
import contextlib
import io
import subprocess
spec=importlib.util.spec_from_file_location('kde_management',Path(__file__).parents[1]/'packaging/kde/manage-kde.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
class KdeManagement(unittest.TestCase):
    def test_diagnostics_only_change_the_isolated_test_stack(self):
        production=m.AUTH_STACK.encode()
        diagnostic=m.diagnostic_stack(production)
        self.assertIn(b'pam_linuxhello.so diagnostic\n',diagnostic)
        self.assertNotIn(b'diagnostic',production)
        self.assertEqual(diagnostic.replace(b' diagnostic\n',b'\n'),production)
        with self.assertRaises(RuntimeError): m.diagnostic_stack(b'auth substack password-auth\n')

    def test_failed_probe_explains_exact_match_failure_and_never_accepts_it(self):
        output='{"similarity":0.7128,"threshold":0.8,"outcome":"rejected","cleanup_ok":true,"failure_reason":"centroid_similarity_below_threshold"}\nPAM_AUTHENTICATE=7\n'
        with patch.object(m.subprocess,'run',return_value=subprocess.CompletedProcess([],1,'',output)), patch.object(m.time,'sleep'), contextlib.redirect_stdout(io.StringIO()) as log:
            with self.assertRaises(m.ActivationError): m.run_probe([],1)
        self.assertIn('0.713',log.getvalue())
        self.assertIn('0.800',log.getvalue())
        self.assertIn('sredniego wzorca',log.getvalue())
        self.assertIn('PAM_AUTHENTICATE=7',log.getvalue())

    def test_probe_timeout_fails_closed_and_success_keeps_all_pam_checks(self):
        with patch.object(m.time,'sleep'), contextlib.redirect_stdout(io.StringIO()):
            with patch.object(m.subprocess,'run',side_effect=subprocess.TimeoutExpired([],40)):
                with self.assertRaises(m.ActivationError): m.run_probe([],2)
            with patch.object(m.subprocess,'run',return_value=subprocess.CompletedProcess([],0,'','PAM_AUTHENTICATE=0\nPAM_ACCT_MGMT=0\nPAM_SETCRED=0\n')):
                m.run_probe([],2)

    def test_preserves_selinux_wallet_account_session_and_exact_rollback(self):
        original='auth [success=done ignore=ignore default=bad] pam_selinux_permit.so\nauth substack password-auth\n-auth optional pam_kwallet5.so\nauth include postlogin\naccount required pam_nologin.so\naccount include password-auth\nsession required pam_selinux.so open\n'
        changed=m.changed(original,True)
        self.assertEqual(m.changed(changed,False),original)
        self.assertEqual(m.changed(changed,True),changed)
        self.assertTrue(changed.startswith(original.splitlines(keepends=True)[0]))
        self.assertIn(m.LINE+m.ORIGINAL+'auth substack password-auth\n-auth optional pam_kwallet5.so',changed)
        self.assertTrue(changed.endswith('account include password-auth\nsession required pam_selinux.so open\n'))
    def test_refuses_unknown_stack_and_malformed_jump(self):
        for value in ['auth sufficient pam_unix.so\n','auth substack password-auth\nauth substack password-auth\n',m.MARKER,m.LINE,m.MARKER+'other\n'+m.LINE]:
            with self.assertRaises(RuntimeError):m.changed(value,True)
    def test_disable_preserves_unrelated_configuration(self):
        source='auth required other.so\n'
        self.assertEqual(m.changed(source,False),source)

    def test_legacy_jump_migrates_and_saved_line_is_validated(self):
        original='auth   substack    password-auth\n-auth optional pam_kwallet5.so\n'
        legacy=m.MARKER+m.LEGACY_LINE+original
        current=m.changed(legacy,True)
        self.assertNotIn(m.LEGACY_LINE,current)
        self.assertEqual(m.changed(current,False),original)
        self.assertEqual(m.changed(legacy,False),original)
        with self.assertRaises(RuntimeError):
            m.changed(current.replace(m.ORIGINAL+'auth   substack    password-auth',m.ORIGINAL+'auth required pam_permit.so'),False)
