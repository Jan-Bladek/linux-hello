#!/usr/bin/python3
"""Root-only negative isolated KDE PAM tests; never open a session or execute sudo."""
import os
from pathlib import Path
import pwd
import subprocess
import tempfile
import importlib.util
spec=importlib.util.spec_from_file_location('kde_manager',Path(__file__).with_name('manage-kde.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)

def invoke(conf,service,user):
    return subprocess.run(['/usr/bin/runcon','-u','system_u','-r','system_r','-t','xdm_t','-l','s0-s0:c0.c1023',str(m.BASE/'kde-pam-probe'),service,user,str(conf)],timeout=40).returncode

def main():
    import sys
    if os.geteuid()!=0 or len(sys.argv)!=2: raise SystemExit('Usage: sudo test-login.py USER (before KDE enrollment marker)')
    name=sys.argv[1];uid=pwd.getpwnam(name).pw_uid
    marker=m.ROOT/str(uid)/'pam-plasmalogin.enabled'
    if marker.exists(): raise SystemExit('Already enabled; refusing to alter the marker')
    with tempfile.TemporaryDirectory(prefix='kde-negative-',dir=m.ROOT) as tmp:
        conf=Path(tmp)
        deny=m.LINE+'auth required pam_deny.so\naccount required pam_permit.so\n'
        permit=m.LINE+'auth required pam_permit.so\naccount required pam_permit.so\n'
        for service in ['plasmalogin','sddm','plasmalogin-greeter','plasmalogin-autologin']:
            m.sudo.atomic_write(conf/service,deny.encode(),0o600)
        subprocess.run(['/usr/sbin/restorecon','-R',tmp],check=True)
        for service in ['plasmalogin','sddm','plasmalogin-greeter','plasmalogin-autologin']:
            assert invoke(conf,service,name)!=0,service
        m.sudo.atomic_write(conf/'plasmalogin',permit.encode(),0o600)
        assert invoke(conf,'plasmalogin',name)==0,'fallback dispatch'
        m.sudo.atomic_write(conf/'plasmalogin',deny.encode(),0o600)
        m.sudo.atomic_write(marker,b'corrupt opt-in\n',0o600)
        try:
            assert invoke(conf,'plasmalogin',name)!=0,'corrupt marker'
        finally: marker.unlink()
        assert invoke(conf,'plasmalogin','root')!=0,'root account'
    print('PASS: xdm_t missing/corrupt marker, unsupported services and system account reject; fallback dispatch retained.')
if __name__=='__main__':main()
