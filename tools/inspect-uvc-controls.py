#!/usr/bin/env python3
"""Read declared UVC extension-unit metadata. Never set controls or capture pixels."""
import argparse
import ctypes
import fcntl
import json
import os
from pathlib import Path
import re
import stat
import sys
import uuid

MAX_DESCRIPTORS = 65536
MAX_CONTROLS = 128


class Query(ctypes.Structure):
    _fields_ = [("unit", ctypes.c_uint8), ("selector", ctypes.c_uint8),
                ("query", ctypes.c_uint8), ("size", ctypes.c_uint16),
                ("data", ctypes.c_void_p)]


def extension_units(data, interface):
    """Parse only VC extension descriptors on alternate zero of this interface."""
    units = []
    active = False
    offset = 0
    seen = set()
    while offset < len(data):
        if offset + 2 > len(data):
            raise ValueError("truncated USB descriptor header")
        length = data[offset]
        if length < 2 or offset + length > len(data):
            raise ValueError("invalid USB descriptor length")
        block = data[offset:offset + length]
        kind = block[1]
        if kind == 2:
            active = False
        if kind == 4:
            if length < 9:
                raise ValueError("truncated interface descriptor")
            active = (block[2] == interface and block[3] == 0
                      and block[5:7] == bytes([14, 1]))
        if active and kind == 0x24 and length >= 3 and block[2] == 6:
            if length < 24:
                raise ValueError("truncated extension descriptor")
            unit = block[3]
            pins = block[21]
            size_offset = 22 + pins
            if size_offset >= length:
                raise ValueError("truncated extension sources")
            size = block[size_offset]
            if size_offset + size + 2 != length:
                raise ValueError("invalid extension bitmap length")
            bitmap = block[size_offset + 1:size_offset + 1 + size]
            selectors = [index + 1 for index in range(size * 8)
                         if bitmap[index // 8] & (1 << (index % 8))]
            if not unit or unit in seen or any(s > 255 for s in selectors):
                raise ValueError("invalid or duplicate extension unit")
            seen.add(unit)
            units.append({"unit": unit, "guid": str(uuid.UUID(bytes_le=block[4:20])),
                          "declared_controls": block[20], "selectors": selectors})
        offset += length
    if sum(len(unit["selectors"]) for unit in units) > MAX_CONTROLS:
        raise ValueError("too many extension controls")
    return units


def get_metadata(fd, unit, selector, request):
    # There is intentionally no caller-provided query code, GET_CUR or SET_CUR.
    if request == "length":
        code, size = 0x85, 2
    elif request == "info":
        code, size = 0x86, 1
    else:
        raise ValueError("only GET_LEN and GET_INFO are allowed")
    buffer = (ctypes.c_uint8 * size)()
    query = Query(unit, selector, code, size, ctypes.addressof(buffer))
    opcode = (3 << 30) | (ctypes.sizeof(Query) << 16) | (ord('u') << 8) | 0x21
    fcntl.ioctl(fd, opcode, query)
    return int.from_bytes(bytes(buffer), "little")


def inspect(device):
    if sys.platform != "linux":
        raise ValueError("Linux is required")
    if not re.fullmatch(r"/dev/video[0-9]+", device):
        raise ValueError("expected an explicit /dev/videoN character device")
    interface_path = (Path('/sys/class/video4linux') / Path(device).name / 'device').resolve(strict=True)
    if (interface_path / 'driver').resolve(strict=True).name != 'uvcvideo':
        raise ValueError("selected device is not using uvcvideo")
    interface = int((interface_path / 'bInterfaceNumber').read_text().strip(), 16)
    usb = interface_path.parent
    with (usb / 'descriptors').open('rb') as stream:
        descriptors = stream.read(MAX_DESCRIPTORS + 1)
    if len(descriptors) > MAX_DESCRIPTORS:
        raise ValueError("USB descriptors exceed limit")
    units = extension_units(descriptors, interface)
    fd = os.open(device, os.O_RDONLY | os.O_CLOEXEC | os.O_NONBLOCK | os.O_NOFOLLOW)
    try:
        info = os.fstat(fd)
        expected = (Path('/sys/class/video4linux') / Path(device).name / 'dev').read_text().strip()
        if not stat.S_ISCHR(info.st_mode) or f'{os.major(info.st_rdev)}:{os.minor(info.st_rdev)}' != expected:
            raise ValueError("device does not match sysfs inventory")
        results = []
        for unit in units:
            for selector in unit['selectors']:
                entry = {"unit": unit['unit'], "guid": unit['guid'], "selector": selector}
                for request in ('length', 'info'):
                    try:
                        entry[request] = get_metadata(fd, unit['unit'], selector, request)
                    except OSError as error:
                        entry[request + '_error'] = {"errno": error.errno, "message": error.strerror}
                if 'info' in entry:
                    entry['get_supported'] = bool(entry['info'] & 1)
                    entry['set_supported'] = bool(entry['info'] & 2)
                results.append(entry)
    finally:
        os.close(fd)
    return {"version": 1, "device": device, "usb_id":
            f"{(usb / 'idVendor').read_text().strip()}:{(usb / 'idProduct').read_text().strip()}",
            "interface": interface, "units": units, "controls": results,
            "queries": ['GET_LEN', 'GET_INFO'], "control_values_read": False,
            "controls_written": False, "capture_started": False,
            "emitter_mapping_verified": False, "authentication_ready": False,
            "metadata_complete": all('length' in c and 'info' in c for c in results)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--device', required=True)
    args = parser.parse_args()
    try:
        report = inspect(args.device)
    except (OSError, ValueError) as error:
        parser.exit(1, f"UVC inspection failed: {error}\n")
    print(json.dumps(report, indent=2))
    return 0 if report['metadata_complete'] else 1


if __name__ == '__main__':
    sys.exit(main())
