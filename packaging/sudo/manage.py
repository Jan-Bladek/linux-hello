#!/usr/bin/python3
"""Root-only sudo PAM opt-in/rollback. Does not edit authselect/system-auth."""
import argparse
import hashlib
import os
from pathlib import Path
import stat
import subprocess
import tempfile

PAM = Path('/etc/pam.d/sudo')
ROOT = Path('/var/lib/linux-hello/sudo')
HELPER = '/usr/local/libexec/linux-hello-sudo/hello-sudo'
MARKER = '# Linux Hello sudo-only experimental integration\n'
LINE = 'auth       sufficient   /usr/lib64/security/pam_linuxhello.so\n'

def protected(path, private=False):
    path = Path(path)
    for parent in reversed(path.parents):
        m = parent.lstat()
        if not stat.S_ISDIR(m.st_mode) or m.st_uid != 0 or m.st_mode & 0o022:
            raise RuntimeError(f'Insecure ancestor: {parent}')
    m = path.lstat()
    mask = 0o077 if private else 0o022
    if not stat.S_ISREG(m.st_mode) or m.st_uid != 0 or m.st_mode & mask or m.st_nlink != 1:
        raise RuntimeError(f'Insecure file: {path}')
    return m

def changed(original, enable):
    # Strict paired markers: refuse malformed/hand-edited integration blocks.
    if original.count(MARKER) != original.count(LINE) or original.count(MARKER) > 1:
        raise RuntimeError('Unexpected existing Linux Hello PAM entries')
    if MARKER in original:
        if MARKER + LINE not in original:
            raise RuntimeError('Linux Hello PAM entries are not adjacent')
        return original if enable else original.replace(MARKER + LINE, '', 1)
    if not enable:
        return original
    lines = original.splitlines(keepends=True)
    expected = [i for i, s in enumerate(lines) if s.split() == ['auth', 'include', 'system-auth']]
    if len(expected) != 1:
        raise RuntimeError('Unsupported sudo PAM stack; review manually')
    lines.insert(expected[0], MARKER + LINE)
    return ''.join(lines)

def atomic_write(path, data, mode):
    fd, name = tempfile.mkstemp(prefix='.hello-', dir=path.parent)
    try:
        os.fchmod(fd, mode)
        with os.fdopen(fd, 'wb') as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(name, path)
        directory = os.open(path.parent, os.O_DIRECTORY)
        try: os.fsync(directory)
        finally: os.close(directory)
    finally:
        if os.path.exists(name): os.unlink(name)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['enable', 'disable', 'status'])
    parser.add_argument('--uid', type=int)
    args = parser.parse_args()
    if os.geteuid() != 0: parser.error('Run with sudo')
    meta = protected(PAM)
    original = PAM.read_bytes()
    text = original.decode('utf-8')
    if args.action == 'status':
        print('enabled' if MARKER + LINE in text else 'disabled')
        return
    if args.action == 'enable':
        if args.uid is None or args.uid < 1000: parser.error('--uid must identify an enrolled normal user')
        protected(HELPER)
        protected('/usr/lib64/security/pam_linuxhello.so')
        protected(ROOT / str(args.uid) / 'config.json', True)
        print('Look straight at the camera: checking the reference before enabling sudo.', flush=True)
        subprocess.run([HELPER, 'diagnose', str(args.uid)], check=True, timeout=40, env={'LANG':'C.UTF-8'})
    updated = changed(text, args.action == 'enable').encode()
    if updated == original:
        print('No changes needed')
        return
    protected(ROOT / '.anchor')
    backup = ROOT / ('sudo-before-' + hashlib.sha256(original).hexdigest()[:16] + '.pam')
    if not backup.exists(): atomic_write(backup, original, 0o600)
    # Refuse replacement if somebody changed the stack during the live scan.
    protected(PAM)
    if PAM.read_bytes() != original: raise RuntimeError('sudo PAM changed during verification')
    atomic_write(PAM, updated, stat.S_IMODE(meta.st_mode))
    subprocess.run(['/usr/sbin/restorecon', str(PAM)], check=True)
    print('sudo face authentication ' + args.action + 'd; original password/account/session stack retained')

if __name__ == '__main__':
    main()
