#!/usr/bin/env python3
"""Inspect pinned ONNX headers without importing a runtime or executing a graph."""
import argparse
from collections import Counter
import hashlib
import importlib.util
import json
from pathlib import Path

_spec = importlib.util.spec_from_file_location("fetch_face_models", Path(__file__).with_name("fetch-face-models.py"))
fetch = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(fetch)


def varint(data, offset):
    value = 0
    for shift in range(0, 70, 7):
        if offset >= len(data):
            raise ValueError("truncated protobuf varint")
        byte = data[offset]
        offset += 1
        if shift == 63 and byte > 1:
            raise ValueError("protobuf varint exceeds 64 bits")
        value |= (byte & 127) << shift
        if byte < 128:
            return value, offset
    raise ValueError("invalid protobuf varint")


def fields(data):
    data = memoryview(data)
    offset = 0
    count = 0
    while offset < len(data):
        count += 1
        if count > 100000:
            raise ValueError("protobuf field count exceeds limit")
        key, offset = varint(data, offset)
        number, wire = key >> 3, key & 7
        if number == 0:
            raise ValueError("invalid protobuf field number")
        if wire == 0:
            value, offset = varint(data, offset)
        elif wire in (1, 2, 5):
            if wire == 2:
                size, offset = varint(data, offset)
            else:
                size = 8 if wire == 1 else 4
            if size > len(data) - offset:
                raise ValueError("protobuf field exceeds remaining bytes")
            value = data[offset:offset + size]
            offset += size
        else:
            raise ValueError("unsupported protobuf wire type")
        yield number, wire, value


def messages(data, number):
    return [value for field, wire, value in fields(data) if field == number and wire == 2]


def scalar(data, number, default=0):
    return next((value for field, wire, value in fields(data) if field == number and wire == 0), default)


def string(data, number, default=""):
    values = messages(data, number)
    if not values:
        return default
    if len(values[0]) > 4096:
        raise ValueError("ONNX metadata string exceeds limit")
    return bytes(values[0]).decode("utf-8")


def value_info(data):
    # ONNX ValueInfoProto.type -> TypeProto.tensor_type -> Tensor.shape -> dim.
    types = messages(data, 2)
    tensors = messages(types[0], 1) if types else []
    shapes = messages(tensors[0], 2) if tensors else []
    dims = []
    if shapes:
        for dim in messages(shapes[0], 1):
            dims.append(string(dim, 2) or scalar(dim, 1, None))
    return {"name": string(data, 1), "element_type": scalar(tensors[0], 1) if tensors else None, "shape": dims}


def inspect(data):
    graphs = messages(data, 7)
    if len(graphs) != 1:
        raise ValueError("expected one ONNX graph")
    graph = graphs[0]
    initializers = {string(value, 8) for value in messages(graph, 5)}
    inputs = [value_info(value) for value in messages(graph, 11)]
    operators = Counter((string(node, 7), string(node, 4)) for node in messages(graph, 1))
    return {"ir_version": scalar(data, 1), "producer": string(data, 2),
            "producer_version": string(data, 3),
            "opsets": [{"domain": string(opset, 1), "version": scalar(opset, 2)} for opset in messages(data, 8)],
            "inputs": [value for value in inputs if value["name"] not in initializers],
            "initializer_count": len(initializers),
            "initializer_backed_inputs": sum(value["name"] in initializers for value in inputs),
            "outputs": [value_info(value) for value in messages(graph, 12)],
            "node_count": sum(operators.values()),
            "operators": [{"domain": domain, "type": kind, "count": count}
                          for (domain, kind), count in sorted(operators.items())],
            "custom_operator_domains": sorted({domain for domain, _ in operators if domain not in ("", "ai.onnx")}),
            "scope": "header inventory only; not a full ONNX validator, license decision or accuracy evaluation"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--models", required=True, type=Path)
    args = parser.parse_args()
    report = []
    for _, name, size, digest in fetch.ARTIFACTS:
        with (args.models / name).open("rb") as source:
            data = source.read(size + 1)
        if len(data) != size or hashlib.sha256(data).hexdigest() != digest:
            raise ValueError(f"model pin mismatch: {name}")
        report.append({"file": name, "sha256": digest, **inspect(data)})
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
