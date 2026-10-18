"""Real filesystem fault/recovery tests with synthetic bytes and no privileges."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SOURCE = Path(__file__).parents[1] / 'packaging/sudo/operation-journal.py'
spec = importlib.util.spec_from_file_location('operation_test', SOURCE)
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)


def read(path):
    if not path.exists() and not path.is_symlink(): return None
    meta = path.lstat()
    if path.is_symlink() or not path.is_file(): raise RuntimeError('unsafe test target')
    return path.read_bytes(), meta.st_mode & 0o7777


def replace(path, before):
    if before is None: path.unlink(missing_ok=True)
    else:
        m.write(path, before[0]); path.chmod(before[1])


class OperationJournalTests(unittest.TestCase):
    def fixture(self, folder):
        a=folder/'a'; b=folder/'b'
        replace(a,(b'synthetic-old-a',0o600));replace(b,(b'synthetic-old-b',0o600))
        planned={a:(b'synthetic-new-a',0o600),b:None}
        return a,b,planned,m.Journal(folder/'.operation',planned,read,replace)

    def test_process_death_mid_publish_recovers_old_state(self):
        with tempfile.TemporaryDirectory() as tmp:
            a,b,planned,journal=self.fixture(Path(tmp))
            code='''import importlib.util,os,sys
from pathlib import Path
s=importlib.util.spec_from_file_location('j',sys.argv[1]);m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
f=Path(sys.argv[2]);a=f/'a';b=f/'b'
def read(p): return (p.read_bytes(),p.stat().st_mode&0o7777) if p.exists() else None
j=m.Journal(f/'.operation',[a,b],read,lambda *args:None)
j.begin({a:(b'synthetic-new-a',0o600),b:None})
m.write(a,b'synthetic-new-a');os._exit(79)
'''
            p=subprocess.run([sys.executable,'-B','-c',code,str(SOURCE),tmp],check=False)
            self.assertEqual(p.returncode,79)
            journal.rollback()
            self.assertEqual(a.read_bytes(),b'synthetic-old-a');self.assertEqual(b.read_bytes(),b'synthetic-old-b')
            self.assertFalse(journal.path.exists())

    def test_admin_content_mode_or_absence_change_is_not_overwritten(self):
        for edit in ['bytes','mode','missing']:
            with tempfile.TemporaryDirectory() as tmp:
                a,b,planned,journal=self.fixture(Path(tmp));journal.begin(planned)
                replace(a,planned[a]);replace(b,None)
                if edit=='bytes':a.write_bytes(b'admin-change')
                elif edit=='mode':a.chmod(0o640)
                else:a.unlink()
                before=read(a)
                with self.assertRaisesRegex(RuntimeError,'administrator change'):journal.rollback()
                self.assertEqual(read(a),before);self.assertFalse(b.exists());self.assertTrue(journal.path.exists())

    def test_unknown_subprocess_postimage_requires_exact_reviewed_state(self):
        with tempfile.TemporaryDirectory() as tmp:
            a,b,planned,journal=self.fixture(Path(tmp));journal.begin()
            a.write_bytes(b'synthetic-subprocess-output')
            review=journal.inspect()['expected_state']
            with self.assertRaises(RuntimeError):journal.rollback()
            b.write_bytes(b'concurrent-admin-change')
            with self.assertRaisesRegex(RuntimeError,'since recovery review'):journal.rollback(review)
            journal.rollback(journal.inspect()['expected_state'])
            self.assertEqual(a.read_bytes(),b'synthetic-old-a');self.assertEqual(b.read_bytes(),b'synthetic-old-b')

    def test_prepare_fsync_failures_do_not_publish_or_mutate_targets(self):
        for fail in range(1,8):
            with tempfile.TemporaryDirectory() as tmp:
                a,b,planned,journal=self.fixture(Path(tmp));real=m.sync;calls=0
                def sync(path):
                    nonlocal calls
                    calls+=1
                    if calls==fail:raise OSError('injected fsync failure')
                    real(path)
                with patch.object(m,'sync',side_effect=sync):
                    try:journal.begin(planned)
                    except OSError:pass
                self.assertEqual(a.read_bytes(),b'synthetic-old-a');self.assertEqual(b.read_bytes(),b'synthetic-old-b')
                if journal.path.exists():journal.rollback()

    def test_oversized_index_is_refused_before_active_publication(self):
        with tempfile.TemporaryDirectory() as tmp:
            a,b,planned,journal=self.fixture(Path(tmp))
            with self.assertRaisesRegex(RuntimeError,'recovery limit'):
                journal.begin(planned,{'oversized':'x'*131072})
            self.assertFalse(journal.path.exists());self.assertEqual(a.read_bytes(),b'synthetic-old-a')
            journal.cleanup_prepared()

    def test_cleanup_only_removes_unpublished_prepare_state(self):
        with tempfile.TemporaryDirectory() as tmp:
            a,b,planned,journal=self.fixture(Path(tmp))
            with patch.object(m,'sync',side_effect=OSError('prepare fsync failure')):
                with self.assertRaises(OSError):journal.begin(planned)
            self.assertFalse(journal.path.exists());self.assertTrue(journal.prepared.exists())
            journal.cleanup_prepared()
            self.assertEqual(a.read_bytes(),b'synthetic-old-a')
            journal.begin(planned)
            with self.assertRaisesRegex(RuntimeError,'Active operation'):journal.cleanup_prepared()
            self.assertTrue(journal.path.exists());journal.rollback()

    def test_review_digest_binds_the_journal_not_just_current_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            a,b,planned,journal=self.fixture(Path(tmp));journal.begin()
            a.write_bytes(b'subprocess-result');review=journal.inspect()['expected_state']
            index=journal.path/'index.json';doc=json.loads(index.read_bytes());doc['context']={'changed':True}
            m.write(index,json.dumps(doc).encode())
            with self.assertRaisesRegex(RuntimeError,'since recovery review'):journal.rollback(review)
            self.assertEqual(a.read_bytes(),b'subprocess-result')

    def test_corrupt_preimage_or_target_injection_refuses_all_recovery(self):
        for tamper in ['preimage','path','hardlink']:
            with tempfile.TemporaryDirectory() as tmp:
                a,b,planned,journal=self.fixture(Path(tmp));journal.begin(planned);replace(a,planned[a])
                if tamper=='preimage':(journal.path/'0.before').write_bytes(b'corrupt')
                elif tamper=='hardlink':os.link(journal.path/'0.before',Path(tmp)/'alias')
                else:
                    p=journal.path/'index.json';doc=json.loads(p.read_bytes());doc['entries'][0]['path']='/etc/unmanaged';m.write(p,json.dumps(doc).encode())
                with self.assertRaises(RuntimeError):journal.rollback()
                self.assertEqual(a.read_bytes(),b'synthetic-new-a')

    def test_interrupted_retired_cleanup_never_rolls_back_committed_state(self):
        with tempfile.TemporaryDirectory() as tmp:
            a,b,planned,journal=self.fixture(Path(tmp));journal.begin(planned);replace(a,planned[a])
            with patch.object(journal,'cleanup_retired',side_effect=OSError('injected cleanup failure')):
                self.assertFalse(journal.commit())
            self.assertFalse(journal.path.exists());self.assertTrue(journal.retired.exists())
            journal.cleanup_retired()
            self.assertEqual(a.read_bytes(),b'synthetic-new-a');self.assertFalse(journal.retired.exists())
