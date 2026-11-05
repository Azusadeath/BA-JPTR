"""明文 SQLite 库的遍历与写回（加解密由 crypt.py 负责）。"""
from __future__ import annotations

import re
import sqlite3


def connect(path: str) -> sqlite3.Connection:
    con = sqlite3.connect(path)
    con.execute("PRAGMA synchronous=OFF")
    return con


def list_tables(con: sqlite3.Connection) -> list[str]:
    cur = con.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    return [r[0] for r in cur.fetchall()]


def select_tables(con: sqlite3.Connection, pattern: str) -> list[str]:
    """按正则筛出可能含玩家文本的表。"""
    pat = re.compile(pattern, re.I)
    return [t for t in list_tables(con) if pat.search(t)]
