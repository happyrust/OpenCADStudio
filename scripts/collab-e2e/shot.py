# -*- coding: utf-8 -*-
"""Reading the screenshots the application writes, with the standard library only.

`capture` writes 8-bit RGB or RGBA PNG files that are not interlaced; that is
all this reads. A picture is kept as one `bytes` of RGB per row, so that rows
and parts of rows can be compared and searched at the speed of `bytes`.

A rectangle is always (left, top, right, bottom) in pixels of the picture,
right and bottom excluded, and is cut to the picture before use.
"""
import struct
import zlib
from itertools import accumulate
from pathlib import Path

PNG_MAGIC = bytes([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])

_masks = {}


def _add(a, b):
    """Byte by byte, (a + b) modulo 256, for two `bytes` of one length."""
    n = len(a)
    if n not in _masks:
        low = int.from_bytes(b"\x7f" * n, "little")
        _masks[n] = (low, low ^ int.from_bytes(b"\xff" * n, "little"))
    low, high = _masks[n]
    x, y = int.from_bytes(a, "little"), int.from_bytes(b, "little")
    return (((x & low) + (y & low)) ^ ((x ^ y) & high)).to_bytes(n, "little")


def _xor(a, b):
    n = len(a)
    return (int.from_bytes(a, "little") ^ int.from_bytes(b, "little")).to_bytes(n, "little")


def _unfilter(kind, line, prior, bpp):
    """One scanline back from its PNG filter (PNG specification, section 9)."""
    if kind == 0:
        return line
    if kind == 2:
        return _add(line, prior)
    if kind == 1:
        out = bytearray(len(line))
        for c in range(bpp):
            out[c::bpp] = bytes(map((255).__and__, accumulate(line[c::bpp])))
        return bytes(out)
    if kind == 4 and not any(line):
        return prior                    # a Paeth line of zeros repeats the line above
    out = bytearray(line)
    if kind == 3:
        for i in range(len(out)):
            left = out[i - bpp] if i >= bpp else 0
            out[i] = (out[i] + ((left + prior[i]) >> 1)) & 255
        return bytes(out)
    if kind == 4:
        for i in range(len(out)):
            if i >= bpp:
                a, c = out[i - bpp], prior[i - bpp]
            else:
                a = c = 0
            b = prior[i]
            pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - c - c)
            out[i] = (out[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        return bytes(out)
    raise ValueError(f"PNG filter type {kind} does not exist")


class Picture:
    """`width`, `height`, and `rows`: one `bytes` of width * 3 (R, G, B) per row."""

    def __init__(self, width, height, rows, path=None):
        self.width, self.height, self.rows, self.path = width, height, rows, path

    def box(self, rect=None):
        """`rect` cut to the picture; the whole picture when it is None."""
        if rect is None:
            return 0, 0, self.width, self.height
        left, top, right, bottom = [int(round(v)) for v in rect]
        return max(0, left), max(0, top), min(self.width, right), min(self.height, bottom)

    def pixel(self, x, y):
        row = self.rows[y]
        return row[3 * x], row[3 * x + 1], row[3 * x + 2]

    def strip(self, y, left, right):
        return self.rows[y][3 * left:3 * right]


def signature(path):
    """The first eight bytes of the file."""
    with open(path, "rb") as f:
        return f.read(8)


def load(path):
    """The picture in a file."""
    return decode(Path(path).read_bytes(), path)


def decode(data, path="picture"):
    """The picture in the bytes of a PNG file; `path` only names it in errors."""
    if data[:8] != PNG_MAGIC:
        raise ValueError(f"{path}: not a PNG file (starts with {data[:8].hex(' ')})")
    at, header, packed = 8, None, []
    while at + 8 <= len(data):
        length, kind = struct.unpack(">I4s", data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        at += 12 + length
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            packed.append(body)
        elif kind == b"IEND":
            break
    if header is None:
        raise ValueError(f"{path}: no IHDR chunk")
    width, height, depth, colour, _compression, _filter, interlace = header
    if depth != 8 or colour not in (2, 6) or interlace:
        raise ValueError(f"{path}: only 8-bit RGB or RGBA without interlace is read here "
                         f"(bit depth {depth}, colour type {colour}, interlace {interlace})")
    bpp = 3 if colour == 2 else 4
    stride = width * bpp
    raw = zlib.decompress(b"".join(packed))
    if len(raw) != height * (stride + 1):
        raise ValueError(f"{path}: {len(raw)} bytes of pixels, {height * (stride + 1)} expected")
    rows, prior = [], bytes(stride)
    for y in range(height):
        start = y * (stride + 1)
        prior = _unfilter(raw[start], raw[start + 1:start + 1 + stride], prior, bpp)
        if bpp == 3:
            rows.append(prior)
        else:
            rgb = bytearray(width * 3)
            rgb[0::3], rgb[1::3], rgb[2::3] = prior[0::4], prior[1::4], prior[2::4]
            rows.append(bytes(rgb))
    return Picture(width, height, rows, str(path))


def far(p, q, tolerance):
    """Two colours differ by more than `tolerance` in some channel."""
    return abs(p[0] - q[0]) > tolerance or abs(p[1] - q[1]) > tolerance or abs(p[2] - q[2]) > tolerance


def commonest(picture, rect=None):
    """The colour most pixels of the rectangle have, and how many have it."""
    left, top, right, bottom = picture.box(rect)
    seen = {}
    for y in range(top, bottom):
        row = picture.strip(y, left, right)
        for i in range(0, len(row), 3):
            key = row[i:i + 3]
            seen[key] = seen.get(key, 0) + 1
    if not seen:
        return None, 0
    key = max(seen, key=seen.get)
    return tuple(key), seen[key]


def count(picture, rect, test):
    """How many pixels of the rectangle `test(red, green, blue)` accepts."""
    left, top, right, bottom = picture.box(rect)
    n = 0
    for y in range(top, bottom):
        row = picture.strip(y, left, right)
        for i in range(0, len(row), 3):
            if test(row[i], row[i + 1], row[i + 2]):
                n += 1
    return n


class Blob:
    """The pixels a search found: how many, the rectangle around them (right
    and bottom excluded) and their centre of mass."""

    def __init__(self):
        self.count, self.left, self.top, self.right, self.bottom = 0, None, None, None, None
        self._sx = self._sy = 0

    def add(self, x, y):
        self.count += 1
        self._sx += x
        self._sy += y
        if self.left is None:
            self.left, self.top, self.right, self.bottom = x, y, x + 1, y + 1
        else:
            self.left, self.right = min(self.left, x), max(self.right, x + 1)
            self.top, self.bottom = min(self.top, y), max(self.bottom, y + 1)

    @property
    def rect(self):
        return None if not self.count else (self.left, self.top, self.right, self.bottom)

    @property
    def centre(self):
        return None if not self.count else (self._sx / self.count, self._sy / self.count)

    @property
    def mid(self):
        """The middle of the rectangle around the pixels."""
        return None if not self.count else ((self.left + self.right - 1) / 2, (self.top + self.bottom - 1) / 2)

    @property
    def size(self):
        return (0, 0) if not self.count else (self.right - self.left, self.bottom - self.top)

    def __repr__(self):
        if not self.count:
            return "no pixels"
        cx, cy = self.centre
        return f"{self.count} px in {self.rect}, centre ({cx:.1f}, {cy:.1f})"


def unlike(picture, colour, rect=None, tolerance=0, skip=()):
    """The pixels of the rectangle that are not `colour` (a channel may differ
    by `tolerance`), leaving out those inside one of the `skip` rectangles."""
    left, top, right, bottom = picture.box(rect)
    plain = bytes(colour) * max(0, right - left)
    found = Blob()
    for y in range(top, bottom):
        row = picture.strip(y, left, right)
        if row == plain:
            continue
        for i in range(0, len(row), 3):
            if row[i:i + 3] != plain[:3] and far(row[i:i + 3], colour, tolerance):
                x = left + i // 3
                if not any(s[0] <= x < s[2] and s[1] <= y < s[3] for s in skip):
                    found.add(x, y)
    return found


def changed(before, after, rect=None, tolerance=0, skip=()):
    """The pixels of the rectangle that differ between two pictures of one
    size (a channel may differ by `tolerance`), leaving out the `skip` rectangles."""
    if (before.width, before.height) != (after.width, after.height):
        raise ValueError(f"the pictures differ in size: {before.width}x{before.height} and {after.width}x{after.height}")
    left, top, right, bottom = before.box(rect)
    found = Blob()
    for y in range(top, bottom):
        a, b = before.strip(y, left, right), after.strip(y, left, right)
        if a == b:
            continue
        delta = _xor(a, b)
        first = (len(delta) - len(delta.lstrip(b"\0"))) // 3
        last = (len(delta.rstrip(b"\0")) - 1) // 3
        for k in range(first, last + 1):
            i = 3 * k
            if delta[i] or delta[i + 1] or delta[i + 2]:
                if tolerance and not far(a[i:i + 3], b[i:i + 3], tolerance):
                    continue
                x = left + k
                if not any(s[0] <= x < s[2] and s[1] <= y < s[3] for s in skip):
                    found.add(x, y)
    return found


def stretches(picture, rect, colour, tolerance=0):
    """The stretches of columns of the rectangle in which some pixel is not
    `colour` (a channel may differ by `tolerance`), left to right, each as
    (first column, column after the last). For a rectangle one row high
    these are the runs of that row."""
    left, top, right, bottom = picture.box(rect)
    width = max(0, right - left)
    plain = bytes(colour) * width
    covered = bytearray(width)
    for y in range(top, bottom):
        row = picture.strip(y, left, right)
        if row == plain:
            continue
        for k in range(width):
            if not covered[k] and far(row[3 * k:3 * k + 3], colour, tolerance):
                covered[k] = 1
    out, start = [], None
    for k in range(width):
        if covered[k] and start is None:
            start = k
        elif not covered[k] and start is not None:
            out.append((left + start, left + k))
            start = None
    if start is not None:
        out.append((left + start, right))
    return out


def mask(picture, rect, colour, tolerance=0):
    """The rectangle as rows of 0 and 1: 1 where the pixel is not `colour`
    (a channel may differ by `tolerance`)."""
    left, top, right, bottom = picture.box(rect)
    rows = []
    for y in range(top, bottom):
        row = picture.strip(y, left, right)
        rows.append(bytes(1 if far(row[i:i + 3], colour, tolerance) else 0 for i in range(0, len(row), 3)))
    return rows


def apply(matrix, x, y):
    """A point through one of the six-number matrices of a `capture` answer
    (`world_to_pixel_matrix`, `pixel_to_world_matrix`): [m00, m01, m10, m11, tx, ty]."""
    m00, m01, m10, m11, tx, ty = matrix
    return m00 * x + m01 * y + tx, m10 * x + m11 * y + ty
