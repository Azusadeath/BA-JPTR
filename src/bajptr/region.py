"""日服安装目录、库文件与 TableCatalog 的定位。

日服的库文件名是 <表哈希>_<crc32>，随每次更新变化，所以必须从
Catalog/TableCatalog.bytes 里读 ExcelDB.db 条目（size/crc），再到 TableBundles 里找
名字以 _<crc> 结尾、大小相等的文件。
"""
from __future__ import annotations

import os
import struct
from pathlib import Path

from . import config as cfg


def game_dir_from_registry() -> str | None:
    try:
        import winreg

        key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, cfg.JP_REGISTRY_KEY)
        try:
            path, _ = winreg.QueryValueEx(key, "InstallLocation")
        finally:
            winreg.CloseKey(key)
        return path or None
    except OSError:
        return None


def resolve_game_dir(explicit: str | None) -> Path:
    if explicit:
        p = Path(explicit)
        if not p.exists():
            raise FileNotFoundError(f"游戏目录不存在: {p}")
        return p
    found = game_dir_from_registry()
    if not found:
        raise RuntimeError("未从注册表找到日服安装路径，请用 --game-dir 指定（例如 E:\\YostarGames\\BlueArchive_JP）")
    return Path(found)


def read_catalog_entry(cat_path: Path, name: str = cfg.EXCELDB_ENTRY) -> dict:
    """在 catalog 里定位表名，返回 size/crc 字段偏移与当前值。"""
    data = cat_path.read_bytes()
    idx = data.find(name.encode())
    if idx < 0:
        raise RuntimeError(f"catalog 里没有 {name} 条目: {cat_path}")
    size_off = idx + 29
    crc_off = idx + 37
    if size_off + 8 > len(data) or crc_off + 8 > len(data):
        raise RuntimeError("catalog 条目越界，布局与预期不符")
    size = struct.unpack_from("<q", data, size_off)[0]
    crc = struct.unpack_from("<q", data, crc_off)[0]
    if size <= 0 or crc <= 0:
        raise RuntimeError(f"catalog 条目值异常: size={size} crc={crc}")
    return {"size_off": size_off, "crc_off": crc_off, "size": size, "crc": crc}


def write_catalog_entry(cat_path: Path, entry: dict, size: int, crc: int) -> None:
    data = bytearray(cat_path.read_bytes())
    struct.pack_into("<q", data, entry["size_off"], size)
    struct.pack_into("<q", data, entry["crc_off"], crc)
    tmp = cat_path.with_name(cat_path.name + ".tmp")
    tmp.write_bytes(bytes(data))
    os.replace(tmp, cat_path)


def find_table_bundle(game_dir: Path, catalog: dict) -> Path:
    """按 catalog 记录的 crc/size 找到磁盘上的库文件。"""
    tb = game_dir / cfg.TABLEBUNDLES_REL
    if not tb.is_dir():
        raise RuntimeError(f"找不到 TableBundles 目录: {tb}")
    suffix = f"_{catalog['crc']}"
    cands = [p for p in tb.iterdir() if p.is_file() and p.name.endswith(suffix)]
    for p in cands:
        if os.path.getsize(p) == catalog["size"]:
            return p
    if cands:
        return cands[0]
    # 兜底：catalog 与磁盘不同步时，取目录里最大的非 zip 文件（库是唯一的大二进制）
    best = None
    for p in tb.iterdir():
        if not p.is_file():
            continue
        with open(p, "rb") as f:
            if f.read(2) == b"PK":
                continue
        size = os.path.getsize(p)
        if size > 1 << 20 and (best is None or size > best[0]):
            best = (size, p)
    if best is None:
        raise RuntimeError(f"TableBundles 里找不到 ExcelDB 库: {tb}")
    return best[1]
