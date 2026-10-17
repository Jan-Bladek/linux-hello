"""Shared Rust/Python policy corpus and actual private Linux-PAM syntax checks."""
import ctypes
import ctypes.util
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('audit_policy_manager',ROOT/'packaging/sudo/manage.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)

class PamPolicyTests(unittest.TestCase):
    def test_same_reviewed_corpus_as_rust(self):
        for case in json.loads((ROOT/'tests/fixtures/pam-policy-cases.json').read_text()):
            with self.subTest(name=case['name']):
                if case['password_allowed']:
                    m.supported_password_policy(case['text'])
                else:
                    with self.assertRaises(RuntimeError):m.supported_password_policy(case['text'])

    def test_sudo_insertion_cannot_hide_mixed_case_auth_or_continuation(self):
        for rule in ['AUTH required pam_example_mfa.so\n','Auth required pam_access.so\n','-AUTH required pam_faillock.so preauth\n','auth required \\\n pam_example_mfa.so\n','@include custom\n']:
            source='auth include system-auth\n'+rule
            with self.subTest(rule=rule):
                with self.assertRaises(RuntimeError):m.changed(source,True)
                managed=m.MARKER+m.LINE+source
                with self.assertRaises(RuntimeError):m.changed(managed,True)
                self.assertEqual(m.changed(managed,False),source)

    @unittest.skipUnless(ctypes.util.find_library('pam'),'Linux-PAM unavailable')
    def test_private_libpam_interprets_case_and_preserves_required_deny(self):
        pam=ctypes.CDLL(ctypes.util.find_library('pam'))
        if not hasattr(pam,'pam_start_confdir'):self.skipTest('private PAM confdir unavailable')
        callback=ctypes.CFUNCTYPE(ctypes.c_int,ctypes.c_int,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p)
        @callback
        def converse(*_):return 19
        class Conv(ctypes.Structure):_fields_=[('callback',callback),('data',ctypes.c_void_p)]
        conv=Conv(converse,None)
        pam.pam_start_confdir.argtypes=[ctypes.c_char_p,ctypes.c_char_p,ctypes.POINTER(Conv),ctypes.c_char_p,ctypes.POINTER(ctypes.c_void_p)]
        pam.pam_authenticate.argtypes=[ctypes.c_void_p,ctypes.c_int];pam.pam_end.argtypes=[ctypes.c_void_p,ctypes.c_int]
        with tempfile.TemporaryDirectory(prefix='hello-private-pam-syntax-') as tmp:
            for keyword in ('auth','AUTH','Auth','-AUTH'):
                text=keyword+' required pam_deny.so\nauth required pam_permit.so\n'
                with self.assertRaises(RuntimeError):m.supported_password_policy(text)
                (Path(tmp)/'fixture').write_text(text)
                handle=ctypes.c_void_p()
                self.assertEqual(pam.pam_start_confdir(b'fixture',b'synthetic',ctypes.byref(conv),tmp.encode(),ctypes.byref(handle)),0)
                try:self.assertNotEqual(pam.pam_authenticate(handle,0),0)
                finally:pam.pam_end(handle,0)
