#!/usr/bin/python3
"""Synthetic root profile recovery and unprivileged native parsing, isolated state.
No camera, policy/PAM edits, installed executable replacement or real profiles.
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
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix='hello-audit-synthetic-',dir='/run') as tmp:
        state=Path(tmp);(state/'.audit-test-only').write_bytes(b'synthetic-only\n')
        (state/'.anchor').touch(mode=0o600);(state/'.setup.lock').touch(mode=0o600)
        subprocess.run(['/usr/bin/mount','--bind',str(state),str(STATE)],check=True)
        try:
            subprocess.run([sys.argv[1],'--exact','tests::interrupted_publication_recovers_only_previous_synthetic_profile','--ignored','--nocapture'],check=True,timeout=20)
            with (STATE/'.setup.lock').open('rb') as lock:
                fcntl.flock(lock,fcntl.LOCK_EX | fcntl.LOCK_NB)
                subprocess.run([sys.argv[1],'--exact','tests::delegated_verification_keeps_parent_exclusive_lock','--ignored','--nocapture'],check=True,timeout=10,
                    pass_fds=(lock.fileno(),),env={**os.environ,'LINUX_HELLO_SETUP_LOCK_FD':str(lock.fileno())})
            subprocess.run([sys.argv[2],'--exact','face_worker::limits::audit_tests::root_worker_drops_all_credentials_before_real_model_parsing','--ignored','--nocapture'],check=True,timeout=20)
        finally:
            subprocess.run(['/usr/bin/umount',str(STATE)],check=True)
    print('PASS: synthetic crash/rollback recovery and dropped-credential model loading; no physical verification.')
if __name__=='__main__':main()
