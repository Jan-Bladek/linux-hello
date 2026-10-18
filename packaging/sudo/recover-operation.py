#!/usr/bin/python3
"""Explicit administrator recovery of the fixed account-operation journal.
Never automatically guesses the intent of an unrecorded subprocess/admin edit.
"""
import argparse
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import resource
import stat

BASE = Path('/usr/local/libexec/linux-hello-sudo')
ROOT = Path('/var/lib/linux-hello/sudo')


def trusted(path):
    for parent in reversed(path.parents):
        meta = parent.lstat()
        if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != 0 or meta.st_mode & 0o022:
            raise RuntimeError('Unsafe recovery code ancestor')
    meta = path.lstat()
    if not stat.S_ISREG(meta.st_mode) or meta.st_uid != 0 or meta.st_nlink != 1 or meta.st_mode & 0o022:
        raise RuntimeError('Unsafe recovery code')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['inspect','restore','cleanup-prepared'])
    parser.add_argument('--expected-state', help='Exact fingerprint from a separately reviewed inspect; permits restoring unrecorded changes')
    args=parser.parse_args()
    if os.getuid()!=0 or os.geteuid()!=0: raise RuntimeError('Explicit administrator authorization required')
    if args.expected_state is not None and (len(args.expected_state)!=64 or any(c not in '0123456789abcdef' for c in args.expected_state)):
        raise RuntimeError('Invalid expected state fingerprint')
    resource.setrlimit(resource.RLIMIT_CORE,(0,0));os.umask(0o077)
    for path in [Path(__file__).absolute(), BASE/'manage.py', BASE/'operation-journal.py']: trusted(path)
    spec=importlib.util.spec_from_file_location('hello_recovery_management', BASE/'manage.py')
    sudo=importlib.util.module_from_spec(spec);spec.loader.exec_module(sudo)
    # Only explicit root recovery accepts these fixed services/profile filenames.
    services=('plasmalogin','sddm','kde','hyprlock','login')
    allowed={Path('/etc/pam.d/sudo')}
    for service in services:
        allowed.add(Path('/etc/pam.d')/service)
        allowed.add(Path('/etc/pam.d')/('linuxhello-kde-auth' if service in ('plasmalogin','sddm','kde') else 'linuxhello-'+service+'-auth'))
    for profile in ROOT.iterdir():
        if not profile.name.isdecimal() or not 1000<=int(profile.name)<=2**32-1: continue
        for name in ['key','key.tpm','template.lht','config.json','pam-sudo.disabled','.config-pending',
                     *['pam-'+s+'.enabled' for s in services]]: allowed.add(profile/name)
        if (profile/'.profile-update').exists():
            raise RuntimeError('Recover the interrupted per-profile journal first with hello-sudo recover UID')
    with sudo.setup_lock(recovery=True):
        journal=sudo.operation_journal(ROOT, allowed, {}, sudo.protected, sudo.atomic_write, begin=False)
        if args.action=='inspect': print(json.dumps(journal.inspect(),indent=2))
        elif args.action=='cleanup-prepared':
            journal.cleanup_prepared();print('Unpublished prepare files removed; managed targets were retained.')
        else:
            sudo.bump_epoch()
            journal.rollback(expected_state=args.expected_state)
            print('Previous account configuration restored. No camera, PAM activation or key migration performed.')


if __name__=='__main__':
    try: main()
    except (RuntimeError,OSError,ValueError) as error:
        raise SystemExit('Recovery stopped: '+str(error))
