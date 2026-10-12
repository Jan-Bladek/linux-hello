"""Descriptor boundaries and the read-only UVC query contract."""
import ctypes
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch
import uuid

spec = importlib.util.spec_from_file_location(
    'uvc_metadata', Path(__file__).resolve().parents[1] / 'tools/inspect-uvc-controls.py')
uvc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(uvc)

GUID = '26b8105a-0713-4870-979d-da79444bb68e'


def interface(number=2, alternate=0):
    return bytes([9, 4, number, alternate, 0, 14, 1, 0, 0])


def extension(unit=13, bitmap=b'\x00\xe0\x00\x00'):
    body = bytes([0x24, 6, unit]) + uuid.UUID(GUID).bytes_le
    body += bytes([3, 1, 10, len(bitmap)]) + bitmap + bytes([0])
    return bytes([len(body) + 1]) + body


class MetadataTests(unittest.TestCase):
    def test_interface_alternate_and_bitmap_scope(self):
        data = interface(0) + extension(7) + interface(2) + extension()
        data += interface(2, 1) + extension(14)
        units = uvc.extension_units(data, 2)
        self.assertEqual(len(units), 1)
        self.assertEqual(units[0]['guid'], GUID)
        self.assertEqual(units[0]['selectors'], [14, 15, 16])

    def test_configuration_boundary_resets_interface(self):
        data = interface() + bytes([9, 2, 9, 0, 0, 1, 0, 0, 0]) + extension()
        self.assertEqual(uvc.extension_units(data, 2), [])

    def test_malformed_descriptors_and_duplicate_units_fail(self):
        for data in (b'\x00\x04', b'\x09\x04', b'\x01',
                     interface() + extension()[:-1],
                     interface() + extension() + extension(),
                     interface() + extension(0)):
            with self.subTest(data=data), self.assertRaises(ValueError):
                uvc.extension_units(data, 2)

    def test_query_requests_never_write_or_read_values(self):
        calls = []

        def ioctl(fd, opcode, query):
            calls.append((query.query, query.size))
            self.assertEqual(fd, 9)
            self.assertEqual(opcode & 0xffff, (ord('u') << 8) | 0x21)
            result = b'\x08\x00' if query.query == 0x85 else b'\x03'
            ctypes.memmove(query.data, result, query.size)
            return 0

        with patch.object(uvc.fcntl, 'ioctl', side_effect=ioctl):
            self.assertEqual(uvc.get_metadata(9, 13, 14, 'length'), 8)
            self.assertEqual(uvc.get_metadata(9, 13, 14, 'info'), 3)
            for request in ('current', 'set', 0x01, 0x81):
                with self.assertRaises(ValueError):
                    uvc.get_metadata(9, 13, 14, request)
        self.assertEqual(calls, [(0x85, 2), (0x86, 1)])

    def test_untrusted_device_paths_rejected_before_open(self):
        with patch.object(uvc.os, 'open') as opened:
            for device in ('/tmp/video2', '/dev/video2/../video0', '/dev/videoX'):
                with self.assertRaises(ValueError):
                    uvc.inspect(device)
            opened.assert_not_called()


if __name__ == '__main__':
    unittest.main()
