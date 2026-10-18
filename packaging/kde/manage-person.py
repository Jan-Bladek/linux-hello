#!/usr/bin/python3
"""Authorized self-account status, pause and protected reference removal.
No target UID, password, model path, camera capture or authentication verdict.
"""
import fcntl
import importlib.util
import json
import math
import os
from pathlib import Path
import pwd
import resource
import signal
import stat
import subprocess
import sys

BASE = Path('/usr/local/libexec/linux-hello-sudo')
PAM_DIR = Path('/etc/pam.d')
MAGIC = b'linux-hello-login-v1\n'


def subject(environment):
    raw = environment.get('PKEXEC_UID', environment.get('SUDO_UID', ''))
    if not raw.isdecimal() or int(raw) < 1000:
        raise RuntimeError('Authorize this operation as a normal account through pkexec or sudo.')
    return pwd.getpwuid(int(raw))


def trusted(path):
    for parent in reversed(path.parents):
        meta = parent.lstat()
        if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != 0 or meta.st_mode & 0o022:
            raise RuntimeError('Unsafe management code directory')
    meta = path.lstat()
    if not stat.S_ISREG(meta.st_mode) or meta.st_uid != 0 or meta.st_nlink != 1 or meta.st_mode & 0o022:
        raise RuntimeError('Unsafe management code file')


def load(name, path):
    trusted(path)
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


def present(path):
    try: path.lstat(); return True
    except FileNotFoundError: return False


def private_profile(profile, uid):
    if profile.name != str(uid): raise RuntimeError('Account/profile mismatch')
    if not present(profile): return False
    meta = profile.lstat()
    if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != 0 or meta.st_mode & 0o077:
        raise RuntimeError('Unsafe protected reference directory')
    return True


def reference_valid(uid, sudo):
    """Use the verifier's exact parser/AEAD/model checks, without opening the camera."""
    helper = BASE / 'hello-sudo'
    sudo.protected(helper)
    try:
        result = subprocess.run([str(helper), 'validate', str(uid)], check=False,
                                timeout=20, stdin=subprocess.DEVNULL,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                env=sudo.child_environment(), **sudo.child_lock_options())
        return result.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def status(account, sudo, kde):
    profile = sudo.ROOT / str(account.pw_uid)
    enrolled = False; backend = 'none'
    if private_profile(profile, account.pw_uid):
        for name in ('config.json', 'template.lht', 'key.tpm', 'key'):
            if present(profile / name): sudo.protected(profile / name, True)
        backend = 'tpm' if present(profile / 'key.tpm') else 'file' if present(profile / 'key') else 'none'
        if present(profile / 'config.json'):
            if (profile / 'config.json').stat().st_size > 16384: raise RuntimeError('Configuration exceeds its size limit')
            config = json.loads((profile / 'config.json').read_bytes())
            if config.get('key_provider') == 'tpm': backend = 'tpm'
            threshold = config.get('threshold')
            valid = (config.get('uid') == account.pw_uid and config.get('version') == 1
                     and type(threshold) in (float, int) and math.isfinite(threshold)
                     and 0.80 <= threshold <= 1.0)
            enrolled = valid and backend != 'none' and present(profile / 'template.lht') and reference_valid(account.pw_uid, sudo)
    def pam_contains(service, block):
        path = PAM_DIR / service
        if not present(path): return False
        sudo.protected(path)
        if path.stat().st_size > 16384: raise RuntimeError('PAM stack exceeds its size limit')
        return block in path.read_text()
    def opted(service):
        path = profile / f'pam-{service}.enabled'
        if not present(path): return False
        sudo.protected(path, True)
        return path.read_bytes() == MAGIC and pam_contains(service, ''.join(kde.layout(service)[:2]))
    return {'version': 1, 'uid': account.pw_uid, 'enrolled': bool(enrolled),
            'sudo': bool(enrolled and not present(profile / 'pam-sudo.disabled') and pam_contains('sudo', sudo.MARKER + sudo.LINE)),
            'login': bool(enrolled and any(opted(service) for service in kde.SERVICES)),
            'unlock': bool(enrolled and any(opted(service) for service in kde.LOCK_SERVICES)), 'key_backend': backend}


def operate(action, account, sudo, kde, acknowledge=None):
    if action not in ('status', 'pause', 'remove'): raise RuntimeError('Unknown account operation')
    if action == 'status' and ((sudo.ROOT / '.operation-update').exists() or (sudo.ROOT / '.install-update').exists()):
        raise RuntimeError('Interrupted system operation requires administrator recovery')
    uid = account.pw_uid; profile = sudo.ROOT / str(uid)
    if action == 'status' or not private_profile(profile, uid):
        result = status(account, sudo, kde)
        if acknowledge: acknowledge(result)
        return result
    names = ['key', 'key.tpm', 'template.lht', 'config.json', 'pam-sudo.disabled',
             *[f'pam-{service}.enabled' for service in (*kde.SERVICES, *kde.LOCK_SERVICES)]]
    paths = [profile / name for name in names]
    paths += [sudo.PAM]
    for service in (*kde.SERVICES, *kde.LOCK_SERVICES):
        paths += [PAM_DIR / service, PAM_DIR / kde.layout(service)[2]]
    planned = {profile / 'pam-sudo.disabled': (b'linux-hello-sudo-disabled-v1\n', 0o600)}
    planned.update({profile / f'pam-{service}.enabled': None for service in (*kde.SERVICES, *kde.LOCK_SERVICES)})
    if action == 'remove':
        planned.update({profile / name: None for name in ('key', 'key.tpm', 'template.lht', 'config.json')})
    journal = sudo.operation_journal(sudo.ROOT, paths, planned, sudo.protected, sudo.atomic_write)
    try:
        sudo.bump_epoch()
        # Disable only this UID. Never globally disable sudo for other accounts.
        sudo.atomic_write(profile / 'pam-sudo.disabled', b'linux-hello-sudo-disabled-v1\n', 0o600)
        for service in (*kde.SERVICES, *kde.LOCK_SERVICES):
            if present(profile / f'pam-{service}.enabled'):
                subprocess.run(['/usr/bin/python3', '-B', str(BASE / 'manage-kde.py'), 'disable',
                                '--user', account.pw_name, '--service', service],
                               check=True, timeout=20, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                               env=sudo.child_environment(), **sudo.child_lock_options())
        if action == 'remove':
            for name in ('key', 'key.tpm', 'template.lht', 'config.json'):
                (profile / name).unlink(missing_ok=True)
        descriptor = os.open(profile, os.O_DIRECTORY | os.O_CLOEXEC)
        try: os.fsync(descriptor)
        finally: os.close(descriptor)
        result = status(account, sudo, kde)
        if any(result[name] for name in ('sudo', 'login', 'unlock')):
            raise RuntimeError('Account uses were not fully disabled')
        if acknowledge: acknowledge(result)
        journal.commit()
        return result
    except BaseException:
        journal.rollback()
        raise


def main():
    if os.geteuid() != 0 or len(sys.argv) != 2: raise RuntimeError('Authorize one self-account operation')
    account = subject(os.environ)
    trusted(Path(__file__).absolute())
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0)); os.umask(0o077)
    sudo = load('hello_management', BASE / 'manage.py')
    kde = load('hello_kde_management', BASE / 'manage-kde.py')
    sudo.protected(sudo.ROOT / '.anchor', True)
    lockpath = sudo.ROOT / '.setup.lock'
    lockpath.touch(mode=0o600, exist_ok=True); sudo.protected(lockpath, True)
    def interrupted(*_): raise InterruptedError('Account operation cancelled')
    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM): signal.signal(sig, interrupted)
    signal.alarm(90)
    try:
        with lockpath.open('r+') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            os.environ['LINUX_HELLO_SETUP_LOCK_FD'] = str(lock.fileno())
            def acknowledge(result):
                result['operation'] = sys.argv[1]
                print(json.dumps(result), flush=True)
            operate(sys.argv[1], account, sudo, kde, acknowledge)
    finally: signal.alarm(0)


if __name__ == '__main__':
    try: main()
    except Exception:
        print(json.dumps({'error': 'Account operation failed. Use your password. Interrupted or conflicting changes require administrator review of the durable operation journal.'}), flush=True)
        sys.exit(1)
