#!/usr/bin/python3
"""Fresh file installation test in a PRIVATE namespace, reusing installed policies.
Root only: python3 test-install.py /absolute/private/stage. No real templates used.
"""
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parents[2]
BASE = Path('/usr/local/libexec/linux-hello-sudo')
STATE = Path('/var/lib/linux-hello/sudo')


def main():
    if os.geteuid() != 0 or len(sys.argv) not in (2, 3): raise RuntimeError('Root and one prepared stage required')
    current = os.readlink('/proc/self/ns/mnt')
    if len(sys.argv) == 2:
        subprocess.run(['/usr/bin/unshare', '--mount', '--propagation', 'private', sys.executable,
                        __file__, sys.argv[1], current], check=True)
        return
    assert current != sys.argv[2] and current != os.readlink('/proc/1/ns/mnt')
    modules = subprocess.check_output(['/usr/sbin/semodule', '-l'], text=True)
    assert 'linuxhello_kde' in modules and 'linuxhello_tpm' in modules, 'Test requires existing policies; never installs policies in the caller namespace'
    contexts = subprocess.check_output(['/usr/sbin/semanage', 'fcontext', '-l', '-C'], text=True)
    assert any(line.split() and line.split()[0] == str(BASE / 'kde-pam-probe') and ':xdm_exec_t:' in line for line in contexts.splitlines())
    os.umask(0o077)
    with tempfile.TemporaryDirectory(dir='/run', prefix='hello-install-test-') as tmp:
        mounted = []
        targets = [BASE, STATE, Path('/usr/local/bin'), Path('/usr/local/share/applications'), Path('/usr/lib64/security')]
        try:
            for index, target in enumerate(targets):
                assert target.is_dir() and not target.is_symlink()
                fixture = Path(tmp) / str(index); fixture.mkdir(mode=0o700 if target == STATE else 0o755)
                subprocess.run(['/usr/bin/mount', '--bind', str(fixture), str(target)], check=True); mounted.append(target)
            subprocess.run([sys.executable, '-B', str(REPO / 'packaging/installer/install-root.py'), sys.argv[1]], check=True)
            for name in ['hello', 'hello-settings', 'hello-face-worker', 'hello-sudo', 'hello-lock-verify', 'kde-pam-probe']:
                path = BASE / name
                expected = 0o4755 if name == 'hello-lock-verify' else 0o755
                assert path.stat().st_uid == 0 and stat.S_IMODE(path.stat().st_mode) == expected
            assert (BASE / 'models/face_recognition_sface_2021dec.onnx').stat().st_size == 38696353
            assert (STATE / '.anchor').stat().st_mode & 0o777 == 0o600
            assert not (STATE / '1000').exists()
            assert Path('/usr/lib64/security/pam_linuxhello.so').is_file()
            subprocess.run(['/usr/local/bin/hello', '--help'], stdout=subprocess.DEVNULL, check=True)
            print('PASS: fresh isolated file installation, modes, model and CLI; no PAM activation or real profiles.')
        finally:
            for target in reversed(mounted): subprocess.run(['/usr/bin/umount', '-l', str(target)], check=True)


if __name__ == '__main__': main()
