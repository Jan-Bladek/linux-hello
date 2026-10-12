#!/usr/bin/python3
"""Root-only isolated PAM stack tests BEFORE enrollment/activation. No sudo exec.
The pam_permit fixture tests fallback dispatch only, in a temporary confdir;
it is never written to /etc/pam.d and grants no operating-system privileges.
"""
import ctypes as c
import os
from pathlib import Path
import pwd
import tempfile

pam = c.CDLL('libpam.so.0')
pam.pam_start_confdir.argtypes = [c.c_char_p, c.c_char_p, c.c_void_p, c.c_char_p, c.POINTER(c.c_void_p)]
pam.pam_authenticate.argtypes = [c.c_void_p, c.c_int]
pam.pam_end.argtypes = [c.c_void_p, c.c_int]
CALLBACK = c.CFUNCTYPE(c.c_int, c.c_int, c.c_void_p, c.c_void_p, c.c_void_p)
@CALLBACK
def conversation(count, messages, response, data):
    return 19  # Never supplies a password or accepts a prompt.
class Conv(c.Structure):
    _fields_ = [('callback', CALLBACK), ('data', c.c_void_p)]
conv = Conv(conversation, None)

def authenticate(directory, service, name):
    handle = c.c_void_p()
    status = pam.pam_start_confdir(service.encode(), name.encode(), c.byref(conv), str(directory).encode(), c.byref(handle))
    if status: raise RuntimeError(f'pam_start_confdir: {status}')
    try: return pam.pam_authenticate(handle, 0)
    finally: pam.pam_end(handle, 0)

if __name__ == '__main__':
    import sys
    if os.geteuid() != 0 or len(sys.argv) != 2: raise SystemExit('Usage: sudo test-pam.py USER (before enrollment)')
    name = sys.argv[1]
    uid = pwd.getpwnam(name).pw_uid
    if Path(f'/var/lib/linux-hello/sudo/{uid}/config.json').exists():
        raise SystemExit('Refusing camera-dependent test: already enrolled')
    module = '/usr/lib64/security/pam_linuxhello.so'
    with tempfile.TemporaryDirectory(prefix='hello-pam-test-') as tmp:
        conf = Path(tmp)
        for service in ['sudo', 'sudo-i', 'sshd']:
            (conf / service).write_text(f'auth sufficient {module}\nauth required pam_deny.so\n')
        # Same identity restriction as real sudo, while retaining privileged PAM.
        os.setresuid(uid, 0, 0)
        for service in ['sudo', 'sudo-i', 'sshd']:
            status = authenticate(conf, service, name)
            assert status != 0, (service, status)
        (conf / 'sudo').write_text(f'auth sufficient {module}\nauth required pam_permit.so\n')
        assert authenticate(conf, 'sudo', name) == 0
        (conf / 'sudo').write_text('auth sufficient /nonexistent/linuxhello-module.so\nauth required pam_deny.so\n')
        assert authenticate(conf, 'sudo', name) != 0
        (conf / 'sudo').write_text('auth sufficient /nonexistent/linuxhello-module.so\nauth required pam_permit.so\n')
        assert authenticate(conf, 'sudo', name) == 0
        os.setresuid(0, 0, 0)
    print('PASS: missing enrollment/module cannot authenticate; fallback dispatch preserved; other PAM services ignored.')
