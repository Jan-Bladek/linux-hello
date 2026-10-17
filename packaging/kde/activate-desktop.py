#!/usr/bin/python3
"""Unprivileged desktop selection and Noctalia UI configuration.
This code never supplies a face verdict or writes PAM/protected enrollment.
"""
import argparse
import os
from pathlib import Path
import pwd
import subprocess
import tempfile
import tomllib

BASE = Path('/usr/local/libexec/linux-hello-sudo')
OVERLAY = '# Linux Hello: submit an empty field to start PAM; PAM still authenticates.\n[lockscreen]\nallow_empty_password = true\n'


def detect():
    # Advisory UI default only. Root independently binds authorization to UID,
    # validates a fixed service whitelist and performs both real PAM checks.
    for process in Path('/proc').iterdir():
        if not process.name.isdecimal():
            continue
        try:
            if process.stat().st_uid == os.getuid() and (process / 'exe').readlink().name == 'noctalia':
                return 'login'
        except (OSError, PermissionError):
            continue
    desktop = os.environ.get('XDG_CURRENT_DESKTOP', '').lower().split(':')
    return 'hyprlock' if 'hyprland' in desktop else 'kde'


def configure_noctalia():
    if os.getuid() < 1000 or os.geteuid() != os.getuid():
        raise RuntimeError('Run Noctalia configuration as your desktop account, without sudo.')
    version = subprocess.check_output(['/usr/bin/noctalia', '--version'], text=True, timeout=5).strip()
    if not version.split() or not version.split()[-1].removeprefix('v').startswith('5.'):
        raise RuntimeError('This adapter requires Noctalia 5.x; older PAM interfaces require review.')
    result = subprocess.run(['/usr/bin/noctalia', 'config', 'export', 'full'],
                            capture_output=True, text=True, timeout=10, check=True)
    if tomllib.loads(result.stdout).get('lockscreen', {}).get('allow_empty_password') is True:
        return
    home = Path(os.environ.get('NOCTALIA_CONFIG_HOME') or os.environ.get('XDG_CONFIG_HOME') or Path.home() / '.config')
    if not home.is_absolute():
        raise RuntimeError('Noctalia configuration home must be absolute.')
    folder = home / 'noctalia'
    folder.mkdir(parents=True, exist_ok=True)
    target = folder / 'linux-hello.toml'
    if target.is_symlink() or (target.exists() and target.read_text() != OVERLAY):
        raise RuntimeError('Existing linux-hello.toml has local changes; enable allow_empty_password in Noctalia settings.')
    existed = target.exists()
    # Only this owned overlay is modified. Never rewrite the shell's settings
    # or enable grace-period/passwordless unlock. Atomic publication, mode0600.
    with tempfile.NamedTemporaryFile(mode='w', dir=folder, delete=False) as stream:
        temporary = Path(stream.name)
        try:
            stream.write(OVERLAY); stream.flush(); os.fsync(stream.fileno())
            os.replace(temporary, target)
        finally:
            temporary.unlink(missing_ok=True)
    try:
        subprocess.run(['/usr/bin/noctalia', 'config', 'validate'], check=True,
                       stdout=subprocess.DEVNULL, timeout=10)
        result = subprocess.run(['/usr/bin/noctalia', 'config', 'export', 'full'],
                                capture_output=True, text=True, timeout=10, check=True)
        if tomllib.loads(result.stdout).get('lockscreen', {}).get('allow_empty_password') is not True:
            raise RuntimeError('Noctalia settings override this option. Enable Allow empty password in Noctalia Security settings.')
    except BaseException:
        if not existed and not target.is_symlink() and target.read_text() == OVERLAY:
            target.unlink()
        raise


def main():
    parser = argparse.ArgumentParser(description='Enable Linux Hello for your desktop screen locker.')
    parser.add_argument('--service', choices=('auto', 'noctalia', 'hyprlock', 'kde'), default='auto')
    parser.add_argument('--detect', action='store_true')
    parser.add_argument('--configure-noctalia', action='store_true')
    args = parser.parse_args()
    if os.getuid() < 1000 or os.geteuid() != os.getuid():
        parser.error('Run as the normal desktop account, without sudo.')
    if args.detect:
        print(detect()); return
    if args.configure_noctalia:
        configure_noctalia(); return
    service = detect() if args.service == 'auto' else {'noctalia': 'login'}.get(args.service, args.service)
    print('Close camera preview and face the camera. Sudo authorization comes first, then two real PAM checks.', flush=True)
    subprocess.run(['/usr/bin/sudo', '--', str(BASE / 'manage-kde.py'), 'enable',
                    '--service', service, '--user', pwd.getpwuid(os.getuid()).pw_name], check=True)
    if service == 'login':
        configure_noctalia()
    print('Ready. On the lock screen, submit the empty password field and look at the camera. Password fallback remains available.')


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, subprocess.SubprocessError) as error:
        raise SystemExit(f'Linux Hello: {error}')
