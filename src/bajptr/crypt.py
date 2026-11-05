"""SQLCipher 页级加解密（纯 Python，AES-256-CBC + HMAC-SHA512）。

日服库实测参数：页 4096、保留 80 字节、IV 16、HMAC 64、PBKDF2-SHA512 2 轮、salt 掩码 0x3A。
页 1 布局：salt(16) + 密文(4000) + IV(16) + HMAC(64)；其余页：密文(4016) + IV(16) + HMAC(64)。
明文页 1 必须以 b"SQLite format 3\\x00" 开头，尾部 80 字节补 0，否则 SQLite 报 malformed。
HMAC 的输入是 密文 + IV + 页号（小端 4 字节）；页 1 的密文不含 salt。
"""
from __future__ import annotations

import hashlib
import hmac
import os
import struct
from pathlib import Path

from Crypto.Cipher import AES

from . import config as cfg

_PLAIN_HEADER = b"SQLite format 3\x00"


def _hkey(key: bytes, salt: bytes) -> bytes:
    return hashlib.pbkdf2_hmac(
        "sha512", key, bytes(b ^ cfg.KDF_SALT_MASK for b in salt), cfg.KDF_ITER, 32
    )


def _mac(hkey: bytes, ciphertext: bytes, iv: bytes, pgno: int) -> bytes:
    return hmac.new(hkey, ciphertext + iv + struct.pack("<I", pgno), hashlib.sha512).digest()[
        : cfg.HMAC_SIZE
    ]


def page_count(path: str | Path) -> int:
    """整页数量（尾部不足一页的字节，例如 CRC 修正字节，忽略）。"""
    return os.path.getsize(path) // cfg.PAGE_SIZE


def verify(path: str | Path, key: bytes) -> tuple[int, int]:
    """逐页校验 HMAC，返回 (通过页数, 失败页数)。"""
    ok = bad = 0
    hkey = None
    with open(path, "rb") as f:
        for pgno in range(1, page_count(path) + 1):
            page = f.read(cfg.PAGE_SIZE)
            if pgno == 1:
                hkey = _hkey(key, page[:16])
                coff, clen = 16, cfg.USABLE - 16
            else:
                coff, clen = 0, cfg.USABLE
            iv = page[cfg.USABLE:cfg.USABLE + cfg.IV_SIZE]
            got = page[cfg.USABLE + cfg.IV_SIZE:cfg.USABLE + cfg.IV_SIZE + cfg.HMAC_SIZE]
            if _mac(hkey, page[coff:coff + clen], iv, pgno) == got:
                ok += 1
            else:
                bad += 1
    return ok, bad


def decrypt(path: str | Path, out_path: str | Path, key: bytes) -> dict:
    """解密为明文 SQLite 库；返回 {"pages", "salt", "ivs"}（salt/ivs 供重加密复用）。"""
    pad = b"\x00" * cfg.RESERVE
    salt = b""
    ivs: list[bytes] = []
    hkey = None
    pages = page_count(path)
    with open(path, "rb") as fi, open(out_path, "wb") as fo:
        for pgno in range(1, pages + 1):
            page = fi.read(cfg.PAGE_SIZE)
            if pgno == 1:
                salt = page[:16]
                hkey = _hkey(key, salt)
                coff, clen = 16, cfg.USABLE - 16
            else:
                coff, clen = 0, cfg.USABLE
            iv = page[cfg.USABLE:cfg.USABLE + cfg.IV_SIZE]
            ivs.append(iv)
            plain = AES.new(key, AES.MODE_CBC, iv).decrypt(page[coff:coff + clen])
            if pgno == 1:
                fo.write(_PLAIN_HEADER)
            fo.write(plain + pad)
        os.fsync(fo.fileno())
    return {"pages": pages, "salt": salt, "ivs": ivs}


def encrypt(
    plain_path: str | Path,
    out_path: str | Path,
    key: bytes,
    salt: bytes,
    ivs: list[bytes] | None = None,
) -> dict:
    """把明文库重新加密写出；返回 {"pages", "salt"}。

    salt 复用原库（客户端只从页 1 读 salt 派生密钥，换新 salt 也行，但复用最保险）；
    每页 IV 优先复用原库同一页的 IV，页数变多时对新页取随机 IV。
    """
    hkey = _hkey(key, salt)
    ivs = ivs or []
    n = os.path.getsize(plain_path) // cfg.PAGE_SIZE
    with open(plain_path, "rb") as fi, open(out_path, "wb") as fo:
        for pgno in range(1, n + 1):
            page = fi.read(cfg.PAGE_SIZE)
            iv = ivs[pgno - 1] if pgno - 1 < len(ivs) else os.urandom(cfg.IV_SIZE)
            if pgno == 1:
                body = page[16:cfg.USABLE]
                ct = AES.new(key, AES.MODE_CBC, iv).encrypt(body)
                fo.write(salt + ct + iv + _mac(hkey, ct, iv, pgno))
            else:
                body = page[:cfg.USABLE]
                ct = AES.new(key, AES.MODE_CBC, iv).encrypt(body)
                fo.write(ct + iv + _mac(hkey, ct, iv, pgno))
        os.fsync(fo.fileno())
    return {"pages": n, "salt": salt}
