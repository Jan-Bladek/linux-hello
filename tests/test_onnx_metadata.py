import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("onnx_headers", Path(__file__).resolve().parents[1] / "tools/inspect-face-models.py")
headers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(headers)


class OnnxMetadataTests(unittest.TestCase):
    def test_malformed_headers_are_bounded_and_rejected(self):
        for data in (b"\x00", b"\x0a\xff", b"\x0a\x09x", b"\x08" + b"\xff" * 10, b"\x0b"):
            with self.subTest(data=data), self.assertRaises(ValueError):
                list(headers.fields(data))

    def test_basic_wire_types_and_empty_graph(self):
        self.assertEqual(list(headers.fields(b"\x08\x07")), [(1, 0, 7)])
        with self.assertRaises(ValueError):
            headers.inspect(b"")


if __name__ == "__main__":
    unittest.main()
