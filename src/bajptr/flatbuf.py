"""FlatBuffer 字符串字段的解析与写回。

移植自 BATR（github.com/Agent-0808/BA-Term-Replacer，GPL-3.0）的 src/batr/flatbuf.py，
算法与其一致：每行 Bytes 是一个完整 FlatBuffer，根是一张表；字符串字段是一个 uoffset32
（相对字段位置），指向 [u32 长度][UTF-8][0x00]。

只要不移动任何已有字节，所有旧偏移仍然有效，因此变长替换的做法是：
等长 -> 原地写字节；变长 -> 把新字符串追加到 blob 末尾（4 字节对齐），再回填该字段的 uoffset32。
日服实测：LocalizeDBSchema 的 blob 是根表 + 若干字符串字段，同一行可含多段语言文本。
"""
from __future__ import annotations

import struct
from collections.abc import Callable, Iterator
from dataclasses import dataclass

MAX_STRING_LEN = 1 << 20


def _is_cjk(ch: str) -> bool:
    cp = ord(ch)
    return 0x3400 <= cp <= 0x9FFF or 0xF900 <= cp <= 0xFAFF or 0x20000 <= cp <= 0x2FA1F


def has_cjk(text: str) -> bool:
    return any(_is_cjk(ch) for ch in text)


def has_kana(text: str) -> bool:
    """含平/片假名 -> 视为残留日文字段（默认跳过）。"""
    return any(0x3040 <= ord(ch) <= 0x30FF for ch in text)


def read_string(b: bytes | bytearray, spos: int, *, require_cjk: bool = True) -> str | None:
    """按六条硬校验读取 spos 处的字符串；任一条不满足返回 None（防误把标量当字符串）。"""
    n = len(b)
    if not (4 <= spos <= n - 5):
        return None
    ln = int.from_bytes(b[spos:spos + 4], "little")
    if ln > MAX_STRING_LEN:
        return None
    end = spos + 4 + ln
    if end >= n or b[end] != 0x00:
        return None
    try:
        text = bytes(b[spos + 4:end]).decode("utf-8")
    except UnicodeDecodeError:
        return None
    if require_cjk and not has_cjk(text):
        return None
    if any(ord(ch) < 32 and ch != "\n" for ch in text):
        return None
    return text


def iter_string_fields(
    blob: bytes, *, allow_ascii: bool = False
) -> Iterator[tuple[int, int, int, str]]:
    """产出 (slot, fpos, spos, text)；结构不合法抛 ValueError。"""
    n = len(blob)
    if n < 16:
        raise ValueError("blob too small")
    root = int.from_bytes(blob[0:4], "little")
    if not (4 <= root <= n - 4):
        raise ValueError("root out of range")
    vt = root - int.from_bytes(blob[root:root + 4], "little", signed=True)
    if not (0 <= vt <= n - 4):
        raise ValueError("vtable out of range")
    vt_len = int.from_bytes(blob[vt:vt + 2], "little")
    if vt_len < 4 or vt_len % 2 != 0 or vt + vt_len > n:
        raise ValueError("bad vtable length")
    for k in range(2, vt_len // 2):
        off = int.from_bytes(blob[vt + 2 * k:vt + 2 * k + 2], "little")
        if off == 0:
            continue
        fpos = root + off
        if fpos + 4 > n:
            continue
        spos = fpos + int.from_bytes(blob[fpos:fpos + 4], "little")
        text = read_string(blob, spos, require_cjk=not allow_ascii)
        if text is not None:
            yield k - 2, fpos, spos, text


@dataclass
class BlobStats:
    inplace: int = 0
    appended: int = 0
    bytes_appended: int = 0
    kana_skipped: int = 0


def patch_blob(
    blob: bytes,
    transform: Callable[[str], str | None],
    *,
    skip_kana: bool = True,
    ascii_transform: Callable[[str], str | None] | None = None,
) -> tuple[bytes | None, BlobStats]:
    """对单个 FlatBuffer blob 做字段级文本补丁；结构不合法抛 ValueError。"""
    b = bytearray(blob)
    stats = BlobStats()
    appended: dict[int, int] = {}
    changed = False
    for _slot, fpos, spos, text in iter_string_fields(b, allow_ascii=ascii_transform is not None):
        if skip_kana and has_kana(text):
            stats.kana_skipped += 1
            continue
        fn = transform if has_cjk(text) else ascii_transform
        if fn is None:
            continue
        new_text = fn(text)
        if new_text is None or new_text == text:
            continue
        new_bytes = new_text.encode("utf-8")
        if len(new_bytes) == len(text.encode("utf-8")):
            b[spos + 4:spos + 4 + len(new_bytes)] = new_bytes
            stats.inplace += 1
        else:
            if spos in appended:
                struct.pack_into("<I", b, fpos, appended[spos] - fpos)
                continue
            pad = (-len(b)) % 4
            b += b"\x00" * pad
            spos_new = len(b)
            b += struct.pack("<I", len(new_bytes)) + new_bytes + b"\x00"
            struct.pack_into("<I", b, fpos, spos_new - fpos)
            appended[spos] = spos_new
            stats.appended += 1
            stats.bytes_appended += pad + 4 + len(new_bytes) + 1
        changed = True
    return (bytes(b) if changed else None, stats)
