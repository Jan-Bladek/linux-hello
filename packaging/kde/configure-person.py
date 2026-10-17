#!/usr/bin/python3
"""One authorized GUI setup: import own reference, configure selected services.
Only pkexec's authenticated real subject is accepted. No face result from GUI is
trusted: every existing manager runs its real verification before activation.
"""
import fcntl
import io
import importlib.util
import json
import os
from pathlib import Path
import pwd
import resource
import stat
import select
import signal
import struct
import subprocess
import sys
import tempfile
import time

BASE = Path('/usr/local/libexec/linux-hello-sudo')
PAM_DIR = Path('/etc/pam.d')


def profile_is_private(profile):
    meta = profile.lstat()
    return stat.S_ISDIR(meta.st_mode) and meta.st_uid == 0 and not meta.st_mode & 0o077


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class DeadlineInput:
    """One monotonic deadline covers header, body and required EOF."""
    def __init__(self, stream, timeout=15):
        self.fd = stream.fileno()
        self.deadline = time.monotonic() + timeout
        os.set_blocking(self.fd, False)

    def read(self, length):
        result = bytearray()
        while len(result) < length:
            remaining = self.deadline - time.monotonic()
            if remaining <= 0 or not select.select([self.fd], [], [], remaining)[0]:
                raise TimeoutError('Setup request timed out')
            try:
                chunk = os.read(self.fd, length - len(result))
            except (BlockingIOError, InterruptedError):
                continue
            if not chunk:
                break
            result.extend(chunk)
        return result

    def readline(self, limit):
        result = bytearray()
        while len(result) < limit:
            byte = self.read(1)
            if not byte:
                break
            result.extend(byte)
            if byte == b'\n':
                break
        return result


def request(input):
    line = input.readline(4097)
    if len(line) > 4096 or not line.endswith(b'\n'):
        raise ValueError('Invalid setup request')
    data = json.loads(line)
    if set(data) not in ({'sudo', 'login', 'unlock', 'source'}, {'sudo', 'login', 'unlock', 'source', 'unlock_service'}):
        raise ValueError('Unexpected setup fields')
    if any(type(data[key]) is not bool for key in ('sudo', 'login', 'unlock')):
        raise ValueError('Invalid selection')
    if not isinstance(data['source'], str) or not Path(data['source']).is_absolute():
        raise ValueError('Absolute reference directory required')
    if data.get('unlock_service', 'kde') not in ('kde', 'hyprlock', 'login'):
        raise ValueError('Unsupported screen locker')
    length = struct.unpack('!H', input.read(2))[0]
    if not 12 <= length <= 1024:
        raise ValueError('Invalid secret length')
    secret = bytearray(input.read(length))
    try:
        if len(secret) != length or len(secret.decode('utf-8')) < 12 or input.read(1):
            raise ValueError('Invalid secret framing')
        return data, secret
    except BaseException:
        secret[:] = b'\0' * len(secret)
        raise


def progress(message, recovery=False):
    try:
        print(json.dumps({'message': message}, ensure_ascii=False), flush=True)
    except BrokenPipeError:
        if not recovery:
            raise InterruptedError('The connection to the application was interrupted.')


def complete(result):
    progress('Ready. Selected uses now use the saved face.')
    print(json.dumps({'configured': result}), flush=True)


def command(args, secret=None):
    result = subprocess.run(args, input=secret, stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL, timeout=100,
                            env={'LANG': 'C.UTF-8', **({ 'LINUX_HELLO_SETUP_LOCK_FD': os.environ['LINUX_HELLO_SETUP_LOCK_FD']} if 'LINUX_HELLO_SETUP_LOCK_FD' in os.environ else {})},
                            pass_fds=(int(os.environ['LINUX_HELLO_SETUP_LOCK_FD']),) if 'LINUX_HELLO_SETUP_LOCK_FD' in os.environ else ())
    if result.returncode:
        raise RuntimeError('Verification or setup failed. Look straight at the camera and try again.')


def configure(data, secret, account, sudo, kde):
    uid = account.pw_uid
    service = kde.detect() if data['login'] else None
    profile = sudo.ROOT / str(uid)
    sudo.protected(sudo.ROOT / '.anchor', True)
    lockers = kde.LOCK_SERVICES
    paths = [profile / name for name in ('key', 'key.tpm', 'template.lht', 'config.json',
             'pam-sudo.disabled', '.config-pending')]
    paths += [sudo.PAM]
    for candidate in (*kde.SERVICES, *lockers):
        paths += [profile / f'pam-{candidate}.enabled', PAM_DIR / candidate,
                  PAM_DIR / kde.layout(candidate)[2]]
    # Protected encrypted files are held in memory only for rollback, not exported
    # or logged. Profile keys/templates remain root-private throughout.
    snapshots = {}
    for path in paths:
        if path.exists() or path.is_symlink():
            meta = sudo.protected(path, path.parent == profile)
            snapshots[path] = (path.read_bytes(), meta.st_mode & 0o777)
        else:
            snapshots[path] = None
    try:
        sudo.bump_epoch()
        profile.mkdir(mode=0o700, exist_ok=True)
        if not profile_is_private(profile):
            raise RuntimeError('Invalid protected reference directory.')
        if any(data[key] for key in ('sudo', 'login', 'unlock')):
            progress('Saving the protected reference. Look straight at the camera.')
            time.sleep(3)
            framed = bytearray(struct.pack('!H', len(secret))) + secret
            try:
                command([str(BASE / 'hello-sudo'), 'enroll-stdin', account.pw_name, data['source']], framed)
            finally:
                framed[:] = b'\0' * len(framed)
        progress('Configuring selected uses. Keep facing the camera.')
        if data['sudo']:
            (profile / 'pam-sudo.disabled').unlink(missing_ok=True)
            command(['/usr/bin/python3', '-B', str(BASE / 'manage.py'), 'enable', '--uid', str(uid)])
        else:
            sudo.atomic_write(profile / 'pam-sudo.disabled', b'linux-hello-sudo-disabled-v1\n', 0o600)
        if service:
            progress('Checking desktop sign-in. Look straight ahead for two short checks.')
            command(['/usr/bin/python3', '-B', str(BASE / 'manage-kde.py'), 'enable', '--user', account.pw_name, '--service', service])
        else:
            # Disable own display-manager opt-in without changing other accounts.
            for candidate in kde.SERVICES:
                if (profile / f'pam-{candidate}.enabled').exists():
                    command(['/usr/bin/python3', '-B', str(BASE / 'manage-kde.py'), 'disable', '--user', account.pw_name, '--service', candidate])
        progress('Checking screen unlock.' if data['unlock'] else 'Disabling face unlock for this account.')
        selected = data.get('unlock_service', 'kde')
        for candidate in lockers:
            if (data['unlock'] and candidate == selected) or (not data['unlock'] and (profile / f'pam-{candidate}.enabled').exists()):
                command(['/usr/bin/python3', '-B', str(BASE / 'manage-kde.py'),
                         'enable' if data['unlock'] else 'disable', '--user', account.pw_name, '--service', candidate])
        result = {'sudo': data['sudo'], 'login': data['login'], 'unlock': data['unlock']}
        complete(result)
        return result
    except BaseException:
        progress('Restoring the previous configuration. Password fallback remains available.', recovery=True)
        sudo.restore_snapshots(snapshots)
        raise


def main():
    if os.geteuid() != 0 or len(sys.argv) != 1:
        raise RuntimeError('Run setup through the application and system authorization.')
    uid = int(os.environ.get('PKEXEC_UID', '-1'))
    if uid < 1000:
        raise RuntimeError('No authorized user account.')
    account = pwd.getpwuid(uid)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    # All code imports come from fixed administrator-owned installed siblings.
    # Validate ownership before importing any privileged management code.
    for path in (Path(__file__).absolute(), BASE / 'manage.py', BASE / 'manage-kde.py'):
        for parent in reversed(path.parents):
            meta = parent.lstat()
            if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != 0 or meta.st_mode & 0o022:
                raise RuntimeError('Unsafe setup helper directory.')
        meta = path.lstat()
        if not stat.S_ISREG(meta.st_mode) or meta.st_uid != 0 or meta.st_mode & 0o022 or meta.st_nlink != 1:
            raise RuntimeError('Unsafe setup helper file.')
    sudo = load('hello_management', BASE / 'manage.py')
    sudo.protected(Path(__file__).absolute())
    for path in (BASE / 'manage.py', BASE / 'manage-kde.py', BASE / 'hello-sudo'):
        sudo.protected(path)
    kde = load('hello_kde_management', BASE / 'manage-kde.py')
    # Never hold a global configuration lock waiting on an unresponsive sender.
    data, secret = request(DeadlineInput(sys.stdin.buffer))
    os.umask(0o077)
    def interrupted(*_):
        raise InterruptedError('Setup cancelled')
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    signal.signal(signal.SIGALRM, interrupted)
    signal.alarm(240)
    try:
        lock_path = sudo.ROOT / '.setup.lock'
        lock_path.touch(mode=0o600, exist_ok=True)
        sudo.protected(lock_path, True)
        with lock_path.open('r+') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            os.environ['LINUX_HELLO_SETUP_LOCK_FD'] = str(lock.fileno())
            configure(data, secret, account, sudo, kde)
    finally:
        secret[:] = b'\0' * len(secret)
        signal.alarm(0)


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        progress(str(error), recovery=True)
        sys.exit(1)
