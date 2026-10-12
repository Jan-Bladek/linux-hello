#!/usr/bin/python3
"""PAM dispatch regression with simulated verifier exits in a PRIVATE mount namespace.
No camera/biometric assertion, OS session or production helper/reference change.
"""
import os
from pathlib import Path
import pwd
import subprocess
import sys
import tempfile

BASE=Path('/usr/local/libexec/linux-hello-sudo')
ROOT=Path('/var/lib/linux-hello/sudo')

def main():
    if os.geteuid()!=0: raise SystemExit('root only')
    if len(sys.argv)==2:
        original=os.readlink('/proc/self/ns/mnt')
        subprocess.run(['/usr/bin/unshare','--mount','--propagation','private',sys.executable,str(Path(__file__).resolve()),'--isolated',sys.argv[1],original],check=True)
        return
    if len(sys.argv)!=4 or sys.argv[1]!='--isolated': raise SystemExit('Usage: sudo test-setcred.py USER')
    # Refuse mounting any fixture over a production path in the caller's namespace.
    current=os.readlink('/proc/self/ns/mnt')
    if current==sys.argv[3] or current==os.readlink('/proc/1/ns/mnt'):
        raise SystemExit('PRIVATE mount namespace required')
    user=sys.argv[2];uid=pwd.getpwnam(user).pw_uid
    if uid<1000: raise SystemExit('Normal account required')
    os.umask(0o077)
    # Everything staged in /run is root-private. No actual templates are copied.
    with tempfile.TemporaryDirectory(prefix='hello-setcred-',dir='/run') as tmp:
        tmp=Path(tmp);state=tmp/'state';state.mkdir();profile=state/str(uid);profile.mkdir()
        (profile/'config.json').write_text('{}')
        (profile/'pam-plasmalogin.enabled').write_text('linux-hello-login-v1\n')
        conf=tmp/'conf';conf.mkdir()
        (conf/'plasmalogin').write_text('auth sufficient /usr/lib64/security/pam_linuxhello.so\nauth required pam_deny.so\naccount required pam_permit.so\n')
        fixture=tmp/'fixture.c'
        fixture.write_text('#include <unistd.h>\n#include <string.h>\n#ifndef OUTCOME\n#define OUTCOME 1\n#endif\nint main(int n,char **v){return geteuid()==0&&n==3&&!strcmp(v[1],"verify")&&!strcmp(v[2],EXPECTED_UID)?OUTCOME:1;}\n')
        good=tmp/'matched';bad=tmp/'rejected'
        for file,exitcode in [(good,0),(bad,1)]:
            subprocess.run(['/usr/bin/cc','-Wall','-Wextra','-Werror','-O2',f'-DOUTCOME={exitcode}',f'-DEXPECTED_UID="{uid}"','-o',str(file),str(fixture)],check=True)
            file.chmod(0o755)
            subprocess.run(['/usr/bin/chcon','-t','usr_t',str(file)],check=True)
        # Give the private fixtures the same read-only state label as production.
        subprocess.run(['/usr/bin/chcon','-R','-t','var_lib_t',str(state),str(conf)],check=True)
        # Execute the installed probe on the normal root filesystem: /run is
        # nosuid and would forbid runcon's SELinux domain transition.
        probe=BASE/'kde-pam-probe'
        # xdm_t must traverse only this private diagnostic directory.
        tmp.chmod(0o755)
        subprocess.run(['/usr/bin/chcon','-t','var_lib_t',str(tmp)],check=True)
        subprocess.run(['/usr/bin/mount','--bind',str(state),str(ROOT)],check=True)
        command=['/usr/bin/runcon','-u','system_u','-r','system_r','-t','xdm_t','-l','s0-s0:c0.c1023',str(probe),'plasmalogin',user,str(conf)]
        try:
            subprocess.run(['/usr/bin/mount','--bind',str(good),str(BASE/'hello-sudo')],check=True)
            assert subprocess.run(command,timeout=10).returncode==0,'successful verifier must complete auth/account/setcred'
            subprocess.run(['/usr/bin/umount',str(BASE/'hello-sudo')],check=True)
            subprocess.run(['/usr/bin/mount','--bind',str(bad),str(BASE/'hello-sudo')],check=True)
            assert subprocess.run(command,timeout=10).returncode!=0,'rejected verifier must not authenticate'
        finally:
            subprocess.run(['/usr/bin/umount',str(BASE/'hello-sudo')],check=True)
            subprocess.run(['/usr/bin/umount',str(ROOT)],check=True)
    print('PASS: real PAM auth/account/setcred dispatch for simulated match; rejection fails closed. Production namespace unchanged.')
if __name__=='__main__':main()
