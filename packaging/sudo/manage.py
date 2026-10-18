#!/usr/bin/python3
"""Root-only sudo PAM opt-in/rollback. Does not edit authselect/system-auth."""
import argparse
import contextlib
import fcntl
import signal
import hashlib
import os
from pathlib import Path
import stat
import subprocess
import tempfile

PAM = Path('/etc/pam.d/sudo')
ROOT = Path('/var/lib/linux-hello/sudo')
HELPER = '/usr/local/libexec/linux-hello-sudo/hello-sudo'
MARKER = '# Linux Hello sudo-only experimental integration\n'
LEGACY_LINE = 'auth       sufficient   /usr/lib64/security/pam_linuxhello.so\n'
PREAUTH = 'auth required pam_env.so\nauth required pam_faildelay.so delay=2000000\n'
LINE = PREAUTH + LEGACY_LINE

def protected(path, private=False):
    path = Path(path)
    for parent in reversed(path.parents):
        m = parent.lstat()
        if not stat.S_ISDIR(m.st_mode) or m.st_uid != 0 or m.st_mode & 0o022:
            raise RuntimeError(f'Insecure ancestor: {parent}')
    m = path.lstat()
    mask = 0o077 if private else 0o022
    if not stat.S_ISREG(m.st_mode) or m.st_uid != 0 or m.st_mode & mask or m.st_nlink != 1:
        raise RuntimeError(f'Insecure file: {path}')
    return m

def changed(original, enable):
    if MARKER + LEGACY_LINE in original:
        original = original.replace(MARKER + LEGACY_LINE, MARKER + LINE, 1)
    # Strict paired markers: refuse malformed/hand-edited integration blocks.
    if original.count(MARKER) != original.count(LINE) or original.count(MARKER) > 1:
        raise RuntimeError('Unexpected existing Linux Hello PAM entries')
    if MARKER in original:
        if MARKER + LINE not in original:
            raise RuntimeError('Linux Hello PAM entries are not adjacent')
        if enable:
            remaining = original.replace(MARKER + LINE, '', 1)
            if auth_lines(remaining) != ['auth include system-auth']:
                raise RuntimeError('Unsupported managed sudo auth policy')
        return original if enable else original.replace(MARKER + LINE, '', 1)
    if not enable:
        return original
    lines = original.splitlines(keepends=True)
    expected = [i for i, s in enumerate(lines) if s.split() == ['auth', 'include', 'system-auth']]
    if len(expected) != 1:
        raise RuntimeError('Unsupported sudo PAM stack; review manually')
    auth = [line.split() for line in auth_lines(original)]
    if auth != [['auth', 'include', 'system-auth']]:
        raise RuntimeError('Unsupported sudo auth policy; additional controls require manual review')
    lines.insert(expected[0], MARKER + LINE)
    return ''.join(lines)

def atomic_write(path, data, mode):
    fd, name = tempfile.mkstemp(prefix='.hello-', dir=path.parent)
    try:
        os.fchmod(fd, mode)
        with os.fdopen(fd, 'wb') as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(name, path)
        directory = os.open(path.parent, os.O_DIRECTORY)
        try: os.fsync(directory)
        finally: os.close(directory)
    finally:
        if os.path.exists(name): os.unlink(name)

def auth_lines(text):
    """Reviewed PAM subset, fail closed on syntax we cannot interpret.
    Linux-PAM type/control keywords are case-insensitive. Module paths/options
    are not. Continuations, unknown record types and malformed records are
    refused explicitly, never dropped from the authentication policy.
    """
    result = []
    for raw in text.split('\n'):
        record = raw.split('#', 1)[0]
        if any(ord(c) > 126 or (ord(c) < 32 and c not in '\t\r') for c in record):
            raise RuntimeError('Unsupported PAM encoding or control character')
        line = record.strip()
        if not line:
            continue
        if '\\' in line or '\0' in line:
            raise RuntimeError('Unsupported PAM continuation or malformed record')
        fields = line.split()
        group = fields[0].removeprefix('-').lower()
        if group not in ('auth', 'account', 'password', 'session') or len(fields) < 3:
            raise RuntimeError('Unsupported or malformed PAM record')
        if group == 'auth':
            fields[0] = 'auth'
            fields[1] = fields[1].lower()
            result.append(' '.join(fields))
    return result


def supported_password_policy(text):
    """Only the explicitly reviewed local Fedora factor policy is supported.
    Unknown modules/options/includes (including MFA/faillock) are refused.
    """
    lines = auth_lines(text)
    allowed = [
        ['auth required pam_env.so', 'auth required pam_faildelay.so delay=2000000',
         'auth sufficient pam_unix.so nullok', 'auth required pam_deny.so'],
        ['auth required pam_env.so', 'auth required pam_faildelay.so delay=2000000',
         'auth sufficient pam_fprintd.so', 'auth sufficient pam_unix.so nullok',
         'auth required pam_deny.so'],
    ]
    if lines not in allowed:
        raise RuntimeError('Unsupported password auth policy: MFA/access/lockout controls require a reviewed integration')


def check_password_policy(service):
    path = Path('/etc/pam.d') / service
    if not (path.exists() or path.is_symlink()): path = Path('/usr/lib/pam.d') / service
    path = path.resolve(strict=True)
    meta = protected(path)
    if meta.st_size > 16384: raise RuntimeError('Password policy exceeds size limit')
    data = path.read_bytes()
    supported_password_policy(data.decode('utf-8'))
    return path, data


@contextlib.contextmanager
def setup_lock(recovery=False):
    """Direct and nested managers use the same validated open file description."""
    path = ROOT / '.setup.lock'
    protected(ROOT / '.anchor', True)
    inherited = os.environ.get('LINUX_HELLO_SETUP_LOCK_FD')
    if inherited is None and not recovery and ((ROOT / '.operation-update').exists() or (ROOT / '.install-update').exists()):
        raise RuntimeError('Interrupted operation requires administrator recovery')
    if inherited is not None:
        fd = os.dup(int(inherited))
    else:
        fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    with os.fdopen(fd, 'rb') as lock:
        meta = protected(path, True)
        opened = os.fstat(lock.fileno())
        if (meta.st_dev, meta.st_ino) != (opened.st_dev, opened.st_ino):
            raise RuntimeError('Invalid setup lock descriptor')
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        previous = os.environ.get('LINUX_HELLO_SETUP_LOCK_FD')
        os.environ['LINUX_HELLO_SETUP_LOCK_FD'] = str(lock.fileno())
        try: yield lock
        finally:
            if previous is None: os.environ.pop('LINUX_HELLO_SETUP_LOCK_FD', None)
            else: os.environ['LINUX_HELLO_SETUP_LOCK_FD'] = previous


def child_lock_options():
    raw = os.environ.get('LINUX_HELLO_SETUP_LOCK_FD')
    return {} if raw is None else {'pass_fds': (int(raw),)}


def child_environment():
    result = {'LANG': 'C.UTF-8'}
    raw = os.environ.get('LINUX_HELLO_SETUP_LOCK_FD')
    if raw is not None: result['LINUX_HELLO_SETUP_LOCK_FD'] = raw
    return result


@contextlib.contextmanager
def write_transaction():
    """Rollback every managed write, without overwriting concurrent admin edits.
    This handles exceptions/signals, not SIGKILL or power loss.
    """
    before = {}; expected = {}; original_write = atomic_write
    def current(path):
        if not path.exists() and not path.is_symlink(): return None
        meta = protected(path)
        if meta.st_size > 8 * 1024 * 1024: raise RuntimeError('Snapshot exceeds limit')
        return path.read_bytes(), stat.S_IMODE(meta.st_mode)
    def write(path, data, mode):
        path = Path(path)
        if path == ROOT / '.auth-epoch': return original_write(path,data,mode)
        if not (path.parent == ROOT or (path.parent.parent == ROOT and path.parent.name.isdecimal())
                or path.parent == Path('/etc/pam.d')):
            return original_write(path, data, mode)
        if path not in before:
            before[path] = current(path)
        elif current(path) != expected[path]:
            raise RuntimeError('Administrator changed a managed file before publication')
        expected[path] = (data, mode)
        original_write(path, data, mode)
    globals()['atomic_write'] = write
    try: yield
    except BaseException as original_error:
        failures = []
        handlers = {sig: signal.signal(sig, signal.SIG_IGN)
                    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM)}
        try:
            for path, snapshot in reversed(list(before.items())):
                try:
                    state = current(path)
                    if state == snapshot: continue
                    if state != expected[path]:
                        raise RuntimeError('Administrator changed a managed file during rollback')
                    if snapshot is None: path.unlink(missing_ok=True)
                    else: original_write(path, snapshot[0], snapshot[1])
                    if path.exists(): subprocess.run(['/usr/sbin/restorecon', str(path)], check=True)
                except BaseException as error: failures.append(error)
        finally:
            for sig, handler in handlers.items(): signal.signal(sig, handler)
        if failures: raise RuntimeError('Rollback incomplete; use password authentication and review managed files') from original_error
        raise
    finally: globals()['atomic_write'] = original_write


def bump_epoch():
    path = ROOT / '.auth-epoch'
    if path.exists() or path.is_symlink(): protected(path, True)
    atomic_write(path, os.urandom(32), 0o600)


def operation_journal(root, paths, planned, validate, writer, begin=True):
    """Fixed-sibling code and durable preimages; caller holds the setup lock."""
    import importlib.util
    source = Path(__file__).with_name('operation-journal.py')
    validate(source)
    spec = importlib.util.spec_from_file_location('hello_operation_journal', source)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    def read(path):
        if not path.exists() and not path.is_symlink(): return None
        meta = validate(path, path.parent.parent == root and path.parent.name.isdecimal())
        if meta.st_size > module.MAX_FILE: raise RuntimeError('Operation snapshot exceeds limit')
        return path.read_bytes(), stat.S_IMODE(meta.st_mode)
    def replace(path, before):
        if path.exists() or path.is_symlink(): validate(path)
        if before is None: path.unlink(missing_ok=True)
        else: writer(path, before[0], before[1])
        if path.exists(): subprocess.run(['/usr/sbin/restorecon', str(path)], check=True)
    journal = module.Journal(root / '.operation-update', paths, read, replace)
    if begin: journal.cleanup_retired()
    return journal.begin(planned) if begin else journal


def restore_snapshots(snapshots, write=None):
    """Attempt every recovery step, even if one file or restorecon fails."""
    write = write or atomic_write
    failures = []
    handlers = {sig: signal.signal(sig, signal.SIG_IGN)
                for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM)}
    try:
        for path, before in snapshots.items():
            try:
                if before is None: path.unlink(missing_ok=True)
                else: write(path, before[0], before[1])
            except BaseException as error: failures.append(error)
            try:
                if path.exists(): subprocess.run(['/usr/sbin/restorecon', str(path)], check=True)
            except BaseException as error: failures.append(error)
    finally:
        for sig, handler in handlers.items(): signal.signal(sig, handler)
    if failures: raise RuntimeError('Rollback incomplete; use password authentication and review managed files')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['enable', 'disable', 'status'])
    parser.add_argument('--uid', type=int)
    args = parser.parse_args()
    if os.geteuid() != 0: parser.error('Run with sudo')
    meta = protected(PAM)
    original = PAM.read_bytes()
    text = original.decode('utf-8')
    if args.action == 'status':
        print('enabled' if MARKER + LINE in text else 'disabled')
        return
    bump_epoch()
    policy = check_password_policy('system-auth') if args.action == 'enable' else None
    if args.action == 'enable':
        if args.uid is None or args.uid < 1000: parser.error('--uid must identify an enrolled normal user')
        protected(HELPER)
        protected('/usr/lib64/security/pam_linuxhello.so')
        protected(ROOT / str(args.uid) / 'config.json', True)
        print('Look straight at the camera: checking the reference before enabling sudo.', flush=True)
        subprocess.run([HELPER, 'diagnose', str(args.uid)], check=True, timeout=40, env=child_environment(), **child_lock_options())
    if policy and policy[0].read_bytes() != policy[1]: raise RuntimeError('Password policy changed during verification')
    updated = changed(text, args.action == 'enable').encode()
    if updated == original:
        print('No changes needed')
        return
    protected(ROOT / '.anchor')
    backup = ROOT / ('sudo-before-' + hashlib.sha256(original).hexdigest()[:16] + '.pam')
    if not backup.exists(): atomic_write(backup, original, 0o600)
    # Refuse replacement if somebody changed the stack during the live scan.
    protected(PAM)
    if PAM.read_bytes() != original: raise RuntimeError('sudo PAM changed during verification')
    atomic_write(PAM, updated, stat.S_IMODE(meta.st_mode))
    subprocess.run(['/usr/sbin/restorecon', str(PAM)], check=True)
    print('sudo face authentication ' + args.action + 'd; original password/account/session stack retained')

if __name__ == '__main__':
    if os.geteuid() != 0: raise SystemExit('Run with sudo')
    with setup_lock(), write_transaction():
        main()
