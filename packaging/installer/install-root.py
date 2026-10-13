#!/usr/bin/python3
"""Fixed-path administrator installation. No PAM/profile activation or key access."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import resource
import signal
import stat
import subprocess
import sys
import tempfile

BASE = Path('/usr/local/libexec/linux-hello-sudo')
STATE = Path('/var/lib/linux-hello/sudo')
PAM = Path('/usr/lib64/security/pam_linuxhello.so')
BIN = Path('/usr/local/bin')
DESKTOP = Path('/usr/local/share/applications/linux-hello.desktop')
MODELS = {'face_detection_yunet_2023mar.onnx': (232589, '8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4'),
          'face_recognition_sface_2021dec.onnx': (38696353, '0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79')}
FILES = {'hello': 0o755, 'hello-settings': 0o755, 'hello-face-worker': 0o755,
         'hello-sudo': 0o755, 'hello-lock-verify': 0o4755, 'pam_linuxhello.so': 0o644,
         'manage.py': 0o755, 'enroll-and-enable.sh': 0o755, 'manage-kde.py': 0o755,
         'activate-kde.sh': 0o755, 'activate-lock.sh': 0o755, 'configure-person.py': 0o755,
         'kde-pam-probe': 0o755, 'linuxhello_kde.pp': 0o644,
         'linuxhello_tpm.pp': 0o644}
NOTICES = {'provenance.json', 'face_detection_yunet-LICENSE', 'face_detection_yunet-README.md',
           'face_recognition_sface-LICENSE', 'face_recognition_sface-README.md'}
DESKTOP_CONTENT = b'[Desktop Entry]\nType=Application\nName=Linux Hello\nComment=Dodawanie twarzy i logowanie do konta\nExec=/usr/local/bin/linux-hello\nIcon=security-high\nTerminal=false\nCategories=Settings;Security;\nStartupNotify=true\n'


def protected(path, private=False):
    if not path.is_absolute() or '..' in path.parts: raise RuntimeError('Unsafe installation path')
    for parent in reversed(path.parents):
        meta = parent.lstat()
        if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != 0 or meta.st_mode & 0o022:
            raise RuntimeError('Unsafe installation ancestor: ' + str(parent))
    try: meta = path.lstat()
    except FileNotFoundError: return None
    forbidden = 0o077 if private else 0o022
    if not stat.S_ISREG(meta.st_mode) or meta.st_uid != 0 or meta.st_nlink != 1 or meta.st_mode & forbidden:
        raise RuntimeError('Unsafe installation file: ' + str(path))
    return meta


def directory(path, mode):
    if path.exists() or path.is_symlink():
        meta = path.lstat()
        if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != 0 or meta.st_mode & 0o022:
            raise RuntimeError('Unsafe installation directory: ' + str(path))
    else:
        directory(path.parent, 0o755)
        path.mkdir(mode=mode)


def atomic(path, contents, mode):
    protected(path)
    fd, pending = tempfile.mkstemp(prefix='.linux-hello-install-', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as output:
            output.write(contents); output.flush(); os.fchmod(output.fileno(), mode); os.fsync(output.fileno())
        os.replace(pending, path)
        sync_dir(path.parent)
    finally:
        Path(pending).unlink(missing_ok=True)


def sync_dir(path):
    fd = os.open(path, os.O_DIRECTORY | os.O_CLOEXEC)
    try: os.fsync(fd)
    finally: os.close(fd)


def read_bundle(stage, owner):
    if not stage.is_absolute() or '..' in stage.parts: raise RuntimeError('Absolute staging path required')
    meta = stage.lstat()
    if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != owner or meta.st_mode & 0o077:
        raise RuntimeError('Staging must be a private directory owned by the installing user')
    def read(relative, maximum):
        path = stage / relative
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)
        try:
            meta = os.fstat(fd)
            if not stat.S_ISREG(meta.st_mode) or meta.st_uid != owner or meta.st_nlink != 1 or meta.st_mode & 0o022 or meta.st_size > maximum:
                raise RuntimeError('Unsafe staging file: ' + relative)
            with os.fdopen(fd, 'rb', closefd=False) as source: data = source.read(maximum + 1)
            if len(data) > maximum: raise RuntimeError('Staging file grew beyond its limit')
            return data
        finally: os.close(fd)
    manifest = json.loads(read('manifest.json', 32768))
    if set(manifest) != {'version', 'commit', 'files'} or type(manifest['version']) is not int or manifest['version'] != 1 or not isinstance(manifest['files'], dict):
        raise RuntimeError('Invalid installation manifest')
    required = (set(FILES) - {'linuxhello_tpm.pp'}) | {'models/' + name for name in MODELS}
    allowed = set(FILES) | {'models/' + name for name in set(MODELS) | NOTICES}
    names = set(manifest['files'])
    if not required <= names <= allowed: raise RuntimeError('Missing or unexpected installation files')
    # Reject directory links before opening their children.
    modelmeta = (stage / 'models').lstat()
    if not stat.S_ISDIR(modelmeta.st_mode) or modelmeta.st_uid != owner or modelmeta.st_mode & 0o022:
        raise RuntimeError('Unsafe model staging directory')
    contents = {}
    for name, digest in manifest['files'].items():
        data = read(name, 100_000_000 if not name.startswith('models/') else 50_000_000)
        if hashlib.sha256(data).hexdigest() != digest: raise RuntimeError('Changed staging file: ' + name)
        if name.startswith('models/') and name[7:] in MODELS:
            size, pinned = MODELS[name[7:]]
            if len(data) != size or digest != pinned: raise RuntimeError('Unpinned face model: ' + name)
        contents[name] = data
    return manifest, contents


def run(args): subprocess.run(args, check=True, env={'LANG': 'C.UTF-8', 'PATH': '/usr/sbin:/usr/bin:/sbin:/bin'})


def install(manifest, contents):
    directory(BASE / 'models', 0o755)
    directory(BIN, 0o755)
    directory(DESKTOP.parent, 0o755)
    directory(STATE, 0o700)
    if STATE.lstat().st_mode & 0o077: raise RuntimeError('State directory must remain root-private')
    lockpath = STATE / '.setup.lock'
    protected(lockpath, True)
    fd = os.open(lockpath, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    with os.fdopen(fd, 'rb') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        updates = {}
        for name, data in contents.items():
            path = PAM if name == 'pam_linuxhello.so' else BASE / name
            updates[path] = (data, 0o644 if name.startswith('models/') else FILES[name])
        updates[BIN / 'linux-hello'] = (b'#!/bin/sh\nexec /usr/local/libexec/linux-hello-sudo/hello-settings "$@"\n', 0o755)
        updates[BIN / 'hello'] = (b'#!/bin/sh\nexec /usr/local/libexec/linux-hello-sudo/hello "$@"\n', 0o755)
        updates[DESKTOP] = (DESKTOP_CONTENT, 0o644)
        updates[BASE / 'install-manifest.json'] = ((json.dumps(manifest, indent=2) + '\n').encode(), 0o600)
        if protected(STATE / '.anchor', True) is None: updates[STATE / '.anchor'] = (b'', 0o600)
        snapshots = {}
        for path in updates:
            meta = protected(path)
            snapshots[path] = None if meta is None else (path.read_bytes(), stat.S_IMODE(meta.st_mode))
        modules = subprocess.check_output(['/usr/sbin/semodule', '-l'], text=True).splitlines()
        existing = {line.split()[0] for line in modules if line.split()}
        local_contexts = subprocess.check_output(['/usr/sbin/semanage', 'fcontext', '-l', '-C'], text=True)
        probe = str(BASE / 'kde-pam-probe')
        entries = [line for line in local_contexts.splitlines() if line.split() and line.split()[0] == probe]
        if entries and not all(':xdm_exec_t:' in line for line in entries):
            raise RuntimeError('Existing probe SELinux override requires manual review')
        added_modules = []; added_context = False
        try:
            for path, (data, mode) in updates.items(): atomic(path, data, mode)
            for module in ['linuxhello_kde', 'linuxhello_tpm']:
                if module + '.pp' in contents and module not in existing:
                    run(['/usr/sbin/semodule', '-i', str(BASE / (module + '.pp'))]); added_modules.append(module)
            if not entries:
                run(['/usr/sbin/semanage', 'fcontext', '-a', '-t', 'xdm_exec_t', probe]); added_context = True
            run(['/usr/sbin/restorecon', '-R', str(BASE), str(STATE), str(PAM), str(BIN / 'linux-hello'), str(BIN / 'hello'), str(DESKTOP)])
            for path in {p.parent for p in updates}: sync_dir(path)
        except BaseException:
            for path, before in snapshots.items():
                if before is None: path.unlink(missing_ok=True)
                else: atomic(path, before[0], before[1])
            if added_context: run(['/usr/sbin/semanage', 'fcontext', '-d', probe])
            for module in reversed(added_modules): run(['/usr/sbin/semodule', '-r', module])
            for path in snapshots:
                if path.exists(): run(['/usr/sbin/restorecon', str(path)])
            raise
    print('Zainstalowano pełną aplikację. Zachowano PAM, wzorce, klucze i istniejące polityki. Usługi nie zostały uruchomione.')


def main():
    if os.geteuid() != 0 or len(sys.argv) != 2: raise RuntimeError('Administrator install: one staging directory required')
    def cancelled(signum, frame): raise InterruptedError('Installation cancelled; restoring previous files')
    signal.signal(signal.SIGTERM, cancelled)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.umask(0o077)
    owner = int(os.environ.get('SUDO_UID', str(os.getuid())))
    manifest, contents = read_bundle(Path(sys.argv[1]), owner)
    install(manifest, contents)


if __name__ == '__main__':
    try: main()
    except (RuntimeError, OSError, ValueError, subprocess.CalledProcessError) as error:
        print('Instalacja systemowa przerwana: ' + str(error), file=sys.stderr); sys.exit(1)
