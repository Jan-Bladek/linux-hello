#!/usr/bin/python3
"""Build the complete Fedora application unprivileged; install fixed files via sudo."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib

REPO = Path(__file__).resolve().parents[2]
PACKAGES = ['git', 'gcc', 'gcc-c++', 'clang-devel', 'pkgconf-pkg-config', 'opencv-devel',
            'pam-devel', 'systemd-devel', 'dbus-devel', 'dbus-daemon', 'fontconfig-devel',
            'freetype-devel', 'libxkbcommon-devel', 'wayland-devel', 'libX11-devel',
            'libXcursor-devel', 'libXi-devel', 'libXrandr-devel', 'mesa-libGL-devel',
            'python3', 'sudo', 'systemd', 'polkit', 'checkpolicy', 'policycoreutils', 'policycoreutils-python-utils',
            'selinux-policy-devel']
BINS = ['hello', 'hello-settings', 'hello-face-worker', 'hello-sudo', 'hello-lock-verify']
SCRIPTS = {'sudo': ['manage.py', 'enroll-and-enable.sh', 'operation-journal.py', 'recover-operation.py'],
           'kde': ['manage-kde.py', 'activate-kde.sh', 'activate-lock.sh', 'activate-desktop.py', 'configure-person.py', 'manage-person.py']}


def run(args, **kwargs):
    print('+ ' + ' '.join(str(arg) for arg in args), flush=True)
    subprocess.run([str(arg) for arg in args], cwd=REPO, check=True, **kwargs)


def fedora(path=Path('/etc/os-release')):
    values = {}
    for line in path.read_text().splitlines():
        if '=' in line:
            key, value = line.split('=', 1)
            values[key] = value.strip('"')
    if values.get('ID') != 'fedora' or int(values.get('VERSION_ID', '0')) < 44:
        raise RuntimeError('This installer supports Fedora 44 or later; no system changes were made.')
    return values


def models_valid(path):
    spec = importlib.util.spec_from_file_location('face_models', REPO / 'tools/fetch-face-models.py')
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    for _, name, size, digest in module.ARTIFACTS:
        file = path / name
        if not file.is_file() or file.is_symlink() or file.stat().st_size != size:
            return False
        if hashlib.sha256(file.read_bytes()).hexdigest() != digest:
            return False
    return True


def dependencies():
    # Keep Fedora's existing full/minimal curl variant instead of conflicting
    # with curl-minimal on a fresh workstation.
    variant = 'curl-minimal'
    if Path('/usr/bin/curl').exists():
        installed = subprocess.check_output(['/usr/bin/rpm', '-qf', '--qf', '%{NAME}', '/usr/bin/curl'], text=True)
        if installed in ('curl', 'curl-minimal'): variant = installed
    return [*PACKAGES, variant]


def rust(offline):
    channel = tomllib.loads((REPO / 'rust-toolchain.toml').read_text())['toolchain']['channel']
    rustup = shutil.which('rustup')
    local = Path.home() / '.cargo/bin/rustup'
    if not rustup and local.is_file(): rustup = str(local)
    if not rustup:
        if offline: raise RuntimeError('Missing rustup; offline mode cannot install a toolchain.')
        with tempfile.TemporaryDirectory(prefix='linux-hello-rustup-') as tmp:
            script = Path(tmp) / 'rustup-init.sh'
            run(['/usr/bin/curl', '--proto', '=https', '--tlsv1.2', '--fail', '--location',
                 '--silent', '--show-error', 'https://sh.rustup.rs', '--output', script])
            run(['/bin/sh', script, '-y', '--profile', 'minimal', '--no-modify-path', '--default-toolchain', 'none'])
        rustup = str(local)
    installed = subprocess.check_output([rustup, 'toolchain', 'list'], text=True)
    if not any(line.startswith(channel + '-') for line in installed.splitlines()):
        if offline: raise RuntimeError(f'Missing toolchain {channel} in offline mode.')
        run([rustup, 'toolchain', 'install', channel, '--profile', 'minimal', '--component', 'rustfmt', '--component', 'clippy'])
    return [rustup, 'run', channel, 'cargo']


def stage_files(stage, models, with_tpm):
    for name in BINS: shutil.copyfile(REPO / 'target/release' / name, stage / name)
    shutil.copyfile(REPO / 'target/release/libhello_pam.so', stage / 'pam_linuxhello.so')
    for folder, names in SCRIPTS.items():
        for name in names: shutil.copyfile(REPO / 'packaging' / folder / name, stage / name)
    (stage / 'models').mkdir()
    notices = ['provenance.json', 'face_detection_yunet-LICENSE', 'face_detection_yunet-README.md',
               'face_recognition_sface-LICENSE', 'face_recognition_sface-README.md']
    for name in ['face_detection_yunet_2023mar.onnx', 'face_recognition_sface_2021dec.onnx', *notices]:
        file = models / name
        if file.is_file() and not file.is_symlink(): shutil.copyfile(file, stage / 'models' / name)
    run(['/usr/bin/cc', '-Wall', '-Wextra', '-Werror', '-O2', '-D_FORTIFY_SOURCE=3',
         '-fstack-protector-strong', '-Wl,-z,relro,-z,now', '-o', stage / 'kde-pam-probe',
         REPO / 'packaging/kde/pam-probe.c', '-Wl,-l:libpam.so.0'])
    for name in ['linuxhello_kde'] + (['linuxhello_tpm'] if with_tpm else []):
        folder = 'kde' if name.endswith('kde') else 'tpm'
        run(['/usr/bin/checkmodule', '-M', '-m', '-o', stage / (name + '.mod'),
             REPO / 'packaging' / folder / (name + '.te')])
        run(['/usr/bin/semodule_package', '-o', stage / (name + '.pp'), '-m', stage / (name + '.mod')])
        (stage / (name + '.mod')).unlink()
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip() if (REPO / '.git').exists() else 'source-archive'
    if (REPO / '.git').exists() and subprocess.check_output(['git', 'status', '--porcelain'], cwd=REPO): commit += '-dirty'
    files = {str(p.relative_to(stage)): hashlib.sha256(p.read_bytes()).hexdigest()
             for p in stage.rglob('*') if p.is_file()}
    (stage / 'manifest.json').write_text(json.dumps({'version': 1, 'commit': commit, 'files': files}, indent=2) + '\n')
    spec = importlib.util.spec_from_file_location('install_root_validation', REPO / 'packaging/installer/install-root.py')
    validator = importlib.util.module_from_spec(spec); spec.loader.exec_module(validator)
    validator.read_bundle(stage, os.getuid())


def desktop():
    folder = Path.home() / '.local/share/applications'
    folder.mkdir(parents=True, exist_ok=True)
    path = folder / 'linux-hello.desktop'
    if path.is_symlink(): raise RuntimeError('Desktop shortcut is a symlink and was left unchanged. Run linux-hello.')
    content = (REPO / 'packaging/installer/linux-hello.desktop').read_text()
    if path.exists() and path.read_text() != content:
        backup = folder / 'linux-hello.desktop.before-install'
        if not backup.exists(): shutil.copyfile(path, backup)
    path.write_text(content)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dry-run', action='store_true', help='Show the plan without downloads, compilation or changes.')
    parser.add_argument('--build-only', action='store_true', help='Build and verify without installing application files.')
    parser.add_argument('--skip-deps', action='store_true', help='Skip dnf; dependencies must already be installed.')
    parser.add_argument('--offline', action='store_true', help='No network; requires cached models, crates and toolchain.')
    parser.add_argument('--with-tpm', action='store_true', default=True, help='Include TPM SELinux policy (default); existing keys are not migrated.')
    args = parser.parse_args()
    if os.geteuid() == 0: raise RuntimeError('Run ./install.sh as your regular user, without sudo. The installer requests authorization when needed.')
    os.umask(0o077)
    fedora()
    if Path('/run/ostree-booted').exists(): raise RuntimeError('This installer does not support Fedora Atomic/ostree.')
    print('Linux Hello — full Fedora installation\nBuild: Rust 1.99.0, GUI, YuNet/SFace, sudo and desktop PAM.\nInstallation does not activate PAM or modify enrolled references.')
    if args.offline and not args.skip_deps: raise RuntimeError('--offline requires --skip-deps so dnf does not access the network.')
    if args.dry_run:
        print('1. Dependencies: ' + ('skipped' if args.skip_deps else 'dnf install ' + ' '.join(PACKAGES) + ' curl/curl-minimal'))
        print('2. Pinned toolchain, tests and --locked compilation as your regular user.')
        print('3. Verify model hashes; download the pinned artifacts if needed.')
        print('4. ' + ('Private staging only.' if args.build_only else 'Sudo authorization, atomic installation, SELinux and desktop shortcut.'))
        print('5. Open Linux Hello and choose face authentication uses in the GUI.')
        return
    if not args.skip_deps: run(['/usr/bin/sudo', '/usr/bin/dnf', 'install', '-y', *dependencies()])
    cargo = rust(args.offline)
    network = ['--offline'] if args.offline else []
    run([*cargo, 'test', '--locked', *network, '-p', 'hello-sudo', '-p', 'hello-pam', '-p', 'hello-settings',
         '--features', 'hello-sudo/sudo-face,hello-pam/kde-pam,hello-settings/face-diagnostics'])
    run([sys.executable, REPO / 'tools/check-docs.py'])
    run([*cargo, 'build', '--release', '--locked', *network, '-p', 'hello-cli', '-p', 'hello-sudo', '-p', 'hello-pam', '-p', 'hello-settings',
         '--features', 'hello-cli/face-opencv,hello-sudo/sudo-face,hello-pam/kde-pam,hello-settings/face-diagnostics'])
    candidates = [REPO / 'work/inference/models', Path('/usr/local/libexec/linux-hello-sudo/models')]
    candidates += sorted((REPO / 'work/install').glob('model-download-*/models'))
    models = next((path for path in candidates if models_valid(path)), REPO / 'work/inference/models')
    if not models_valid(models):
        if args.offline: raise RuntimeError('No valid cached models available (offline mode).')
        root = REPO / 'work/install'; root.mkdir(parents=True, exist_ok=True)
        fresh = Path(tempfile.mkdtemp(prefix='model-download-', dir=root)) / 'models'
        run([sys.executable, REPO / 'tools/fetch-face-models.py', '--output', fresh])
        models = fresh
    root = REPO / 'work/install'; root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='stage-', dir=root) as tmp:
        stage = Path(tmp)
        stage_files(stage, models, args.with_tpm)
        if args.build_only:
            print('Ready: full compilation, tests, models and staging verified. No application files were installed.')
            return
        run(['/usr/bin/sudo', '/usr/bin/python3', '-B', REPO / 'packaging/installer/install-root.py', stage])
    desktop()
    print('Ready! Open Linux Hello from your application launcher or run linux-hello.\nScan → name → select uses → Save and set up.\nUse a separate reference passphrase; system authorization uses your account password.')
    if args.with_tpm: print('New system profiles require TPM wrapping. Existing file-key profiles require an explicit tpm-enable migration; copied historical keys are not erased.')


if __name__ == '__main__':
    try: main()
    except (RuntimeError, OSError, subprocess.CalledProcessError) as error:
        print('Installation stopped: ' + str(error), file=sys.stderr); sys.exit(1)
