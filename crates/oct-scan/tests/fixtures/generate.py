#!/usr/bin/env python3
"""Regenerates the PDF fixtures for the scanner tests.

Run from this directory:  python3 generate.py

Only the standard library is used, deliberately: the fixtures must be reproducible on any machine
without `qpdf`, `mutool` or a PDF library. They are committed because the tests must not depend on
this script being run, and this script is committed because a fixture nobody can regenerate is a
fixture nobody can trust.

Why the fixtures are shaped this way:

- `pages-in-object-stream.pdf` exists to fail a naive page count. Its three page objects live inside
  a FlateDecode-compressed object stream, so the bytes `/Type /Page` do not appear in the file at all.
  Anything that counts pages by scanning bytes returns 0, not 3 — which is exactly risk R2, and the
  test asserts that the naive method is wrong on this file so the fixture cannot become vacuous.
  Note that `ghostscript -sDEVICE=pdfwrite` cannot produce this: its output has no object streams.
- `with-info-metadata.pdf` carries a classic cross-reference table and an Info dictionary with a
  title and an author, so metadata extraction is tested against something real rather than a stub.
  Its page count (4) deliberately differs from the other fixture's (3), so a test that reads the
  wrong file cannot pass by accident.
"""

from __future__ import annotations

import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent


def pdf_string(value: str) -> bytes:
    """Encodes a PDF literal string, escaping the characters that would end it."""
    escaped = value.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")
    return f"({escaped})".encode("latin-1")


def classic_xref_pdf(objects: dict[int, bytes], root: int, info: int | None) -> bytes:
    """Builds a PDF with a classic cross-reference table and uncompressed objects."""
    out = bytearray(b"%PDF-1.4\n")
    offsets: dict[int, int] = {}
    for number in sorted(objects):
        offsets[number] = len(out)
        out += f"{number} 0 obj\n".encode("ascii") + objects[number] + b"\nendobj\n"

    highest = max(objects)
    xref_at = len(out)
    out += f"xref\n0 {highest + 1}\n".encode("ascii")
    out += b"0000000000 65535 f \n"
    for number in range(1, highest + 1):
        if number in offsets:
            out += f"{offsets[number]:010d} 00000 n \n".encode("ascii")
        else:
            # Object numbers need not be contiguous; a missing one is a free entry, not a hole in
            # the table. Emitting the entry keeps the table well formed.
            out += b"0000000000 65535 f \n"

    trailer = f"<< /Size {highest + 1} /Root {root} 0 R"
    if info is not None:
        trailer += f" /Info {info} 0 R"
    trailer += " >>"
    out += f"trailer\n{trailer}\nstartxref\n{xref_at}\n%%EOF\n".encode("ascii")
    return bytes(out)


def object_stream_pdf(page_objects: dict[int, bytes], page_numbers: list[int]) -> bytes:
    """Builds a PDF whose page objects live inside one compressed object stream.

    Structure: 1 = catalog, 2 = pages, 3..n = page objects (inside the object stream), then the
    object stream itself, then the cross-reference stream that indexes everything.
    """
    pages_number = 2
    objstm_number = max(page_numbers) + 1
    xref_number = objstm_number + 1

    # The object stream's decoded payload: a header of (object number, offset) pairs relative to
    # /First, followed by the concatenated object bodies.
    bodies = [page_objects[number] for number in page_numbers]
    offsets = []
    cursor = 0
    for body in bodies:
        offsets.append(cursor)
        cursor += len(body)

    header = b"".join(
        f"{number} {offset} ".encode("ascii")
        for number, offset in zip(page_numbers, offsets)
    )
    decoded = header + b"".join(bodies)

    out = bytearray(b"%PDF-1.5\n")
    obj_offsets: dict[int, int] = {}

    obj_offsets[1] = len(out)
    out += b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n"

    kids = " ".join(f"{number} 0 R" for number in page_numbers)
    obj_offsets[pages_number] = len(out)
    out += (
        f"{pages_number} 0 obj\n<< /Type /Pages /Kids [{kids}] /Count {len(page_numbers)} >>\nendobj\n"
    ).encode("ascii")

    obj_offsets[objstm_number] = len(out)
    compressed = zlib.compress(decoded, 9)
    out += (
        f"{objstm_number} 0 obj\n<< /Type /ObjStm /N {len(page_numbers)} /First {len(header)} "
        f"/Filter /FlateDecode /Length {len(compressed)} >>\nstream\n"
    ).encode("ascii")
    out += compressed + b"\nendstream\nendobj\n"

    # The cross-reference stream indexes objects 0..xref_number. Page objects live inside the object
    # stream, so their entries are type 2 (compressed): the object stream's number and the object's
    # zero-based position inside it. The stream's own entry is type 1 at its own offset, which is
    # known before the entries are built because the stream is written last.
    obj_offsets[xref_number] = len(out)

    def type1(offset: int) -> bytes:
        return (1).to_bytes(1, "big") + offset.to_bytes(2, "big") + (0).to_bytes(1, "big")

    def type2(container: int, index: int) -> bytes:
        return (2).to_bytes(1, "big") + container.to_bytes(2, "big") + index.to_bytes(1, "big")

    entries = bytearray()
    entries += (0).to_bytes(1, "big") + (0).to_bytes(2, "big") + (255).to_bytes(1, "big")
    entries += type1(obj_offsets[1])
    entries += type1(obj_offsets[pages_number])
    for index, number in enumerate(page_numbers):
        entries += type2(objstm_number, index)
    entries += type1(obj_offsets[objstm_number])
    entries += type1(obj_offsets[xref_number])
    assert len(entries) == (xref_number + 1) * 4, (len(entries), xref_number)
    compressed_entries = zlib.compress(bytes(entries), 9)
    out += (
        f"{xref_number} 0 obj\n<< /Type /XRef /Size {xref_number + 1} /Root 1 0 R "
        f"/W [1 2 1] /Index [0 {xref_number + 1}] /Filter /FlateDecode "
        f"/Length {len(compressed_entries)} >>\nstream\n"
    ).encode("ascii")
    out += compressed_entries + b"\nendstream\nendobj\n"
    out += f"startxref\n{obj_offsets[xref_number]}\n%%EOF\n".encode("ascii")
    return bytes(out)


def build_object_stream_fixture(page_count: int = 3) -> bytes:
    page_numbers = list(range(3, 3 + page_count))
    page_objects = {
        number: f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] >>".encode("ascii")
        for number in page_numbers
    }
    return object_stream_pdf(page_objects, page_numbers)


def build_info_fixture(page_count: int = 4) -> bytes:
    content = b"BT /F1 12 Tf 20 100 Td (A page) Tj ET"
    page_numbers = list(range(3, 3 + page_count))
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [{' '.join(f'{n} 0 R' for n in page_numbers)}] "
            f"/Count {page_count} >>"
        ).encode("ascii"),
    }
    font_number = 3 + page_count * 2
    for index, number in enumerate(page_numbers):
        objects[number] = (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Contents {number + 1} 0 R "
            f"/Resources << /Font << /F1 {font_number} 0 R >> >> >>"
        ).encode("ascii")
        objects[number + 1] = (
            f"<< /Length {len(content)} >>\nstream\n".encode("ascii") + content + b"\nendstream"
        )
    objects[font_number] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"
    info_number = font_number + 1
    objects[info_number] = (
        b"<< /Title " + pdf_string("A Known Title") + b" /Author " + pdf_string("A Known Author") + b" >>"
    )
    return classic_xref_pdf(objects, root=1, info=info_number)


def main() -> None:
    object_stream = HERE / "pages-in-object-stream.pdf"
    object_stream.write_bytes(build_object_stream_fixture())
    print(f"wrote {object_stream.name} ({object_stream.stat().st_size} bytes)")

    info = HERE / "with-info-metadata.pdf"
    info.write_bytes(build_info_fixture())
    print(f"wrote {info.name} ({info.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
