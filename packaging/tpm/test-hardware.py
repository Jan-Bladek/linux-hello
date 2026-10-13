#!/usr/bin/python3
"""Real TPM synthetic tests in a private mount namespace; --kde adds SELinux/self-UID checks.
Install the optional TPM policy and build the release helper before --kde.
"""
import os, pathlib, subprocess, sys, tempfile
assert os.geteuid()==0, 'Run with sudo after cargo test --no-run'
repo=pathlib.Path(__file__).resolve().parents[2]
binaries=list((repo/'target/debug/deps').glob('hello_sudo-*'))
binaries=[p for p in binaries if p.is_file() and os.access(p,os.X_OK)]
binaries=[p for p in binaries if 'tpm::tests::real_tpm_migration_and_fail_closed: test' in subprocess.check_output([str(p),'--list'],text=True)]
assert len(binaries)==1, 'Build exactly one sudo-face TPM test executable with cargo test --no-run'
current=os.readlink('/proc/self/ns/mnt')
if len(sys.argv)==1 or sys.argv[1:] == ['--kde']:
    subprocess.run(['/usr/bin/unshare','--mount','--propagation','private',sys.executable,__file__,current,*sys.argv[1:]],check=True)
else:
    assert current!=sys.argv[1] and current!=os.readlink('/proc/1/ns/mnt')
    os.umask(0o077)
    with tempfile.TemporaryDirectory(dir='/run',prefix='hello-tpm-test-') as tmp:
        state=pathlib.Path(tmp)
        (state/'.tpm-test-only').write_bytes(b'isolated-synthetic-fixture\n')
        subprocess.run(['/usr/bin/mount','--bind',str(state),'/var/lib/linux-hello/sudo'],check=True)
        try:
            subprocess.run([str(binaries[0]),'real_tpm_migration_and_fail_closed','--ignored','--test-threads=1'],check=True)
            if '--kde' in sys.argv[2:]:
                base=pathlib.Path('/usr/local/libexec/linux-hello-sudo')
                with tempfile.TemporaryDirectory(dir=base,prefix='.tpm-probe-') as install:
                    install=pathlib.Path(install); install.chmod(0o755)
                    helper=install/'helper'; probe=install/'probe'
                    import shutil
                    shutil.copyfile(repo/'target/release/hello-sudo',helper); helper.chmod(0o755)
                    source=state/'probe.c'
                    source.write_text('#include <unistd.h>\nint main(void){execl("/usr/local/libexec/linux-hello-sudo/hello-sudo","hello-sudo","tpm-status","1000",(char*)0);return 1;}\n')
                    subprocess.run(['/usr/bin/cc','-Wall','-Wextra','-Werror','-O2',str(source),'-o',str(probe)],check=True)
                    subprocess.run(['/usr/bin/chcon','-R','-t','var_lib_t',str(state)],check=True)
                    subprocess.run(['/usr/bin/chcon','-t','usr_t',str(install),str(helper)],check=True)
                    subprocess.run(['/usr/bin/chcon','-t','xdm_exec_t',str(probe)],check=True)
                    subprocess.run(['/usr/bin/mount','--bind',str(helper),str(base/'hello-sudo')],check=True)
                    try:
                        result=subprocess.run(['/usr/bin/runcon','-u','system_u','-r','system_r','-t','xdm_t','-l','s0-s0:c0.c1023',str(probe)])
                        print('XDM_T_TPM_KEY_ONLY_EXIT=',result.returncode)
                        assert result.returncode==0
                        probe.chmod(0o4755)
                        result=subprocess.run(['/usr/bin/setpriv','--reuid=1000','--regid=1000','--init-groups',str(probe)])
                        print('OWN_UID_TPM_KEY_ONLY_EXIT=',result.returncode)
                        assert result.returncode==0
                    finally:
                        subprocess.run(['/usr/bin/umount','-l',str(base/'hello-sudo')],check=True)
        finally:
            subprocess.run(['/usr/bin/umount','/var/lib/linux-hello/sudo'],check=True)
