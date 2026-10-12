import importlib.util
from pathlib import Path
import unittest
spec = importlib.util.spec_from_file_location('sudo_management', Path(__file__).parents[1] / 'packaging/sudo/manage.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class SudoManagement(unittest.TestCase):
    def test_preserves_stack_and_rollback(self):
        original = '#%PAM-1.0\nauth include system-auth\naccount include system-auth\nsession include system-auth\n'
        enabled = module.changed(original, True)
        self.assertEqual(module.changed(enabled, False), original)
        self.assertEqual(module.changed(enabled, True), enabled)
        self.assertEqual(enabled.count(module.LINE), 1)
        self.assertIn('account include system-auth\nsession include system-auth\n', enabled)
    def test_refuses_unexpected_or_malformed_stack(self):
        for stack in ['auth sufficient pam_permit.so\n', 'auth include system-auth\nauth include system-auth\n', module.MARKER, module.LINE, module.MARKER + 'other\n' + module.LINE]:
            with self.assertRaises(RuntimeError): module.changed(stack, True)
    def test_disable_never_removes_other_lines(self):
        original = '# custom\nauth required pam_unix.so\n'
        self.assertEqual(module.changed(original, False), original)
