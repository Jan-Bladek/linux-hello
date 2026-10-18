"""Installer crash points with synthetic files, mocked policy tools, no root."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SOURCE=Path(__file__).parents[1]/'packaging/installer/install-root.py'
spec=importlib.util.spec_from_file_location('recovery_installer',SOURCE)
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)

CHILD='''import importlib.util,os,sys
from pathlib import Path
s=importlib.util.spec_from_file_location('installer',sys.argv[1]);m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
f=Path(sys.argv[2]);m.BASE=f/'base';m.STATE=f/'state';m.PAM=f/'pam.so';m.BIN=f/'bin';m.DESKTOP=f/'apps/hello.desktop'
m.protected=lambda p,private=False:p.lstat() if p.exists() else None
m.directory=lambda p,mode:p.mkdir(mode=mode,parents=True,exist_ok=True)
m.run=lambda args:None
m.subprocess.check_output=lambda *args,**kwargs:''
original=m.atomic
kill=Path(sys.argv[3])
def atomic(p,data,mode):
 original(p,data,mode)
 if p==kill:os._exit(79)
m.atomic=atomic
m.install({'version':1},{'hello-sudo':b'new-helper','pam_linuxhello.so':b'new-pam'})
'''


class InstallerRecoveryTests(unittest.TestCase):
    def test_process_death_at_bundle_steps_keeps_anchor_closed_until_recovery(self):
        for point in ['state/.auth-epoch','base/hello-sudo','pam.so','base/install-manifest.json','state/.anchor']:
            with self.subTest(point=point),tempfile.TemporaryDirectory() as tmp:
                f=Path(tmp);base=f/'base';state=f/'state';base.mkdir();state.mkdir(mode=0o700)
                (base/'hello-sudo').write_bytes(b'old-helper');(f/'pam.so').write_bytes(b'old-pam')
                (state/'.anchor').write_bytes(b'');(state/'.anchor').chmod(0o600)
                p=subprocess.run([sys.executable,'-B','-c',CHILD,str(SOURCE),tmp,str(f/point)],check=False,stdout=subprocess.DEVNULL)
                self.assertEqual(p.returncode,79)
                self.assertTrue((state/'.install-update/index.json').exists())
                if point!='state/.anchor':self.assertFalse((state/'.anchor').exists())
                with patch.multiple(m,BASE=base,STATE=state,PAM=f/'pam.so',BIN=f/'bin',DESKTOP=f/'apps/hello.desktop'),\
                     patch.object(m,'protected',side_effect=lambda p,private=False:p.lstat() if p.exists() else None),\
                     patch.object(m,'run'),patch.object(m.subprocess,'check_output',return_value=''):
                    with m.attempt_lock():
                        deferred=[];journal=m.installation_journal(m.recovery_targets(),deferred)
                        m.recover_installation(journal,deferred)
                self.assertEqual((base/'hello-sudo').read_bytes(),b'old-helper');self.assertEqual((f/'pam.so').read_bytes(),b'old-pam')
                self.assertTrue((state/'.anchor').exists());self.assertFalse((state/'.install-update').exists())
                self.assertFalse((f/'bin/hello').exists())
