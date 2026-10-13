#!/usr/bin/python3
"""Real management helper/child managers with synthetic profiles in PRIVATE mounts.
No camera, face match, production profile or production PAM writes.
"""
import json
import os
from pathlib import Path
import pwd
import subprocess
import sys
import tempfile

BASE = Path('/usr/local/libexec/linux-hello-sudo')
ROOT = Path('/var/lib/linux-hello/sudo')
PAM = Path('/etc/pam.d')


def main():
    if os.geteuid() != 0 or len(sys.argv) not in (2, 3): raise RuntimeError('Run with sudo: test-person.py USER')
    current = os.readlink('/proc/self/ns/mnt')
    if len(sys.argv) == 2:
        subprocess.run(['/usr/bin/unshare', '--mount', '--propagation', 'private', sys.executable,
                        __file__, sys.argv[1], current], check=True)
        return
    assert current != sys.argv[2] and current != os.readlink('/proc/1/ns/mnt')
    account = pwd.getpwnam(sys.argv[1]); uid = account.pw_uid
    assert uid >= 1000
    os.umask(0o077)
    with tempfile.TemporaryDirectory(dir='/run', prefix='hello-person-test-') as tmp:
        state = Path(tmp) / 'state'; state.mkdir()
        conf = Path(tmp) / 'pam'; conf.mkdir()
        # Preserve the exact administrator preimage only in this disposable namespace.
        for name in ('sudo', 'kde', 'plasmalogin', 'sddm', 'linuxhello-kde-auth'):
            source = PAM / name
            if source.is_file(): (conf / name).write_bytes(source.read_bytes())
        (state / '.anchor').write_bytes(b'')
        profile = state / str(uid); profile.mkdir()
        other = state / str(uid + 10000); other.mkdir()
        (profile / 'config.json').write_text(json.dumps({'uid': uid, 'version': 1, 'threshold': 0.93}))
        (profile / 'template.lht').write_bytes(b'synthetic-encrypted-template')
        (profile / 'key.tpm').write_bytes(b'synthetic-sealed-key')
        for owner in (profile, other):
            for service in ('plasmalogin', 'kde'):
                if (conf / service).exists(): (owner / f'pam-{service}.enabled').write_bytes(b'linux-hello-login-v1\n')
        before = {p: p.read_bytes() for p in conf.iterdir()}
        env = {'LANG': 'C.UTF-8', 'SUDO_UID': str(uid)}
        def invoke(action):
            result = subprocess.check_output([str(BASE / 'manage-person.py'), action], env=env)
            data = json.loads(result)
            assert data['uid'] == uid and data['operation'] == action
            assert b'synthetic-encrypted-template' not in result and b'synthetic-sealed-key' not in result
            return data
        subprocess.run(['/usr/bin/mount', '--bind', str(state), str(ROOT)], check=True)
        subprocess.run(['/usr/bin/mount', '--bind', str(conf), str(PAM)], check=True)
        try:
            assert invoke('status')['enrolled']
            paused = invoke('pause')
            assert paused['enrolled'] and not any(paused[name] for name in ('sudo', 'login', 'unlock'))
            assert (profile / 'template.lht').exists() and (profile / 'key.tpm').exists()
            assert all(p.read_bytes() == data for p, data in before.items())
            assert all(p.read_bytes() == b'linux-hello-login-v1\n' for p in other.iterdir())
            removed = invoke('remove')
            assert not removed['enrolled'] and removed['key_backend'] == 'none'
            assert not (profile / 'template.lht').exists() and not (profile / 'key.tpm').exists()
            assert all(p.read_bytes() == data for p, data in before.items())
            rejected = subprocess.run([str(BASE / 'manage-person.py'), 'status'], env={'LANG':'C.UTF-8','PKEXEC_UID':'0','SUDO_UID':str(uid)}, stdout=subprocess.DEVNULL)
            assert rejected.returncode != 0
            print('PASS: real self-account status/pause/remove, other-account isolation and rejected subject. No production changes.')
        finally:
            subprocess.run(['/usr/bin/umount', str(PAM)], check=True)
            subprocess.run(['/usr/bin/umount', str(ROOT)], check=True)


if __name__ == '__main__': main()
