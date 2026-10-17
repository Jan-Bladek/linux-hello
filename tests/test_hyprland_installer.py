"""Profile orchestration tests: real biometric setup is never simulated as deployed."""
import contextlib
import importlib.util
import io
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('hyprland_installer', REPO/'packaging/installer/install-hyprland-noctalia.py')
m = importlib.util.module_from_spec(spec);spec.loader.exec_module(m)


class HyprlandInstallerTests(unittest.TestCase):
    def run_profile(self, options, runner=None):
        stack=contextlib.ExitStack();self.addCleanup(stack.close)
        stack.enter_context(patch.object(m.os,'getuid',return_value=1000))
        stack.enter_context(patch.object(m.os,'geteuid',return_value=1000))
        stack.enter_context(patch.object(m,'check_platform'))
        stack.enter_context(patch.object(m,'check_desktop'))
        stack.enter_context(patch.object(m.subprocess,'check_output',return_value='login\n'))
        run=stack.enter_context(patch.object(m,'run',side_effect=runner))
        stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
        m.main(options)
        return run

    def test_install_only_never_activates_or_opens_gui(self):
        run=self.run_profile(['--skip-deps','--offline','--with-tpm'])
        self.assertEqual(run.call_count,1)
        self.assertEqual(run.call_args.args[0],[m.sys.executable,m.REPO/'packaging/installer/install.py','--skip-deps','--offline','--with-tpm'])

    def test_explicit_setup_order_reuses_reviewed_managers(self):
        run=self.run_profile(['--enroll-sudo','--activate-login','--activate-unlock','--open-settings'])
        commands=[call.args[0] for call in run.call_args_list]
        self.assertEqual(commands[1:],[
            [m.BASE/'enroll-and-enable.sh'],[m.BASE/'activate-kde.sh'],
            [m.BASE/'activate-desktop.py','--service','noctalia'],['/usr/local/bin/linux-hello']])

    def test_failed_activation_stops_later_setup_without_claiming_rollback(self):
        calls=[]
        def run(args):
            calls.append(args)
            if args==[m.BASE/'activate-kde.sh']:
                raise subprocess.CalledProcessError(1,args)
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_profile(['--activate-login','--activate-unlock','--open-settings'],run)
        self.assertEqual(len(calls),2)
        self.assertFalse(any('activate-desktop.py' in str(item) for call in calls for item in call))

    def test_build_only_does_not_touch_installed_helpers(self):
        with patch.object(m.subprocess,'check_output',side_effect=AssertionError('installed adapter invoked')):
            # run_profile patches this function too, so use direct setup here.
            with patch.object(m.os,'getuid',return_value=1000),patch.object(m.os,'geteuid',return_value=1000),patch.object(m,'check_platform'),patch.object(m,'check_desktop'),patch.object(m,'run') as run,contextlib.redirect_stdout(io.StringIO()):
                m.main(['--build-only','--skip-deps','--offline'])
                self.assertEqual(run.call_count,1)
                self.assertIn('--build-only',run.call_args.args[0])

    def test_dry_run_is_inert_and_invalid_combinations_fail_before_actions(self):
        with patch.object(m.os,'getuid',return_value=1000),patch.object(m.os,'geteuid',return_value=1000),patch.object(m,'check_platform',side_effect=AssertionError('platform read')),patch.object(m,'check_desktop',side_effect=AssertionError('desktop command')),patch.object(m,'run',side_effect=AssertionError('side effect')),patch.object(m.subprocess,'check_output',side_effect=AssertionError('side effect')),contextlib.redirect_stdout(io.StringIO()),contextlib.redirect_stderr(io.StringIO()):
            m.main(['--dry-run','--activate-unlock'])
            for options in (['--offline'],['--build-only','--activate-unlock'],['--build-only','--open-settings'],['--user','other']):
                with self.assertRaises(SystemExit):m.main(options)

    def test_root_or_setuid_caller_is_rejected(self):
        for real,effective in ((0,0),(1000,0)):
            with patch.object(m.os,'getuid',return_value=real),patch.object(m.os,'geteuid',return_value=effective),contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit):m.main(['--dry-run'])

    def test_missing_or_unsupported_desktop_fails_before_install(self):
        with patch.object(m.Path,'is_file',return_value=False):
            with self.assertRaisesRegex(RuntimeError,'Missing'):m.check_desktop()
        with patch.object(m.Path,'is_file',return_value=True),patch.object(m.os,'access',return_value=True),patch.object(m.subprocess,'check_output',return_value='noctalia v4.6.0'):
            with self.assertRaisesRegex(RuntimeError,'Noctalia 5.x'):m.check_desktop()
        with patch.object(m.os,'getuid',return_value=1000),patch.object(m.os,'geteuid',return_value=1000),patch.object(m,'check_platform'),patch.object(m,'check_desktop',side_effect=RuntimeError('unsupported')),patch.object(m,'run') as run,contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(RuntimeError):m.main([])
            run.assert_not_called()
