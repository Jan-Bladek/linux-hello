#!/usr/bin/python3
"""Test real SELinux admission replacement without cameras or production state."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile

CHILD = r'''
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/xattr.h>
#include <unistd.h>
int main(int argc, char **argv) {
    assert(argc == 2);
    char context[256] = {0};
    int current = open("/proc/self/attr/current", O_RDONLY);
    assert(current >= 0 && read(current, context, sizeof(context)-1) > 0);
    close(current);
    assert(strstr(context, ":xdm_t:"));
    assert(chdir(argv[1]) == 0);
    int fd = open(".linux-hello-admission-pending", O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW, 0600);
    assert(fd >= 0 && write(fd, "xdm-counter", 11) == 11);
    assert(fsync(fd) == 0);
    memset(context, 0, sizeof(context));
    assert(fgetxattr(fd, "security.selinux", context, sizeof(context)-1) > 0);
    assert(strstr(context, ":linuxhello_admission_t:"));
    close(fd);
    assert(rename(".linux-hello-admission-pending", ".admission.json") == 0);
    assert(unlink("protected-template") == -1 && (errno == EACCES || errno == EPERM));
    puts("PASS: xdm_t replaces only the admission fixture; protected-file unlink denied.");
    return 0;
}
'''


def main():
    if os.geteuid() != 0:
        raise RuntimeError('Root required for private namespaces and SELinux fixture labels')
    if subprocess.check_output(['/usr/sbin/getenforce'], text=True).strip() != 'Enforcing':
        raise RuntimeError('SELinux must remain Enforcing')
    namespace = os.readlink('/proc/self/ns/mnt')
    if len(sys.argv) == 1:
        subprocess.run(['/usr/bin/unshare', '--mount', '--propagation', 'private',
                        sys.executable, '-B', str(Path(__file__).absolute()), namespace], check=True)
        return
    assert len(sys.argv) == 2 and namespace != sys.argv[1]
    assert namespace != os.readlink('/proc/1/ns/mnt')
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix='hello-admission-policy-', dir='/run') as folder:
        root = Path(folder)
        (root / 'probe.c').write_text(CHILD)
        subprocess.run(['/usr/bin/cc', '-Wall', '-Wextra', '-Werror', '-O2', '-o', str(root/'probe'), str(root/'probe.c')], check=True)
        (root / 'protected-template').write_bytes(b'synthetic-only')
        (root / '.admission.json').write_bytes(b'initial-counter')
        subprocess.run(['/usr/bin/chcon', '-R', '-t', 'var_lib_t', folder], check=True)
        subprocess.run(['/usr/bin/chcon', '-t', 'linuxhello_admission_t', str(root / '.admission.json')], check=True)
        subprocess.run(['/usr/bin/chcon', '-t', 'xdm_exec_t', str(root/'probe')], check=True)
        # /run is nosuid: use a private bind with suid enabled so the test
        # exercises xdm_t policy instead of failing the nosuid transition gate.
        driver = '/usr/local/libexec/linux-hello-sudo/kde-pam-probe'
        subprocess.run(['/usr/bin/mount', '--bind', str(root/'probe'), driver], check=True)
        subprocess.run(['/usr/bin/mount', '-o', 'remount,bind,suid', driver], check=True)
        for _ in range(2):
            # Model the ordinary root/sudo writer before a display-manager writer.
            pending = root / '.linux-hello-admission-pending'
            pending.write_bytes(b'root-counter')
            assert b':linuxhello_admission_t:' in os.getxattr(pending, 'security.selinux')
            os.replace(pending, root / '.admission.json')
            subprocess.run(['/usr/bin/runcon', '-u', 'system_u', '-r', 'system_r', '-t', 'xdm_t',
                            '-l', 's0-s0:c0.c1023', driver, folder],
                           check=True, timeout=15, env={'LANG': 'C.UTF-8'})
            assert (root / 'protected-template').read_bytes() == b'synthetic-only'
    print('PASS: repeated cross-domain admission replacement; no production files or camera accessed.')


if __name__ == '__main__':
    main()
