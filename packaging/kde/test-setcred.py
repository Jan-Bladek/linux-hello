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
import importlib.util
import ctypes as c
import shutil
sys.dont_write_bytecode=True
spec=importlib.util.spec_from_file_location('kde_management',Path(__file__).with_name('manage-kde.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)

def sudo_authenticate(conf, user):
    """Actual PAM dispatch in this private namespace, no sudo command/session."""
    pam=c.CDLL('libpam.so.0')
    pam.pam_start_confdir.argtypes=[c.c_char_p,c.c_char_p,c.c_void_p,c.c_char_p,c.POINTER(c.c_void_p)]
    pam.pam_set_item.argtypes=[c.c_void_p,c.c_int,c.c_void_p]
    pam.pam_authenticate.argtypes=[c.c_void_p,c.c_int]
    pam.pam_end.argtypes=[c.c_void_p,c.c_int]
    callback=c.CFUNCTYPE(c.c_int,c.c_int,c.c_void_p,c.c_void_p,c.c_void_p)
    @callback
    def conversation(*_): return 19
    class Conv(c.Structure): _fields_=[('callback',callback),('data',c.c_void_p)]
    conv=Conv(conversation,None);handle=c.c_void_p();name=user.encode()
    assert pam.pam_start_confdir(b'sudo',name,c.byref(conv),str(conf).encode(),c.byref(handle))==0
    try:
        assert pam.pam_set_item(handle,8,c.cast(c.c_char_p(name),c.c_void_p))==0
        return pam.pam_authenticate(handle,0x8000) # informational prompt suppressed
    finally: pam.pam_end(handle,0)

BASE=Path('/usr/local/libexec/linux-hello-sudo')
ROOT=Path('/var/lib/linux-hello/sudo')

def main():
    if os.geteuid()!=0: raise SystemExit('root only')
    if sys.argv[1:2]!=['--isolated'] and len(sys.argv) in (2,4):
        original=os.readlink('/proc/self/ns/mnt')
        subprocess.run(['/usr/bin/unshare','--mount','--propagation','private',sys.executable,str(Path(__file__).resolve()),'--isolated',*sys.argv[1:],original],check=True)
        return
    if len(sys.argv) not in (4,6) or sys.argv[1]!='--isolated': raise SystemExit('Usage: sudo test-setcred.py USER [CURRENT_PAM_MODULE CURRENT_LOCK_LAUNCHER]')
    # Refuse mounting any fixture over a production path in the caller's namespace.
    current=os.readlink('/proc/self/ns/mnt')
    if current==sys.argv[-1] or current==os.readlink('/proc/1/ns/mnt'):
        raise SystemExit('PRIVATE mount namespace required')
    user=sys.argv[2];uid=pwd.getpwnam(user).pw_uid
    if uid<1000: raise SystemExit('Normal account required')
    os.umask(0o077)
    # Everything staged in /run is root-private. No actual templates are copied.
    with tempfile.TemporaryDirectory(prefix='.hello-setcred-binaries-',dir='/usr/local/libexec') as binfolder, tempfile.TemporaryDirectory(prefix='hello-setcred-',dir='/run') as tmp:
        tmp=Path(tmp);state=tmp/'state';state.mkdir();profile=state/str(uid);profile.mkdir()
        (profile/'config.json').write_text('{}')
        (profile/'pam-plasmalogin.enabled').write_text('linux-hello-login-v1\n')
        (profile/'pam-kde.enabled').write_text('linux-hello-login-v1\n')
        conf=tmp/'conf';conf.mkdir()
        (conf/'plasmalogin').write_text('auth sufficient /usr/lib64/security/pam_linuxhello.so\nauth required pam_deny.so\naccount required pam_permit.so\n')
        fixture=tmp/'fixture.c'
        fixture.write_text('#include <unistd.h>\n#include <string.h>\n#ifndef OUTCOME\n#define OUTCOME 1\n#endif\nint main(int n,char **v){return geteuid()==0&&n==4&&(!strcmp(v[1],"verify-service")||!strcmp(v[1],"diagnose-service"))&&(!strcmp(v[3],"sudo")||!strcmp(v[3],"sudo-i")||!strcmp(v[3],"kde")||!strcmp(v[3],"plasmalogin")||!strcmp(v[3],"sddm"))&&!strcmp(v[2],EXPECTED_UID)?OUTCOME:1;}\n')
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
        binary_mounts=[]
        try:
            if len(sys.argv)==6:
                # Root-private staging on the normal root filesystem: /run is
                # nosuid. The actual production files are only overlaid in this
                # verified private namespace, never truncated or replaced.
                for source,target,name,mode,label in [
                    (sys.argv[3],Path('/usr/lib64/security/pam_linuxhello.so'),'pam.so',0o644,'lib_t'),
                    (sys.argv[4],BASE/'hello-lock-verify','launcher',0o4755,'usr_t')]:
                    staged=Path(binfolder)/name;shutil.copyfile(source,staged);staged.chmod(mode)
                    subprocess.run(['/usr/bin/chcon','-t',label,str(staged)],check=True)
                    subprocess.run(['/usr/bin/mount','--bind',str(staged),str(target)],check=True);binary_mounts.append(target)
            subprocess.run(['/usr/bin/mount','--bind',str(good),str(BASE/'hello-sudo')],check=True)
            (conf/'sudo').write_text('auth sufficient /usr/lib64/security/pam_linuxhello.so\nauth required pam_deny.so\n')
            assert sudo_authenticate(conf,user)==0,'legacy enrolled own account remains enabled'
            (profile/'pam-sudo.disabled').write_text('linux-hello-sudo-disabled-v1\n')
            assert sudo_authenticate(conf,user)!=0,'disabled account cannot use the successful helper'
            (profile/'pam-sudo.disabled').unlink()
            (profile/'pam-sudo.disabled').symlink_to(profile/'absent-disable-target')
            assert sudo_authenticate(conf,user)!=0,'broken disable marker must fail closed'
            (profile/'pam-sudo.disabled').unlink()
            other=state/str(uid+1);other.mkdir()
            (other/'pam-sudo.disabled').write_text('linux-hello-sudo-disabled-v1\n')
            assert sudo_authenticate(conf,user)==0,'another account disable must not disable this account'
            assert subprocess.run(command,timeout=10).returncode==0,'successful verifier must complete auth/account/setcred'
            (conf/'plasmalogin').write_text('auth sufficient /usr/lib64/security/pam_linuxhello.so diagnostic\nauth required pam_deny.so\naccount required pam_permit.so\n')
            assert subprocess.run(command,timeout=10).returncode==0,'diagnostics must preserve auth/account/setcred'
            # Reproduce the actual parent stack including optional wallet hooks.
            # Numeric success jumps previously left no positive impression and
            # failed here with PERM_DENIED6 despite a successful face helper.
            source=Path('/etc/pam.d/plasmalogin')
            if not source.exists(): source=Path('/usr/lib/pam.d/plasmalogin')
            m.snapshot_pam(conf)
            original=source.read_text()
            if m.MARKER not in original:
                password_line=next(line for line in original.splitlines(keepends=True) if line.split()==['auth','substack','password-auth'])
                legacy=original.replace(password_line,m.MARKER+m.LEGACY_LINE+password_line,1)
                (conf/'plasmalogin').write_text(legacy)
                subprocess.run(['/usr/bin/chcon','-R','-t','var_lib_t',str(conf)],check=True)
                old=subprocess.run(command,timeout=10,capture_output=True,text=True)
                assert old.returncode!=0 and 'PAM_AUTHENTICATE=6' in old.stderr,'old numeric jump should reproduce the missing positive auth result'
            full=m.changed(source.read_text(),True)
            (conf/'plasmalogin').write_text(full)
            locker=Path('/etc/pam.d/kde').read_text()
            (conf/'kde').write_text(m.changed(locker,True))
            (conf/m.SUBSTACK).write_text(m.AUTH_STACK)
            conf.chmod(0o755)
            for path in conf.iterdir(): path.chmod(0o644)
            gid=pwd.getpwnam(user).pw_gid
            lock_command=['/usr/bin/setpriv','--reuid',str(uid),'--regid',str(gid),'--init-groups',str(probe),'kde',user,str(conf)]
            assert subprocess.run(lock_command,timeout=10).returncode==0,'unprivileged locker must verify only its own account via the setuid launcher'
            launch=['/usr/bin/setpriv','--reuid',str(uid),'--regid',str(gid),'--init-groups',str(BASE/'hello-lock-verify')]
            assert subprocess.run(launch+['verify',str(uid)],timeout=10).returncode!=0,'launcher must reject all supplied commands and target IDs'
            assert subprocess.run([str(BASE/'hello-lock-verify')],timeout=10).returncode!=0,'root/system real UID must not use the launcher'
            lock_marker=profile/'pam-kde.enabled'
            lock_marker.unlink()
            assert subprocess.run(lock_command,timeout=10).returncode!=0,'missing locker opt-in must reject'
            lock_marker.write_text('corrupt\n')
            assert subprocess.run(lock_command,timeout=10).returncode!=0,'corrupt locker opt-in must reject'
            lock_marker.write_text('linux-hello-login-v1\n')
            (conf/m.SUBSTACK).write_text(m.AUTH_STACK)
            subprocess.run(['/usr/bin/chcon','-R','-t','var_lib_t',str(conf)],check=True)
            assert subprocess.run(command,timeout=10).returncode==0,'matched face must complete the real KDE parent stack'
            # A previous required denial must not be overridden by biometrics.
            (conf/'plasmalogin').write_text('auth required pam_deny.so\n'+full)
            assert subprocess.run(command,timeout=10).returncode!=0,'earlier required failure must survive biometric success'
            (conf/'plasmalogin').write_text(full)
            subprocess.run(['/usr/bin/umount',str(BASE/'hello-sudo')],check=True)
            subprocess.run(['/usr/bin/mount','--bind',str(bad),str(BASE/'hello-sudo')],check=True)
            assert subprocess.run(lock_command,timeout=10).returncode!=0,'unprivileged locker mismatch must reject'
            assert subprocess.run(command,timeout=10).returncode!=0,'rejected verifier must not authenticate'
            (conf/m.SUBSTACK).write_bytes(m.diagnostic_stack(m.AUTH_STACK.encode()))
            assert subprocess.run(command,timeout=10).returncode!=0,'diagnostic rejection must retain the password fallback'
            (conf/'plasmalogin').write_text('auth sufficient /usr/lib64/security/pam_linuxhello.so\nauth required pam_deny.so\naccount required pam_permit.so\n')
            assert subprocess.run(command,timeout=10).returncode!=0,'normal rejection must not authenticate either'
        finally:
            subprocess.run(['/usr/bin/umount',str(BASE/'hello-sudo')],check=True)
            subprocess.run(['/usr/bin/umount',str(ROOT)],check=True)
            for target in reversed(binary_mounts):subprocess.run(['/usr/bin/umount',str(target)],check=True)
    print('PASS: real PAM auth/account/setcred dispatch for simulated match; rejection fails closed. Production namespace unchanged.')
if __name__=='__main__':main()
