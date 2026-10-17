#!/usr/bin/python3
"""Root-only desktop opt-in: display managers and self-bound lockers."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import time
import sys

# The installed sibling is root-owned; never import management code from $PWD.
import importlib.util
SIBLING = Path(__file__).resolve().with_name('manage.py')
if not SIBLING.exists():
    SIBLING = Path(__file__).resolve().parents[1] / 'sudo/manage.py'
spec = importlib.util.spec_from_file_location('hello_sudo_management', SIBLING)
sudo = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sudo)
ROOT = sudo.ROOT
BASE = Path('/usr/local/libexec/linux-hello-sudo')
MARKER = '# Linux Hello KDE login experimental integration\n'
LEGACY_LINE = 'auth        [success=1 default=ignore] /usr/lib64/security/pam_linuxhello.so\n'
SUBSTACK = 'linuxhello-kde-auth'
LINE = f'auth        substack      {SUBSTACK}\n'
ORIGINAL = '# Linux Hello original password line: '
LEGACY_AUTH_STACK = 'auth sufficient /usr/lib64/security/pam_linuxhello.so\nauth substack password-auth\n'
AUTH_STACK = sudo.PREAUTH + 'auth sufficient /usr/lib64/security/pam_linuxhello.so\nauth substack password-auth\n'
SERVICES = ('plasmalogin', 'sddm')
LOCK_SERVICES = ('kde', 'hyprlock', 'login')
MANAGED_SERVICES = SERVICES + LOCK_SERVICES
DESKTOP_MARKER = '# Linux Hello desktop unlock experimental integration\n'
LOCKER_DEFAULT = '#%PAM-1.0\nauth substack system-auth\naccount required pam_nologin.so\naccount include system-auth\npassword include system-auth\nsession include system-auth\n'

def layout(service):
    if service not in MANAGED_SERVICES: raise RuntimeError('Unsupported desktop PAM service')
    if service in ('login', 'hyprlock'):
        substack = f'linuxhello-{service}-auth'
        return DESKTOP_MARKER, f'auth        substack      {substack}\n', substack, 'system-auth'
    return MARKER, LINE, SUBSTACK, 'password-auth'

def auth_stack(service):
    return sudo.PREAUTH + 'auth sufficient /usr/lib64/security/pam_linuxhello.so\nauth substack ' + layout(service)[3] + '\n'

class ActivationError(RuntimeError):
    """Expected, fail-closed live-test failure; no Python traceback needed."""

def run_probe(command, stage):
    print(f'Check {stage}/2. Look straight ahead with both eyes visible; no head turn is needed.', flush=True)
    for remaining in (3, 2, 1):
        print(f'Scanning in {remaining}...', flush=True)
        time.sleep(1)
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=40, env={'LANG':'C.UTF-8'})
    except subprocess.TimeoutExpired as error:
        raise ActivationError(f'Test {stage}/2: verification timed out. Sign-in PAM was not changed.') from error
    report_probe(result.stdout + result.stderr)
    if result.returncode:
        raise ActivationError(f'Test {stage}/2 rejected. Sign-in PAM was not changed; password fallback remains available. See the details above.')

def report_probe(output):
    """Display bounded metadata from this exact PAM invocation, never biometrics."""
    reasons = {
        'landmark_shape_mismatch': 'face geometry does not match the reference',
        'centroid_similarity_below_threshold': 'centroid similarity is below the threshold',
        'exemplar_similarity_below_threshold': 'saved-sample similarity is below the threshold',
    }
    for line in output.splitlines():
        if line.startswith('PAM_'):
            print(line, flush=True)
            continue
        try:
            data = json.loads(line)
        except (ValueError, TypeError):
            if line.startswith('Linux Hello: '): print(line[:1024], flush=True)
            continue
        if not isinstance(data, dict): continue
        if 'progress' in data:
            print(f"Frames {data.get('accepted_frames',0)}/3: {str(data['progress'])[:512]}", flush=True)
        if 'outcome' not in data: continue
        score, threshold = data.get('similarity'), data.get('threshold')
        if isinstance(score, (int,float)) and isinstance(threshold, (int,float)):
            print(f'Similarity: {score:.3f}; required: {threshold:.3f}.', flush=True)
        reason = data.get('failure_reason')
        if reason: print('Reason: ' + reasons.get(str(reason), str(reason)[:1024]), flush=True)
        if not data.get('cleanup_ok',False): print('Camera cleanup failed; result rejected.', flush=True)

def diagnostic_stack(content):
    """Enable metadata only in the private root-owned test, never production PAM."""
    text = content.decode('utf-8')
    line = 'auth sufficient /usr/lib64/security/pam_linuxhello.so\n'
    if text.count(line) != 1: raise RuntimeError('Missing unique Linux Hello test line')
    return text.replace(line, line.rstrip('\n') + ' diagnostic\n', 1).encode()

def changed(original, enable, service="kde"):
    marker, line, substack, password_stack = layout(service)
    if enable: sudo.auth_lines(original)  # reject unsupported logical-record syntax
    # Migrate only the exact old managed jump block; preserve all other lines.
    if marker + LEGACY_LINE in original:
        if original.count(marker) != 1 or original.count(LEGACY_LINE) != 1:
            raise RuntimeError('Malformed legacy Linux Hello KDE PAM block')
        original = original.replace(marker + LEGACY_LINE, '', 1)
    if original.count(marker) != original.count(line) or original.count(marker) > 1:
        raise RuntimeError('Malformed Linux Hello KDE PAM block')
    if marker in original:
        lines=original.splitlines(keepends=True)
        i=lines.index(marker)
        if i+2>=len(lines) or lines[i+1]!=line or not lines[i+2].startswith(ORIGINAL) or original.count(ORIGINAL)!=1:
            raise RuntimeError('Malformed Linux Hello KDE PAM block')
        password=lines[i+2][len(ORIGINAL):]
        if password.split()!=['auth','substack',password_stack]:
            raise RuntimeError('Malformed saved password substack')
        if enable: return original
        lines[i:i+3]=[password]
        return ''.join(lines)
    if ORIGINAL in original or substack in original or LEGACY_LINE in original:
        raise RuntimeError('Unmanaged Linux Hello KDE PAM configuration')
    if not enable: return original
    lines = original.splitlines(keepends=True)
    indexes = [i for i,s in enumerate(lines) if s.split() == ['auth','substack',password_stack]]
    if len(indexes) != 1: raise RuntimeError('Unsupported desktop PAM stack; expected one reviewed password substack')
    # Sufficient records success and ends only this substack; wallet/postlogin
    # and earlier required controls stay in the parent stack. Failed biometrics
    # execute the unchanged password-auth substack. A numeric jump alone did
    # not record a positive authentication result (PAM_PERM_DENIED6).
    i=indexes[0]
    if not lines[i].endswith('\n'): raise RuntimeError('Password substack must end with newline')
    lines[i]=marker + line + ORIGINAL + lines[i]
    return ''.join(lines)

def detect():
    fragment = subprocess.check_output(['/usr/bin/systemctl','show','display-manager.service','--property=FragmentPath','--value'], text=True).strip()
    name = Path(fragment).stem
    if name not in SERVICES: raise RuntimeError(f'Unsupported display manager: {name}')
    return name

def snapshot_pam(conf):
    """Include/substack paths resolve inside pam_start_confdir, not /etc/pam.d.
    Copy trusted vendor defaults then administrator/authselect overrides so the
    isolated test executes exactly the host's included password/account stack.
    """
    for folder in [Path('/usr/lib/pam.d'),Path('/etc/pam.d')]:
        if not folder.exists(): continue
        for source in folder.iterdir():
            if not source.is_file(): continue
            resolved=source.resolve(strict=True)
            sudo.protected(resolved)
            sudo.atomic_write(conf/source.name,resolved.read_bytes(),0o600)

def probe(user, content, service=None):
    driver = BASE / 'kde-pam-probe'
    sudo.protected(driver)
    service = service or detect()
    with tempfile.TemporaryDirectory(prefix='kde-pam-test-', dir='/run' if service in LOCK_SERVICES else ROOT) as folder:
        conf = Path(folder)
        snapshot_pam(conf)
        import pwd
        account = pwd.getpwnam(user)
        profile = ROOT / str(account.pw_uid)
        shadow = conf / 'profile'
        shadow.mkdir(mode=0o700)
        for name in ('config.json', 'template.lht', 'key', 'key.tpm'):
            source = profile / name
            if source.exists() or source.is_symlink():
                meta = sudo.protected(source, True)
                if meta.st_size > 1024 * 1024: raise RuntimeError('Profile exceeds test size limit')
                sudo.atomic_write(shadow / name, source.read_bytes(), 0o600)
        sudo.atomic_write(shadow / 'setup.lock', b'', 0o600)
        sudo.atomic_write(shadow / f'pam-{service}.enabled', b'linux-hello-login-v1\n', 0o600)
        command=['/usr/bin/runcon','-u','system_u','-r','system_r','-t','xdm_t','-l','s0-s0:c0.c1023',str(driver),service,user,str(conf)]
        if service in LOCK_SERVICES:
            import pwd
            account=pwd.getpwnam(user)
            # Locker PAM is unprivileged, unlike display-manager PAM. Only the
            # non-biometric test configuration is public; state stays private.
            conf.chmod(0o755)
            command=['/usr/bin/setpriv','--reuid',str(account.pw_uid),'--regid',str(account.pw_gid),'--init-groups',str(driver),service,user,str(conf)]
        # Mounting happens before entering xdm_t/dropping UID; neither probe
        # can opt the production profile in. The child namespace dies with it.
        binding = "import subprocess,sys,os; subprocess.run(['/usr/bin/mount','--bind',sys.argv[1],sys.argv[2]],check=True); subprocess.run(['/usr/bin/mount','--bind',sys.argv[3],sys.argv[4]],check=True); subprocess.run(['/usr/bin/mount','--bind',sys.argv[5],'/etc/pam.d'],check=True); os.execv(sys.argv[6],sys.argv[6:])"
        command = ['/usr/bin/unshare', '--mount', '--propagation', 'private',
                   '/usr/bin/python3', '-c', binding, str(shadow), str(profile), str(shadow / 'setup.lock'), str(ROOT / '.setup.lock'), str(conf), *command]
        mode=0o644 if service in LOCK_SERVICES else 0o600
        # First prove that this adapter itself matched, independently of any
        # pre-existing permit/other credential module in the vendor stack.
        strict=sudo.PREAUTH + 'auth sufficient /usr/lib64/security/pam_linuxhello.so diagnostic\nauth required pam_deny.so\naccount required pam_permit.so\n'
        sudo.atomic_write(conf / service, strict.encode(), mode)
        if service in LOCK_SERVICES:
            for path in conf.iterdir():
                if path.is_file(): path.chmod(0o644)
        subprocess.run(['/usr/sbin/restorecon','-R',str(conf)], check=True)
        run_probe(command,1)
        sudo.atomic_write(conf / service, content, mode)
        sudo.atomic_write(conf / layout(service)[2], diagnostic_stack(auth_stack(service).encode()), mode)
        subprocess.run(['/usr/sbin/restorecon','-R',str(conf)], check=True)
        # Identical SELinux domain to the real Plasma/SDDM root helper. Probe
        # authenticates/checks account/credentials but opens no desktop session.
        run_probe(command,2)

def main():
    p = argparse.ArgumentParser()
    p.add_argument('action',choices=['enable','disable','status'])
    p.add_argument('--user',required=True)
    p.add_argument('--service',choices=MANAGED_SERVICES)
    args=p.parse_args()
    if os.geteuid()!=0: p.error('Run with sudo')
    import pwd
    uid=pwd.getpwnam(args.user).pw_uid
    if uid<1000: p.error('Normal user account required')
    service=args.service or detect()
    if args.action=='enable' and service not in LOCK_SERVICES and service!=detect(): raise RuntimeError('Service is not the active display manager')
    pam=Path('/etc/pam.d') / service
    vendor=Path('/usr/lib/pam.d') / service
    override=pam.exists() or pam.is_symlink()
    source=pam if override else vendor
    created_service = service == 'hyprlock' and not (source.exists() or source.is_symlink())
    if created_service:
        meta = None
        original = LOCKER_DEFAULT.encode()
    else:
        meta=sudo.protected(source)
        original=source.read_bytes()
    mode = 0o644 if meta is None else stat.S_IMODE(meta.st_mode)
    managed_marker, managed_line, companion_name, password_stack = layout(service)
    text=original.decode('utf-8')
    marker=ROOT / str(uid) / f'pam-{service}.enabled'
    if args.action=='status':
        enabled=managed_marker+managed_line in text and marker.exists()
        print(f'{service}: {"enabled" if enabled else "disabled"} for {args.user}')
        return
    sudo.bump_epoch()
    policy = sudo.check_password_policy(password_stack) if args.action == 'enable' else None
    updated=changed(text,args.action=='enable',service).encode()
    sudo.protected(ROOT / '.anchor')
    record=ROOT / f'kde-{service}-original.json'
    if args.action=='enable':
        companion=Path('/etc/pam.d') / companion_name
        companion_before=None
        if companion.exists() or companion.is_symlink():
            sudo.protected(companion)
            companion_before=companion.read_bytes()
            if companion_before not in ((auth_stack(service).encode(), LEGACY_AUTH_STACK.encode()) if service not in ('login','hyprlock') else (auth_stack(service).encode(),)):
                raise RuntimeError('Managed authentication substack has administrator changes')
        sudo.protected(ROOT / str(uid) / 'config.json',True)
        sudo.protected(BASE / 'hello-sudo')
        sudo.protected('/usr/lib64/security/pam_linuxhello.so')
        if marker.exists() or marker.is_symlink(): sudo.protected(marker,True)
        print('Look straight at the camera: checking sign-in PAM and SELinux.',flush=True)
        probe(args.user,updated,service)
        backup=ROOT / f'{service}-before-{hashlib.sha256(original).hexdigest()[:16]}.pam'
        if not backup.exists(): sudo.atomic_write(backup,original,0o600)
        if not record.exists():
            sudo.atomic_write(record,json.dumps({'override_existed':override,'backup':str(backup),'created_service':created_service}).encode(),0o600)
    if policy and policy[0].read_bytes()!=policy[1]: raise RuntimeError('Password policy changed during verification')
    # Preserve unexpected administrator changes made during the live probe.
    if ((not created_service and source.read_bytes()!=original) or (created_service and (source.exists() or source.is_symlink()))
            or ((pam.exists() or pam.is_symlink()) != override)):
        raise RuntimeError('KDE PAM changed during verification')
    if args.action=='enable':
        if companion.exists() or companion.is_symlink():
            sudo.protected(companion)
            if companion_before is None or companion.read_bytes()!=companion_before:
                raise RuntimeError('Authentication substack changed during verification')
        elif companion_before is not None:
            raise RuntimeError('Authentication substack removed during verification')
        # Install dependency first. On interruption the old service stack still
        # works; the companion alone grants nothing without per-user opt-in.
        sudo.atomic_write(companion,auth_stack(service).encode(),0o644)
        subprocess.run(['/usr/sbin/restorecon',str(companion)],check=True)
    if args.action=='disable':
        marker.unlink(missing_ok=True)
        # The PAM block is shared; disabling one user must not disable another
        # separately opted-in account. Never count insecure/malformed markers.
        others=False
        for candidate in ROOT.glob(f'[0-9]*/pam-{service}.enabled'):
            if not candidate.parent.name.isdigit() or int(candidate.parent.name)<1000: continue
            sudo.protected(candidate,True)
            if candidate.read_bytes()==b'linux-hello-login-v1\n': others=True
        if others:
            print(f'{service}: disabled for {args.user}; other enrolled accounts retain their opt-in.')
            return
        remove=False
        if record.exists():
            sudo.protected(record,True)
            data=json.loads(record.read_text())
            if data.get('created_service') and not vendor.exists() and updated == LOCKER_DEFAULT.encode(): remove=True
            if not data['override_existed'] and vendor.exists():
                sudo.protected(vendor)
                remove=updated==vendor.read_bytes()
        if remove:
            pam.unlink(missing_ok=True)
            directory=os.open(pam.parent,os.O_DIRECTORY)
            try: os.fsync(directory)
            finally: os.close(directory)
        elif override and updated!=original:
            sudo.atomic_write(pam,updated,mode)
            subprocess.run(['/usr/sbin/restorecon',str(pam)],check=True)
    elif updated!=original or created_service:
        sudo.atomic_write(pam,updated,mode)
        subprocess.run(['/usr/sbin/restorecon',str(pam)],check=True)
    if args.action == 'enable':
        sudo.atomic_write(marker,b'linux-hello-login-v1\n',0o600)
        subprocess.run(['/usr/sbin/restorecon',str(marker)],check=True)
    print(f'{service}: {args.action}d for {args.user}; password/account/session hooks retained. Active session continues.')

if __name__=='__main__':
    try:
        if os.geteuid()!=0: raise RuntimeError('Run with sudo')
        with sudo.setup_lock(), sudo.write_transaction():
            main()
    except (ActivationError, subprocess.CalledProcessError, RuntimeError, OSError) as error:
        print(f'Linux Hello: {error}', file=sys.stderr)
        sys.exit(1)
