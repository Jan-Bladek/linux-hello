#!/usr/bin/python3
"""Root-private durable preimages and conservative recovery of fixed managed paths.
No secret bytes are included in metadata, exceptions or inspect output. The caller
must hold the validated provisioning lock and validate the exact target allowlist.
"""
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import tempfile

MAX_FILE = 100_000_000
MAX_ENTRIES = 128


def sync(path):
    fd = os.open(path, os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW)
    try: os.fsync(fd)
    finally: os.close(fd)


def private(path, directory=False):
    meta = path.lstat()
    kind = stat.S_ISDIR if directory else stat.S_ISREG
    if (not kind(meta.st_mode) or meta.st_uid != os.geteuid() or meta.st_mode & 0o077
            or (not directory and meta.st_nlink != 1)):
        raise RuntimeError('Unsafe operation journal')
    return meta


def write(path, data):
    fd, pending = tempfile.mkstemp(prefix='.pending-', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data); stream.flush(); os.fchmod(stream.fileno(), 0o600); os.fsync(stream.fileno())
        os.replace(pending, path); sync(path.parent)
    finally: Path(pending).unlink(missing_ok=True)


def fingerprint(snapshot):
    return None if snapshot is None else {'sha256': hashlib.sha256(snapshot[0]).hexdigest(), 'mode': snapshot[1]}


class Journal:
    def __init__(self, path, allowed, read, replace):
        self.path = Path(path); self.allowed = {str(p) for p in allowed}
        self.read = read; self.replace = replace
        self.retired = self.path.with_name(self.path.name + '-retired')
        self.prepared = self.path.with_name(self.path.name + '-prepare')

    def begin(self, planned=None, context=None):
        for path in (self.path, self.prepared, self.retired):
            if path.exists() or path.is_symlink():
                raise RuntimeError('Pending operation recovery required; use your password')
        if not 1 <= len(self.allowed) <= MAX_ENTRIES:
            raise RuntimeError('Invalid operation target count')
        self.prepared.mkdir(mode=0o700)
        entries = []
        # Prepare failures never publish an active marker or mutate targets.
        for index, name in enumerate(sorted(self.allowed)):
            before = self.read(Path(name))
            if before is not None:
                if len(before[0]) > MAX_FILE: raise RuntimeError('Operation preimage exceeds limit')
                write(self.prepared / f'{index}.before', before[0])
            after = None if planned is None or Path(name) not in planned else {'value': fingerprint(planned[Path(name)])}
            entries.append({'path': name, 'before': fingerprint(before), 'after': after})
        index = (json.dumps({'version': 1, 'entries': entries, 'context': context}) + '\n').encode()
        if len(index) > 131072: raise RuntimeError('Operation index exceeds recovery limit')
        write(self.prepared / 'index.json', index)
        sync(self.prepared)
        os.rename(self.prepared, self.path); sync(self.path.parent)
        return self

    def entries(self):
        private(self.path, True)
        index = self.path / 'index.json'
        if private(index).st_size > 131072: raise RuntimeError('Oversized operation index')
        doc = json.loads(index.read_bytes())
        if set(doc) != {'version', 'entries', 'context'} or type(doc['version']) is not int or doc['version'] != 1:
            raise RuntimeError('Invalid operation index')
        entries = doc['entries']
        if not isinstance(entries, list) or not 1 <= len(entries) <= MAX_ENTRIES:
            raise RuntimeError('Invalid operation entries')
        if any(not isinstance(e, dict) or not isinstance(e.get('path'), str) for e in entries):
            raise RuntimeError('Invalid operation target')
        names = {e['path'] for e in entries}
        if not names <= self.allowed or len(entries) != len(names):
            raise RuntimeError('Operation targets differ from fixed allowlist')
        for index, entry in enumerate(entries):
            if set(entry) != {'path', 'before', 'after'}: raise RuntimeError('Invalid operation entry')
            for value in [entry['before'], *( [entry['after'].get('value')] if entry['after'] is not None else [])]:
                if value is not None and (set(value) != {'sha256', 'mode'} or type(value['mode']) is not int
                        or not 0 <= value['mode'] <= 0o7777 or not isinstance(value['sha256'], str)
                        or len(value['sha256']) != 64 or any(c not in '0123456789abcdef' for c in value['sha256'])):
                    raise RuntimeError('Invalid operation fingerprint')
            if entry['after'] is not None and set(entry['after']) != {'value'}: raise RuntimeError('Invalid operation intent')
            if entry['before'] is not None:
                backup = self.path / f'{index}.before'
                if private(backup).st_size > MAX_FILE: raise RuntimeError('Oversized operation preimage')
                if hashlib.sha256(backup.read_bytes()).hexdigest() != entry['before']['sha256']:
                    raise RuntimeError('Corrupt operation preimage')
        return entries

    def inspect(self):
        entries = self.entries()
        state = [{'path': e['path'], 'current': fingerprint(self.read(Path(e['path'])))} for e in entries]
        index_digest = hashlib.sha256((self.path / 'index.json').read_bytes()).hexdigest()
        digest = hashlib.sha256(json.dumps({'journal': index_digest, 'current': state}, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        files = [{**item, 'before': entry['before'], 'planned_after': entry['after']} for item, entry in zip(state, entries)]
        return {'expected_state': digest, 'journal_sha256': index_digest, 'files': files, 'recovery_required': True}

    def rollback(self, expected_state=None, finalize=True):
        entries = self.entries()
        current = {e['path']: fingerprint(self.read(Path(e['path']))) for e in entries}
        if expected_state is not None:
            if self.inspect()['expected_state'] != expected_state:
                raise RuntimeError('Managed state changed since recovery review')
        else:
            # Preflight all paths before restoring even one. Unknown postimages,
            # including interrupted external commands, require explicit review.
            for entry in entries:
                now = current[entry['path']]
                if now != entry['before'] and (entry['after'] is None or now != entry['after']['value']):
                    raise RuntimeError('Unrecorded or administrator change; recovery review required')
        for index, entry in reversed(list(enumerate(entries))):
            path = Path(entry['path'])
            if fingerprint(self.read(path)) != current[entry['path']]:
                raise RuntimeError('Managed file changed during recovery')
            if current[entry['path']] == entry['before']: continue
            before = entry['before']
            data = None if before is None else ((self.path / f'{index}.before').read_bytes(), before['mode'])
            self.replace(path, data)
            sync(path.parent)
        if finalize: self.commit()

    def context(self):
        self.entries()
        return json.loads((self.path / 'index.json').read_bytes())['context']

    def commit(self):
        # This rename is the durable commit point. A retired journal never rolls
        # back a committed operation, even if backup cleanup is interrupted.
        os.rename(self.path, self.retired)
        try: sync(self.path.parent)
        except OSError:
            # A commit was not acknowledged. Republish the blocking marker
            # before propagating a durability failure to the caller.
            os.rename(self.retired, self.path); sync(self.path.parent)
            raise
        try: self.cleanup_retired()
        except OSError:
            print('Linux Hello: operation committed; retired journal cleanup is pending.', file=sys.stderr)
            return False
        return True

    def cleanup_retired(self):
        if not self.retired.exists(): return
        private(self.retired, True)
        for path in self.retired.iterdir():
            private(path)
            if path.name != 'index.json' and not (path.name.endswith('.before') and path.stem.isdecimal()):
                raise RuntimeError('Unknown retired operation file')
        for path in self.retired.iterdir(): path.unlink()
        sync(self.retired); self.retired.rmdir(); sync(self.retired.parent)

    def cleanup_prepared(self):
        if self.path.exists() or self.path.is_symlink():
            raise RuntimeError('Active operation requires recovery, not prepare cleanup')
        private(self.prepared, True)
        for path in self.prepared.iterdir():
            private(path)
            if (path.name != 'index.json' and not path.name.startswith('.pending-')
                    and not (path.name.endswith('.before') and path.stem.isdecimal())):
                raise RuntimeError('Unknown prepared operation file')
        for path in self.prepared.iterdir(): path.unlink()
        sync(self.prepared);self.prepared.rmdir();sync(self.prepared.parent)
