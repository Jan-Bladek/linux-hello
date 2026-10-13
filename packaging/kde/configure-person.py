#!/usr/bin/python3
"""One authorized GUI setup: import own reference, configure selected services.
Only pkexec's authenticated real subject is accepted. No face result from GUI is
trusted: every existing manager runs its real verification before activation.
"""
import fcntl
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


def request(input):
    line = input.readline(4097)
    if len(line) > 4096 or not line.endswith(b'\n'):
        raise ValueError('Invalid setup request')
    data = json.loads(line)
    if set(data) != {'sudo', 'login', 'unlock', 'source'}:
        raise ValueError('Unexpected setup fields')
    if any(type(data[key]) is not bool for key in ('sudo', 'login', 'unlock')):
        raise ValueError('Invalid selection')
    if not isinstance(data['source'], str) or not Path(data['source']).is_absolute():
        raise ValueError('Absolute reference directory required')
    length = struct.unpack('!H', input.read(2))[0]
    if not 12 <= length <= 1024:
        raise ValueError('Invalid secret length')
    secret = bytearray(input.read(length))
    if len(secret) != length or len(secret.decode('utf-8')) < 12 or input.read(1):
        raise ValueError('Invalid secret framing')
    return data, secret


def progress(message, recovery=False):
    try:
        print(json.dumps({'message': message}, ensure_ascii=False), flush=True)
    except BrokenPipeError:
        if not recovery:
            raise InterruptedError('Połączenie z aplikacją zostało przerwane.')


def complete(result):
    progress('Gotowe. Wybrane funkcje korzystają z zapisanej twarzy.')
    print(json.dumps({'configured': result}), flush=True)


def command(args, secret=None):
    result = subprocess.run(args, input=secret, stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL, timeout=100,
                            env={'LANG': 'C.UTF-8'})
    if result.returncode:
        raise RuntimeError('Weryfikacja lub konfiguracja nie powiodła się. Patrz prosto w kamerę i spróbuj ponownie.')


def configure(data, secret, account, sudo, kde):
    uid = account.pw_uid
    service = kde.detect() if data['login'] else None
    profile = sudo.ROOT / str(uid)
    sudo.protected(sudo.ROOT / '.anchor', True)
    paths = [profile / name for name in ('key', 'template.lht', 'config.json',
             'pam-sudo.disabled', 'pam-kde.enabled', '.config-pending')]
    paths += [sudo.PAM, PAM_DIR / 'kde', PAM_DIR / kde.SUBSTACK]
    for candidate in kde.SERVICES:
        paths += [profile / f'pam-{candidate}.enabled', PAM_DIR / candidate]
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
        profile.mkdir(mode=0o700, exist_ok=True)
        if not profile_is_private(profile):
            raise RuntimeError('Nieprawidłowy katalog chronionego wzorca.')
        if any(data[key] for key in ('sudo', 'login', 'unlock')):
            progress('Zapisuję chroniony wzorzec. Patrz prosto w kamerę.')
            time.sleep(3)
            framed = bytearray(struct.pack('!H', len(secret))) + secret
            try:
                command([str(BASE / 'hello-sudo'), 'enroll-stdin', account.pw_name, data['source']], framed)
            finally:
                framed[:] = b'\0' * len(framed)
        progress('Konfiguruję wybrane zastosowania twarzy. Pozostań przed kamerą.')
        if data['sudo']:
            (profile / 'pam-sudo.disabled').unlink(missing_ok=True)
            command(['/usr/bin/python3', '-B', str(BASE / 'manage.py'), 'enable', '--uid', str(uid)])
        else:
            sudo.atomic_write(profile / 'pam-sudo.disabled', b'linux-hello-sudo-disabled-v1\n', 0o600)
        if service:
            progress('Sprawdzam logowanie KDE. Patrz prosto — dwa krótkie testy.')
            command(['/usr/bin/python3', '-B', str(BASE / 'manage-kde.py'), 'enable', '--user', account.pw_name, '--service', service])
        else:
            # Disable own display-manager opt-in without changing other accounts.
            for candidate in kde.SERVICES:
                if (profile / f'pam-{candidate}.enabled').exists():
                    command(['/usr/bin/python3', '-B', str(BASE / 'manage-kde.py'), 'disable', '--user', account.pw_name, '--service', candidate])
        progress('Sprawdzam odblokowanie ekranu.' if data['unlock'] else 'Wyłączam odblokowanie twarzą dla tego konta.')
        if data['unlock'] or (profile / 'pam-kde.enabled').exists():
            command(['/usr/bin/python3', '-B', str(BASE / 'manage-kde.py'),
                     'enable' if data['unlock'] else 'disable', '--user', account.pw_name, '--service', 'kde'])
        result = {'sudo': data['sudo'], 'login': data['login'], 'unlock': data['unlock']}
        complete(result)
        return result
    except BaseException:
        progress('Przywracam poprzednią konfigurację. Hasło pozostaje dostępne.', recovery=True)
        for path, before in snapshots.items():
            if before is None:
                path.unlink(missing_ok=True)
            else:
                sudo.atomic_write(path, before[0], before[1])
            if path.exists():
                subprocess.run(['/usr/sbin/restorecon', str(path)], check=True)
        raise


def main():
    if os.geteuid() != 0 or len(sys.argv) != 1:
        raise RuntimeError('Uruchom konfigurację przez aplikację i systemową autoryzację.')
    uid = int(os.environ.get('PKEXEC_UID', '-1'))
    if uid < 1000:
        raise RuntimeError('Brak autoryzowanego konta użytkownika.')
    account = pwd.getpwuid(uid)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    # All code imports come from fixed administrator-owned installed siblings.
    # Validate ownership before importing any privileged management code.
    for path in (Path(__file__).absolute(), BASE / 'manage.py', BASE / 'manage-kde.py'):
        for parent in reversed(path.parents):
            meta = parent.lstat()
            if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != 0 or meta.st_mode & 0o022:
                raise RuntimeError('Niebezpieczny katalog konfiguratora.')
        meta = path.lstat()
        if not stat.S_ISREG(meta.st_mode) or meta.st_uid != 0 or meta.st_mode & 0o022 or meta.st_nlink != 1:
            raise RuntimeError('Niebezpieczny plik konfiguratora.')
    sudo = load('hello_management', BASE / 'manage.py')
    sudo.protected(Path(__file__).absolute())
    for path in (BASE / 'manage.py', BASE / 'manage-kde.py', BASE / 'hello-sudo'):
        sudo.protected(path)
    kde = load('hello_kde_management', BASE / 'manage-kde.py')
    # Never hold a global configuration lock waiting on an unresponsive sender.
    if not select.select([sys.stdin], [], [], 15)[0]:
        raise RuntimeError('Przekroczono czas przesłania konfiguracji.')
    data, secret = request(sys.stdin.buffer)
    os.umask(0o077)
    def interrupted(*_):
        raise InterruptedError('Konfiguracja anulowana')
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
