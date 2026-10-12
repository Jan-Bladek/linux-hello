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
    if os.geteuid()!=0 or len(sys.argv)!=2: raise SystemExit('Usage: sudo test-password-fallback.py USER')
    user=sys.argv[1];uid=pwd.getpwnam(user).pw_uid
    if uid<1000: raise SystemExit('Normal account required')
    service=m.detect()
    source=Path('/etc/pam.d')/service
    if not source.exists(): source=Path('/usr/lib/pam.d')/service
    m.sudo.protected(source)
    content=m.changed(source.read_text(),True).encode()
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
        with tempfile.TemporaryDirectory(prefix='kde-fallback-',dir=m.ROOT) as tmp:
            conf=Path(tmp)
            m.snapshot_pam(conf)
            m.sudo.atomic_write(conf/service,content,0o600)
            subprocess.run(['/usr/sbin/restorecon','-R',tmp],check=True)
            print('Detector intentionally unavailable. Enter the original account password in the hidden PAM prompt.',flush=True)
            subprocess.run(['/usr/bin/runcon','-u','system_u','-r','system_r','-t','xdm_t','-l','s0-s0:c0.c1023',str(m.BASE/'kde-pam-probe'),service,user,str(conf),'--password'],check=True,timeout=60)
        print('PASS: real KDE password/account/setcred stack succeeds despite model failure. No desktop session opened.')
    finally:
        if pending.exists(): pending.rename(model)
        if prior is None: marker.unlink(missing_ok=True)
        else: m.sudo.atomic_write(marker,prior,0o600)
        subprocess.run(['/usr/sbin/restorecon',str(model)],check=True)
        print('Model and login marker restored.',flush=True)
if __name__=='__main__':main()
