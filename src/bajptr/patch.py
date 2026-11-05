"""一站式流程：定位库 -> 备份 -> 解密 -> 字段级补丁 -> 重加密 -> 输出或安装。

安装模式（--install）不改动原库文件：把新库写成 <同前缀>_<新crc32>，再把
Catalog/TableCatalog.bytes 里 ExcelDB.db 条目的 size/crc 改成新值——这正是汉化补丁
在日服上用的同一套机制（已在 E:\\YostarGames\\BlueArchive_JP 实测生效）。
恢复只需把备份目录里的 TableCatalog.bytes/hash 拷回去。
"""
from __future__ import annotations

import binascii
import json
import os
import shutil
import sqlite3
import tempfile
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path

from . import config as cfg
from . import crypt, db, region
from .flatbuf import patch_blob
from .glossary import Glossary


@dataclass
class PatchStats:
    rows_seen: int = 0
    rows_changed: int = 0
    fields_changed: int = 0
    inplace: int = 0
    appended: int = 0
    bytes_appended: int = 0
    kana_skipped: int = 0
    parse_failed: int = 0
    tables: list[str] = field(default_factory=list)
    target: str = ""
    plain_bytes: int = 0
    out_path: str = ""
    crc: int = 0


def _make_transform(g: Glossary | None) -> tuple[Callable[[str], str | None], Callable[[str], str | None]]:
    def transform(text: str) -> str | None:
        return g.apply(text) if g is not None else None

    def ascii_transform(text: str) -> str | None:
        return g.apply_exact(text) if g is not None else None

    return transform, ascii_transform


def _rows(con: sqlite3.Connection, table: str) -> tuple[list[tuple[int, bytes]], str]:
    """返回 ([(id, blob)], 更新语句)；顺手兼容没有 rowid 的表。"""
    try:
        cur = con.execute(f'SELECT rowid, Bytes FROM [{table}]')
        rows = cur.fetchall()
        return [(r[0], r[1]) for r in rows], f'UPDATE [{table}] SET Bytes=? WHERE rowid=?'
    except sqlite3.Error:
        pass
    cur = con.execute(f'SELECT Key, Bytes FROM [{table}]')
    rows = cur.fetchall()
    return [(r[0], r[1]) for r in rows], f'UPDATE [{table}] SET Bytes=? WHERE Key=?'


def patch_plain(
    plain: Path,
    *,
    tables: str,
    glossary: Glossary | None,
    skip_kana: bool,
    ascii_exact: bool,
    dry_run: bool,
    log=print,
) -> PatchStats:
    """对明文库做字段级补丁；dry_run 时只统计不写。"""
    stats = PatchStats()
    transform, ascii_transform = _make_transform(glossary)
    if not ascii_exact:
        ascii_transform = None
    con = db.connect(str(plain))
    try:
        todo = db.select_tables(con, tables)
        stats.tables = todo
        log(f"tables={len(db.list_tables(con))}  text tables matched={len(todo)}")
        for table in todo:
            try:
                rows, sql = _rows(con, table)
            except sqlite3.Error as exc:
                log(f"  [{table}] skipped: {exc}")
                continue
            updates: list[tuple[bytes, int]] = []
            t_fields = t_in = t_app = t_kana = t_bad = 0
            for rid, blob in rows:
                if not isinstance(blob, (bytes, bytearray)) or len(blob) < 16:
                    continue
                try:
                    new_blob, bs = patch_blob(
                        bytes(blob), transform, skip_kana=skip_kana, ascii_transform=ascii_transform
                    )
                except ValueError:
                    stats.parse_failed += 1
                    t_bad += 1
                    continue
                stats.kana_skipped += bs.kana_skipped
                t_kana += bs.kana_skipped
                if new_blob is None:
                    continue
                updates.append((new_blob, rid))
                stats.rows_changed += 1
                stats.fields_changed += bs.inplace + bs.appended
                stats.inplace += bs.inplace
                stats.appended += bs.appended
                stats.bytes_appended += bs.bytes_appended
                t_fields += bs.inplace + bs.appended
                t_in += bs.inplace
                t_app += bs.appended
            if updates and not dry_run:
                con.executemany(sql, updates)
                con.commit()
            if updates or t_kana or t_bad:
                log(
                    f"  [{table}] rows={len(rows)} changed={len(updates)} fields={t_fields} "
                    f"(inplace={t_in} appended={t_app}) kana_skipped={t_kana}"
                    + (f" parse_failed={t_bad}" if t_bad else "")
                )
            stats.rows_seen += len(rows)
    finally:
        con.close()
    return stats


def _crc32_file(path: Path) -> int:
    crc = 0
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            crc = binascii.crc32(chunk, crc)
    return crc & 0xFFFFFFFF


def _backup(cat: Path, hash_path: Path, backup_root: Path, manifest: dict) -> Path:
    stamp = time.strftime("%Y%m%d_%H%M%S")
    dst = backup_root / stamp
    dst.mkdir(parents=True, exist_ok=True)
    shutil.copy2(cat, dst / "TableCatalog.bytes")
    if hash_path.exists():
        shutil.copy2(hash_path, dst / "TableCatalog.hash")
    (dst / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    return dst


def run(
    *,
    game_dir: str | None,
    db_path: str | None,
    key_hex: str,
    out_dir: str,
    table_pattern: str,
    glossary_path: str | None,
    skip_kana: bool,
    ascii_exact: bool,
    dry_run: bool,
    install: bool,
    verify: bool,
    backup_dir: str,
    work_dir: str | None,
    log=print,
) -> PatchStats:
    key = bytes.fromhex(key_hex.strip())
    game = region.resolve_game_dir(game_dir) if not db_path else None
    cat = (game / cfg.CATALOG_REL) if game else None
    entry = None
    if db_path:
        src = Path(db_path)
        if cat is None:
            candidate = Path(db_path).parents[2] / "Catalog" / "TableCatalog.bytes"
            cat = candidate if candidate.exists() else None
        if cat is not None and cat.exists():
            try:
                entry = region.read_catalog_entry(cat)
            except RuntimeError as exc:
                log(f"catalog: {exc}（安装模式将不可用）")
                entry = None
    else:
        assert cat is not None
        if not cat.exists():
            raise RuntimeError(f"找不到 catalog: {cat}")
        entry = region.read_catalog_entry(cat)
        src = region.find_table_bundle(game, entry)
    log(f"库文件: {src} ({os.path.getsize(src) / 1048576:.1f} MB)")
    if cat is not None:
        log(f"catalog: {cat}")
    if entry:
        log(f"catalog 条目: size={entry['size']} crc={entry['crc']}")

    glossary = Glossary.load(glossary_path) if glossary_path else None
    if glossary is not None:
        log(f"词表: {glossary_path}  {len(glossary)} 条")
    else:
        log("词表: 无（只统计，不做任何替换）")

    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    tmpdir = Path(tempfile.mkdtemp(prefix="bajptr_", dir=work_dir))
    plain = tmpdir / "plain.db"
    enc = tmpdir / "patched.db"
    try:
        log("解密中…")
        info = crypt.decrypt(src, plain, key)
        log(f"解密完成: {info['pages']} 页 -> {plain} ({os.path.getsize(plain) / 1048576:.1f} MB)")
        stats = patch_plain(
            plain,
            tables=table_pattern,
            glossary=glossary,
            skip_kana=skip_kana,
            ascii_exact=ascii_exact,
            dry_run=dry_run,
            log=log,
        )
        stats.target = str(src)
        if dry_run:
            log(
                f"[dry-run] rows={stats.rows_seen} changed={stats.rows_changed} "
                f"fields={stats.fields_changed}（未写任何游戏文件）"
            )
            return stats
        log("重加密中…")
        crypt.encrypt(plain, enc, key, info["salt"], info["ivs"])
        crc = _crc32_file(enc)
        size = os.path.getsize(enc)
        stats.crc, stats.plain_bytes = crc, size

        prefix = src.name.split("_")[0]
        out_name = f"{prefix}_{crc}"
        if install:
            if game is None:
                raise RuntimeError("--install 需要能定位游戏目录（用 --game-dir）")
            if entry is None:
                raise RuntimeError("--install 需要可解析的 TableCatalog.bytes")
            bdir = _backup(
                cat,
                game / cfg.CATALOG_HASH_REL,
                Path(backup_dir),
                {
                    "created": time.strftime("%Y-%m-%d %H:%M:%S"),
                    "game_dir": str(game),
                    "db_file": src.name,
                    "catalog_size_before": entry["size"],
                    "catalog_crc_before": entry["crc"],
                },
            )
            log(f"已备份 catalog -> {bdir}")
            target = src.with_name(out_name)
            if target != src:
                shutil.copy2(enc, target)
            else:
                tmp = target.with_name(target.name + ".new")
                shutil.copy2(enc, tmp)
                os.replace(tmp, target)
            region.write_catalog_entry(cat, entry, size, crc)
            log(f"已安装: {target}")
            log(f"catalog 已更新: size={entry['size']} -> {size}, crc={entry['crc']} -> {crc}")
            stats.out_path = str(target)
        else:
            target = out / out_name
            shutil.copy2(enc, target)
            log(f"输出: {target}（未改动游戏目录；加 --install 才会写进去）")
            stats.out_path = str(target)

        if verify and glossary is not None:
            log("回读校验中…")
            chk = tmpdir / "check.db"
            crypt.decrypt(Path(stats.out_path), chk, key)
            con = db.connect(str(chk))
            try:
                res = con.execute("PRAGMA integrity_check").fetchone()[0]
            finally:
                con.close()
            if res != "ok":
                raise RuntimeError(f"integrity_check: {res}")
            log("integrity_check = ok")
        if glossary is not None:
            log("词表命中：")
            for row in glossary.report():
                log("  " + row.replace("\t", " -> ", 1))
        log(
            f"完成: rows={stats.rows_seen} changed={stats.rows_changed} fields={stats.fields_changed} "
            f"(inplace={stats.inplace} appended={stats.appended} +{stats.bytes_appended}B) "
            f"kana_skipped={stats.kana_skipped}"
            + (f" parse_failed={stats.parse_failed}" if stats.parse_failed else "")
        )
        return stats
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)
