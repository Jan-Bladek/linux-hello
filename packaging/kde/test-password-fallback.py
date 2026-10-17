#!/usr/bin/python3
"""Explicit root-only live PAM password fallback test, with guaranteed model restore."""
import os
from pathlib import Path
import pwd
import signal
import subprocess
import sys
import tempfile
import importlib.util
spec=importlib.util.spec_from_file_location('kde_manager',Path(__file__).with_name('manage-kde.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)

def main():
    if os.geteuid()!=0 or len(sys.argv) not in (2,4): raise SystemExit('Usage: sudo test-password-fallback.py USER [--service kde|hyprlock|login]')
    locker=len(sys.argv)==4 and sys.argv[2]=='--service' and sys.argv[3] in m.LOCK_SERVICES
    if len(sys.argv)==4 and not locker: raise SystemExit('Only reviewed screen-locker services are supported')
    user=sys.argv[1];uid=pwd.getpwnam(user).pw_uid
    if uid<1000: raise SystemExit('Normal account required')
    service=sys.argv[3] if locker else m.detect()
    source=Path('/etc/pam.d')/service
    if not source.exists(): source=Path('/usr/lib/pam.d')/service
    m.sudo.protected(source)
    content=m.changed(source.read_text(),True,service).encode()
    model=m.BASE/'models/face_detection_yunet_2023mar.onnx'
    pending=model.with_name('.detector-kde-fallback-test')
    m.sudo.protected(model)
    if pending.exists(): raise SystemExit('Unrestored test model exists')
    marker=m.ROOT/str(uid)/f'pam-{service}.enabled'
    prior=None
    if marker.exists():
        m.sudo.protected(marker,True);prior=marker.read_bytes()
    def interrupted(*args): raise InterruptedError('Test interrupted')
    signal.signal(signal.SIGTERM,interrupted);signal.signal(signal.SIGINT,interrupted)
    try:
        m.sudo.atomic_write(marker,b'linux-hello-login-v1\n',0o600)
        model.rename(pending)
        with tempfile.TemporaryDirectory(prefix='kde-fallback-',dir='/run' if locker else m.ROOT) as tmp:
            conf=Path(tmp)
            m.snapshot_pam(conf)
            m.sudo.atomic_write(conf/service,content,0o600)
            m.sudo.atomic_write(conf/m.layout(service)[2],m.auth_stack(service).encode(),0o600)
            if locker:
                conf.chmod(0o755)
                for path in conf.iterdir(): path.chmod(0o644)
            subprocess.run(['/usr/sbin/restorecon','-R',tmp],check=True)
            print('Detector intentionally unavailable. Enter the original account password in the hidden PAM prompt.',flush=True)
            command=['/usr/bin/runcon','-u','system_u','-r','system_r','-t','xdm_t','-l','s0-s0:c0.c1023',str(m.BASE/'kde-pam-probe'),service,user,str(conf),'--password']
            if locker:
                command=['/usr/bin/setpriv','--reuid',str(uid),'--regid',str(pwd.getpwnam(user).pw_gid),'--init-groups',str(m.BASE/'kde-pam-probe'),service,user,str(conf),'--password']
            subprocess.run(command,check=True,timeout=60)
        print('PASS: real desktop password/account/setcred stack succeeds despite model failure. No desktop session opened.')
    finally:
        if pending.exists(): pending.rename(model)
        if prior is None: marker.unlink(missing_ok=True)
        else: m.sudo.atomic_write(marker,prior,0o600)
        subprocess.run(['/usr/sbin/restorecon',str(model)],check=True)
        print('Model and login marker restored.',flush=True)
if __name__=='__main__':main()
