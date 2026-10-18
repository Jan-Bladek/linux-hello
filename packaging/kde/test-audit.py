#!/usr/bin/python3
"""Synthetic policy/profile recovery and unprivileged native parsing, isolated state.
No camera, production policy/PAM edits, executable replacement or real profiles.
"""
import os
import fcntl
from pathlib import Path
import subprocess
import sys
import tempfile
STATE=Path('/var/lib/linux-hello/sudo')

def main():
    if os.geteuid()!=0 or len(sys.argv) not in (3,4):
        raise RuntimeError('Root and current hello-sudo/hello-cli test executables required')
    namespace=os.readlink('/proc/self/ns/mnt')
    if len(sys.argv)==3:
        subprocess.run(['/usr/bin/unshare','--mount','--propagation','private',sys.executable,
                        __file__,sys.argv[1],sys.argv[2],namespace],check=True)
        return
    assert namespace!=sys.argv[3] and namespace!=os.readlink('/proc/1/ns/mnt')
    # A renamed/missing ignored test must not turn a zero-test run into PASS.
    expected = [
        (sys.argv[1], 'tests::runtime_policy_checks_actual_sudo_i_and_mixed_case_controls'),
        (sys.argv[1], 'tests::interrupted_publication_recovers_only_previous_synthetic_profile'),
        (sys.argv[1], 'profile::tests::interrupted_prepare_and_retired_cleanup_preserve_the_active_generation'),
        (sys.argv[1], 'coordination::operation_tests::actual_reader_refuses_interrupted_operations_in_private_state'),
        (sys.argv[1], 'tests::delegated_verification_keeps_parent_exclusive_lock'),
        (sys.argv[2], 'face_worker::limits::audit_tests::root_worker_drops_all_credentials_before_real_model_parsing'),
    ]
    for executable in {exe for exe, _ in expected}:
        names = subprocess.check_output([executable, '--list'], text=True)
        for candidate, name in expected:
            if candidate == executable and name + ': test' not in names:
                raise RuntimeError('Current test executable is missing required regression: ' + name)
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix='hello-audit-synthetic-',dir='/run') as tmp:
        state=Path(tmp);(state/'.audit-test-only').write_bytes(b'synthetic-only\n')
        (state/'.anchor').touch(mode=0o600);(state/'.setup.lock').touch(mode=0o600)
        subprocess.run(['/usr/bin/mount','--bind',str(state),str(STATE)],check=True)
        pam=state/'pam';pam.mkdir(mode=0o700);(pam/'.audit-test-only').write_bytes(b'synthetic-only\n')
        pam_mounted=False
        try:
            subprocess.run(['/usr/bin/mount','--bind',str(pam),'/etc/pam.d'],check=True)
            pam_mounted=True
            subprocess.run([sys.argv[1],'--exact','tests::runtime_policy_checks_actual_sudo_i_and_mixed_case_controls','--ignored','--nocapture'],check=True,timeout=20)
            subprocess.run([sys.argv[1],'--exact','tests::interrupted_publication_recovers_only_previous_synthetic_profile','--ignored','--nocapture'],check=True,timeout=20)
            subprocess.run([sys.argv[1],'--exact','profile::tests::interrupted_prepare_and_retired_cleanup_preserve_the_active_generation','--ignored','--nocapture'],check=True,timeout=20)
            subprocess.run([sys.argv[1],'--exact','coordination::operation_tests::actual_reader_refuses_interrupted_operations_in_private_state','--ignored','--nocapture'],check=True,timeout=10)
            with (STATE/'.setup.lock').open('rb') as lock:
                fcntl.flock(lock,fcntl.LOCK_EX | fcntl.LOCK_NB)
                subprocess.run([sys.argv[1],'--exact','tests::delegated_verification_keeps_parent_exclusive_lock','--ignored','--nocapture'],check=True,timeout=10,
                    pass_fds=(lock.fileno(),),env={**os.environ,'LINUX_HELLO_SETUP_LOCK_FD':str(lock.fileno())})
            subprocess.run([sys.argv[2],'--exact','face_worker::limits::audit_tests::root_worker_drops_all_credentials_before_real_model_parsing','--ignored','--nocapture'],check=True,timeout=20)
        finally:
            if pam_mounted: subprocess.run(['/usr/bin/umount','/etc/pam.d'],check=True)
            subprocess.run(['/usr/bin/umount',str(STATE)],check=True)
    print('PASS: synthetic runtime PAM policy, crash/rollback/cleanup, admission and dropped-credential model loading; no physical verification.')
if __name__=='__main__':main()
