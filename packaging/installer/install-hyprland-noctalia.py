#!/usr/bin/python3
"""Install the complete Linux Hello bundle for an existing Hyprland/Noctalia desktop.
Builds through the shared Fedora installer; biometric activation is explicit and
uses the installed managers' real checks, never synthetic face results.
"""
import argparse
import importlib.util
import os
from pathlib import Path
import subprocess
import sys

REPO = Path(__file__).resolve().parents[2]
BASE = Path('/usr/local/libexec/linux-hello-sudo')
BUILD_OPTIONS = ('build_only', 'skip_deps', 'offline', 'with_tpm')


def run(args, **kwargs):
    # No shell or privilege-bearing user-supplied commands. Secrets are entered
    # directly into the existing sudo/enrollment dialogs, never into argv/logs.
    subprocess.run([str(arg) for arg in args], cwd=REPO, check=True, **kwargs)


def check_platform():
    spec = importlib.util.spec_from_file_location('hello_full_installer', REPO / 'packaging/installer/install.py')
    installer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(installer)
    installer.fedora()
    if Path('/run/ostree-booted').exists():
        raise RuntimeError('Fedora Atomic/ostree is not supported by this source installer.')


def check_desktop():
    for name in ('Hyprland', 'noctalia'):
        path = Path('/usr/bin') / name
        if not path.is_file() or not os.access(path, os.X_OK):
            raise RuntimeError(f'Missing {path}. Install your Hyprland / Noctalia desktop first. No external repository is enabled by this installer.')
    version = subprocess.check_output(['/usr/bin/noctalia', '--version'], text=True, timeout=5).strip()
    words = version.split()
    if not words or not words[-1].removeprefix('v').startswith('5.'):
        raise RuntimeError('Noctalia 5.x is required (reviewed on 5.3.0); the older Quickshell interface is not supported.')
    print(f'Desktop check: Hyprland installed; {version}.', flush=True)


def setup_commands(args):
    commands = []
    if args.enroll_sudo:
        commands.append([BASE / 'enroll-and-enable.sh'])
    if args.activate_login:
        commands.append([BASE / 'activate-kde.sh'])
    if args.activate_unlock:
        commands.append([BASE / 'activate-desktop.py', '--service', 'noctalia'])
    return commands


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--dry-run', action='store_true', help='Print the full plan without executing commands or changing files.')
    parser.add_argument('--build-only', action='store_true', help='Compile and stage only; no application installation or activation.')
    parser.add_argument('--skip-deps', action='store_true', help='Skip DNF; native build dependencies must already be installed.')
    parser.add_argument('--offline', action='store_true', help='Use cached Rust/crates/models only; requires --skip-deps.')
    parser.add_argument('--with-tpm', action='store_true', help='Install the optional TPM policy; do not seal/reset/migrate keys.')
    parser.add_argument('--enroll-sudo', action='store_true', help='Explicitly import your previously saved encrypted GUI reference and activate sudo after real face checks.')
    parser.add_argument('--activate-login', action='store_true', help='Activate the existing PlasmaLogin/SDDM service after two real face checks; requires a protected reference.')
    parser.add_argument('--activate-unlock', action='store_true', help='Activate Noctalia unlock after two real face checks, then configure empty-submit PAM initiation; requires a protected reference.')
    parser.add_argument('--open-settings', action='store_true', help='Open the installed GUI after successful installation/setup.')
    args = parser.parse_args(argv)
    if os.getuid() < 1000 or os.geteuid() != os.getuid():
        parser.error('Run as your normal desktop account, without sudo. Authorization is requested only where required.')
    if args.offline and not args.skip_deps:
        parser.error('--offline requires --skip-deps.')
    activate = args.enroll_sudo or args.activate_login or args.activate_unlock
    if args.build_only and (activate or args.open_settings):
        parser.error('--build-only cannot be combined with activation or --open-settings.')
    full = [sys.executable, REPO / 'packaging/installer/install.py']
    full += ['--' + name.replace('_', '-') for name in BUILD_OPTIONS if getattr(args, name)]
    commands = setup_commands(args)
    print('Linux Hello — Hyprland + Noctalia installation', flush=True)
    print('Target: existing mutable Fedora 44+ desktop, Noctalia 5.x. No OS/compositor installation or display-manager replacement.', flush=True)
    print('Build: native dependencies, pinned Rust, locked Cargo packages, verified YuNet/SFace models, GUI, worker, PAM, helpers and SELinux.', flush=True)
    if args.dry_run:
        print('1. Check Fedora, Hyprland and Noctalia version (read-only at execution).')
        print('2. Run the shared full installer: ' + ' '.join(str(item) for item in full))
        if not args.build_only:
            print('3. Validate the installed Noctalia adapter and its own-account execution.')
        for index, command in enumerate(commands, 4):
            print(f'{index}. Explicit live setup: ' + ' '.join(str(item) for item in command))
        if not activate:
            print('No PAM activation selected.' + (' Build/staging only.' if args.build_only else ' First run: scan, name and Save and set up in Linux Hello.'))
        if args.open_settings:
            print('Open /usr/local/bin/linux-hello after installation.')
        print('Never lock/log out/restart the session; never rewrite Hyprland keybindings, shared authselect or other users\' settings.')
        return
    # Check the desktop before DNF, downloads or any application installation.
    check_platform()
    check_desktop()
    run(full)
    if args.build_only:
        print('Build/staging complete. Application files, PAM and session settings were not installed or activated.')
        return
    # Exercise the fixed installed adapter as this ordinary account. No root
    # enrollment is inspected, no camera is opened and this grants no match.
    detected = subprocess.check_output([sys.executable, '-B', str(BASE / 'activate-desktop.py'), '--detect'],
                                       text=True, timeout=10).strip()
    if detected not in ('kde', 'hyprlock', 'login'):
        raise RuntimeError('Installed desktop adapter returned an invalid service.')
    if commands:
        print('Close camera preview and stay facing the camera. Each manager must pass its real checks. Password fallback remains available.', flush=True)
    for command in commands:
        run(command)
    print('Linux Hello installation complete.', flush=True)
    if not activate:
        print('First use: run linux-hello inside your Noctalia session, scan your face, name it, choose uses and select Save and set up.', flush=True)
    if args.activate_unlock:
        print('Noctalia unlock enabled. At your next deliberate lock, submit the empty password field once and face the camera.', flush=True)
    elif not args.enroll_sudo:
        print('Existing protected reference? Enable unlock with /usr/local/libexec/linux-hello-sudo/activate-desktop.py --service noctalia', flush=True)
    print('Hyprland session sign-in uses your existing PlasmaLogin/SDDM; greetd/Noctalia Greeter are not integrated.', flush=True)
    if args.open_settings:
        # Attached launch: errors stay visible; no background process or new
        # service is hidden from the invoking terminal.
        run(['/usr/local/bin/linux-hello'])


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(f'Installation/setup stopped: {error}\nCompleted installation or earlier successful activation steps may remain. Password fallback is retained; no later setup step was run.', file=sys.stderr)
        sys.exit(1)
