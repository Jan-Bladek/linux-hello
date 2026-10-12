#!/usr/bin/python3
"""Root-only opt-in for the installed KDE display manager. Preserves password PAM."""
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
LINE = 'auth        [success=1 default=ignore] /usr/lib64/security/pam_linuxhello.so\n'
SERVICES = ('plasmalogin', 'sddm')

class ActivationError(RuntimeError):
    """Expected, fail-closed live-test failure; no Python traceback needed."""

def run_probe(command, stage):
    print(f'Test {stage}/2. Patrz prosto, pokaz oba oczy; nie skrecaj glowy.', flush=True)
    for remaining in (3, 2, 1):
        print(f'Skanowanie za {remaining}...', flush=True)
        time.sleep(1)
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=40, env={'LANG':'C.UTF-8'})
    except subprocess.TimeoutExpired as error:
        raise ActivationError(f'Test {stage}/2: przekroczono czas weryfikacji. PAM logowania nie zostal zmieniony.') from error
    report_probe(result.stdout + result.stderr)
    if result.returncode:
        raise ActivationError(f'Test {stage}/2 odrzucony. PAM logowania nie zostal zmieniony; haslo nadal dziala. Szczegoly powyzej.')

def report_probe(output):
    """Display bounded metadata from this exact PAM invocation, never biometrics."""
    reasons = {
        'landmark_shape_mismatch': 'geometria twarzy nie pasuje do wzorca',
        'centroid_similarity_below_threshold': 'podobienstwo do sredniego wzorca jest za niskie',
        'exemplar_similarity_below_threshold': 'podobienstwo do zapisanych ujec jest za niskie',
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
            print(f"Ujecia {data.get('accepted_frames',0)}/3: {str(data['progress'])[:512]}", flush=True)
        if 'outcome' not in data: continue
        score, threshold = data.get('similarity'), data.get('threshold')
        if isinstance(score, (int,float)) and isinstance(threshold, (int,float)):
            print(f'Podobienstwo: {score:.3f}; wymagane: {threshold:.3f}.', flush=True)
        reason = data.get('failure_reason')
        if reason: print('Przyczyna: ' + reasons.get(str(reason), str(reason)[:1024]), flush=True)
        if not data.get('cleanup_ok',False): print('Nie udalo sie bezpiecznie zamknac kamery; wynik odrzucony.', flush=True)

def diagnostic_stack(content):
    """Enable metadata only in the private root-owned test, never production PAM."""
    text = content.decode('utf-8')
    if text.count(LINE) != 1: raise RuntimeError('Missing unique Linux Hello test line')
    return text.replace(LINE, LINE.rstrip('\n') + ' diagnostic\n', 1).encode()

def changed(original, enable):
    if original.count(MARKER) != original.count(LINE) or original.count(MARKER) > 1:
        raise RuntimeError('Malformed Linux Hello KDE PAM block')
    if MARKER in original:
        if MARKER + LINE not in original: raise RuntimeError('Non-adjacent Linux Hello KDE PAM block')
        return original if enable else original.replace(MARKER + LINE, '', 1)
    if not enable: return original
    lines = original.splitlines(keepends=True)
    indexes = [i for i,s in enumerate(lines) if s.split() == ['auth','substack','password-auth']]
    if len(indexes) != 1: raise RuntimeError('Unsupported KDE PAM stack; expected one auth substack password-auth')
    # Jump only over that password substack after a successful face match. Keep
    # preceding controls, optional wallet hooks, postlogin and all other groups.
    lines.insert(indexes[0], MARKER + LINE)
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

def probe(user, content):
    driver = BASE / 'kde-pam-probe'
    sudo.protected(driver)
    with tempfile.TemporaryDirectory(prefix='kde-pam-test-', dir=ROOT) as folder:
        conf = Path(folder)
        snapshot_pam(conf)
        service = detect()
        command=['/usr/bin/runcon','-u','system_u','-r','system_r','-t','xdm_t','-l','s0-s0:c0.c1023',str(driver),service,user,str(conf)]
        # First prove that this adapter itself matched, independently of any
        # pre-existing permit/other credential module in the vendor stack.
        strict='auth sufficient /usr/lib64/security/pam_linuxhello.so diagnostic\nauth required pam_deny.so\naccount required pam_permit.so\n'
        sudo.atomic_write(conf / service, strict.encode(), 0o600)
        subprocess.run(['/usr/sbin/restorecon','-R',str(conf)], check=True)
        run_probe(command,1)
        sudo.atomic_write(conf / service, diagnostic_stack(content), 0o600)
        subprocess.run(['/usr/sbin/restorecon','-R',str(conf)], check=True)
        # Identical SELinux domain to the real Plasma/SDDM root helper. Probe
        # authenticates/checks account/credentials but opens no desktop session.
        run_probe(command,2)

def main():
    p = argparse.ArgumentParser()
    p.add_argument('action',choices=['enable','disable','status'])
    p.add_argument('--user',required=True)
    p.add_argument('--service',choices=SERVICES)
    args=p.parse_args()
    if os.geteuid()!=0: p.error('Run with sudo')
    import pwd
    uid=pwd.getpwnam(args.user).pw_uid
    if uid<1000: p.error('Normal user account required')
    service=args.service or detect()
    if args.action=='enable' and service!=detect(): raise RuntimeError('Service is not the active display manager')
    pam=Path('/etc/pam.d') / service
    vendor=Path('/usr/lib/pam.d') / service
    override=pam.exists() or pam.is_symlink()
    source=pam if override else vendor
    meta=sudo.protected(source)
    original=source.read_bytes()
    text=original.decode('utf-8')
    marker=ROOT / str(uid) / f'pam-{service}.enabled'
    if args.action=='status':
        enabled=MARKER+LINE in text and marker.exists()
        print(f'{service}: {"enabled" if enabled else "disabled"} for {args.user}')
        return
    updated=changed(text,args.action=='enable').encode()
    sudo.protected(ROOT / '.anchor')
    record=ROOT / f'kde-{service}-original.json'
    if args.action=='enable':
        sudo.protected(ROOT / str(uid) / 'config.json',True)
        sudo.protected(BASE / 'hello-sudo')
        sudo.protected('/usr/lib64/security/pam_linuxhello.so')
        existed=marker.exists() or marker.is_symlink()
        if existed: sudo.protected(marker,True)
        sudo.atomic_write(marker,b'linux-hello-login-v1\n',0o600)
        subprocess.run(['/usr/sbin/restorecon',str(marker)],check=True)
        try:
            print('Patrz prosto w kamere: testuje PAM i SELinux ekranu logowania.',flush=True)
            probe(args.user,updated)
        except BaseException:
            if not existed: marker.unlink(missing_ok=True)
            raise
        backup=ROOT / f'{service}-before-{hashlib.sha256(original).hexdigest()[:16]}.pam'
        if not backup.exists(): sudo.atomic_write(backup,original,0o600)
        if not record.exists():
            sudo.atomic_write(record,json.dumps({'override_existed':override,'backup':str(backup)}).encode(),0o600)
    # Preserve unexpected administrator changes made during the live probe.
    if source.read_bytes()!=original or ((pam.exists() or pam.is_symlink()) != override):
        raise RuntimeError('KDE PAM changed during verification')
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
            if not data['override_existed'] and vendor.exists():
                sudo.protected(vendor)
                remove=updated==vendor.read_bytes()
        if remove:
            pam.unlink(missing_ok=True)
            directory=os.open(pam.parent,os.O_DIRECTORY)
            try: os.fsync(directory)
            finally: os.close(directory)
        elif override and updated!=original:
            sudo.atomic_write(pam,updated,stat.S_IMODE(meta.st_mode))
            subprocess.run(['/usr/sbin/restorecon',str(pam)],check=True)
    elif updated!=original:
        sudo.atomic_write(pam,updated,stat.S_IMODE(meta.st_mode))
        subprocess.run(['/usr/sbin/restorecon',str(pam)],check=True)
    print(f'{service}: {args.action}d for {args.user}; password/account/session/wallet hooks retained. Active session continues.')

if __name__=='__main__':
    try:
        main()
    except (ActivationError, subprocess.CalledProcessError, RuntimeError, OSError) as error:
        print(f'Linux Hello: {error}', file=sys.stderr)
        sys.exit(1)
