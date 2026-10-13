import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('check_docs', Path(__file__).parents[1] / 'tools/check-docs.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class DocumentationLinks(unittest.TestCase):
    def test_links_images_and_heading_fragments(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'target.md').write_text('# Héllo, `World`!\n# Héllo, `World`!\n')
            (root / 'image.png').write_bytes(b'fixture')
            source = root / 'source.md'
            source.write_text('[one](target.md#héllo-world)\n[two](target.md#héllo-world-1)\n![image](image.png)\n')
            self.assertEqual(m.check_file(source, root), [])
            source.write_text('[bad](target.md#missing)\n[missing](absent.md)\n')
            self.assertEqual(len(m.check_file(source, root)), 2)

    def test_fenced_examples_and_external_links_are_not_opened(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'source.md'
            source.write_text('```sh\n[example](absent.md)\n```\n[external](https://example.invalid/x#missing)\n')
            self.assertEqual(m.check_file(source, root), [])

    def test_source_archive_discovers_docs_without_scanning_build_or_cache(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ['README.md', 'docs/install.md', 'tests/README.md', 'packaging/notes.md',
                         'work/private.md', 'vendor/crate/README.md', 'target/build.md']:
                path = root / name; path.parent.mkdir(parents=True, exist_ok=True); path.write_text('text')
            self.assertEqual(set(m.markdown_names(root)), {'README.md', 'docs/install.md', 'tests/README.md', 'packaging/notes.md'})
