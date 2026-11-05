"""bajptr 命令行入口。"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
from pathlib import Path

from . import __version__, config as cfg, crypt, db, region
from . import patch as patch_mod
from .flatbuf import has_cjk, iter_string_fields


def _oneline(text: str) -> str:
    """把换行/制表符转义，保证一行一条记录（TSV/日志都不被撑破）。"""
    return text.replace("\r", "\\r").replace("\n", "\\n").replace("\t", "\\t")


def _setup_stdout() -> None:
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except Exception:
            pass


def _locate(args) -> tuple[Path | None, Path | None, dict | None, Path]:
    """解析游戏目录 / catalog / catalog 条目 / 库文件。"""
    if getattr(args, "db", None):
        src = Path(args.db)
        cat = None
        if getattr(args, "game_dir", None):
            cat = Path(args.game_dir) / cfg.CATALOG_REL
        if cat is None or not cat.exists():
            cand = src.parents[2] / "Catalog" / "TableCatalog.bytes" if len(src.parents) > 2 else None
            cat = cand if cand and cand.exists() else None
        entry = None
        if cat is not None:
            try:
                entry = region.read_catalog_entry(cat)
            except RuntimeError:
                entry = None
        return None, cat, entry, src
    game = region.resolve_game_dir(getattr(args, "game_dir", None))
    cat = game / cfg.CATALOG_REL
    if not cat.exists():
        raise RuntimeError(f"找不到 catalog: {cat}")
    entry = region.read_catalog_entry(cat)
    src = region.find_table_bundle(game, entry)
    return game, cat, entry, src


def _decrypt(args) -> tuple[Path, bytes, str]:
    """解密到临时目录，返回 (明文库路径, key, 临时目录)；调用方负责清理。"""
    import tempfile

    key = bytes.fromhex(args.key.strip())
    _game, _cat, _entry, src = _locate(args)
    tmpdir = tempfile.mkdtemp(prefix="bajptr_")
    plain = Path(tmpdir) / "plain.db"
    info = crypt.decrypt(src, plain, key)
    print(f"库文件: {src} ({os.path.getsize(src) / 1048576:.1f} MB) / {info['pages']} 页")
    return plain, key, tmpdir


def cmd_paths(args) -> int:
    game = region.resolve_game_dir(args.game_dir) if args.game_dir else region.game_dir_from_registry()
    if not game:
        print("未找到日服安装目录（注册表无记录），请用 --game-dir 指定")
        return 1
    game = Path(game)
    print(f"游戏目录: {game}")
    cat = game / cfg.CATALOG_REL
    print(f"catalog : {cat}  exists={cat.exists()}")
    if not cat.exists():
        return 1
    entry = region.read_catalog_entry(cat)
    print(f"条目    : size={entry['size']} (0x{entry['size']:X})  crc={entry['crc']} (0x{entry['crc']:X})")
    try:
        src = region.find_table_bundle(game, entry)
        print(f"库文件  : {src}")
        print(f"页数    : {crypt.page_count(src)}")
    except RuntimeError as exc:
        print(f"库文件  : {exc}")
    return 0


def cmd_backup(args) -> int:
    game = region.resolve_game_dir(args.game_dir)
    cat = game / cfg.CATALOG_REL
    entry = region.read_catalog_entry(cat)
    src = region.find_table_bundle(game, entry)
    stamp_root = Path(args.out)
    stamp_root.mkdir(parents=True, exist_ok=True)
    dst = stamp_root / "catalog"
    dst.mkdir(parents=True, exist_ok=True)
    shutil.copy2(cat, dst / "TableCatalog.bytes")
    hp = game / cfg.CATALOG_HASH_REL
    if hp.exists():
        shutil.copy2(hp, dst / "TableCatalog.hash")
    (dst / "manifest.json").write_text(
        json.dumps(
            {
                "game_dir": str(game),
                "db_file": src.name,
                "size": entry["size"],
                "crc": entry["crc"],
                "note": "恢复时把这两个文件拷回 Catalog/ 即可",
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )
    print(f"已备份 catalog -> {dst}")
    if args.with_db:
        target = stamp_root / src.name
        print(f"复制库文件（{os.path.getsize(src) / 1048576:.1f} MB）…")
        shutil.copy2(src, target)
        print(f"已备份库 -> {target}")
    return 0


def cmd_restore(args) -> int:
    game = region.resolve_game_dir(args.game_dir)
    src_dir = Path(args.backup)
    cat = src_dir / "TableCatalog.bytes"
    if not cat.exists():
        raise FileNotFoundError(f"备份目录里没有 TableCatalog.bytes: {src_dir}")
    dst_dir = game / "BlueArchive_Data" / "StreamingAssets" / "TableBundles" / "Catalog"
    shutil.copy2(cat, dst_dir / "TableCatalog.bytes")
    hp = src_dir / "TableCatalog.hash"
    if hp.exists():
        shutil.copy2(hp, dst_dir / "TableCatalog.hash")
    print(f"已恢复 catalog: {dst_dir}")

    entry = region.read_catalog_entry(dst_dir / "TableCatalog.bytes")
    bundle_dir = dst_dir.parent
    prefix = ""
    mpath = src_dir / "manifest.json"
    if mpath.exists():
        prefix = json.loads(mpath.read_text(encoding="utf-8")).get("db_file", "").rsplit("_", 1)[0]
    stale = []
    if prefix:
        for p in bundle_dir.iterdir():
            if not p.is_file():
                continue
            stem, _, crc_part = p.name.rpartition("_")
            if stem == prefix and crc_part.isdigit() and int(crc_part) != entry["crc"]:
                stale.append(p)
    if stale:
        if args.prune:
            for p in stale:
                size_mb = p.stat().st_size / 1048576
                p.unlink()
                print(f"已删除无引用的库文件: {p.name}（{size_mb:.1f} MB）")
        else:
            print("以下库文件已不再被 catalog 引用（可加 --prune 删除）:")
            for p in stale:
                print(f"  {p}  {p.stat().st_size / 1048576:.1f} MB")
    return 0


def cmd_scan(args) -> int:
    plain, _key, tmpdir = _decrypt(args)
    try:
        con = db.connect(str(plain))
        todo = db.select_tables(con, args.tables)
        needle = args.keyword
        hits = 0
        for table in todo:
            try:
                cur = con.execute(f'SELECT rowid, Bytes FROM [{table}]')
            except Exception:
                continue
            for rid, blob in cur:
                if not isinstance(blob, (bytes, bytearray)):
                    continue
                try:
                    fields = list(iter_string_fields(bytes(blob)))
                except ValueError:
                    continue
                for slot, _fpos, _spos, text in fields:
                    if needle in text:
                        hits += 1
                        if hits <= args.limit:
                            print(f"{table}\t{rid}\tfield{slot}\t{_oneline(text)[:120]}")
        print(f"命中 {hits} 处（显示前 {min(hits, args.limit)} 条）")
        return 0
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


def cmd_export(args) -> int:
    plain, _key, tmpdir = _decrypt(args)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    try:
        con = db.connect(str(plain))
        todo = db.select_tables(con, args.tables)
        total = 0
        for table in todo:
            try:
                cur = con.execute(f'SELECT rowid, Bytes FROM [{table}]')
            except Exception:
                continue
            rows: list[str] = []
            for rid, blob in cur:
                if not isinstance(blob, (bytes, bytearray)):
                    continue
                try:
                    fields = list(iter_string_fields(bytes(blob)))
                except ValueError:
                    continue
                for slot, _fpos, _spos, text in fields:
                    if has_cjk(text):
                        rows.append(f"{rid}\t{slot}\t{_oneline(text)}")
            if rows:
                p = out / f"{table}.tsv"
                p.write_text("rowid\tfield\ttext\n" + "\n".join(rows), encoding="utf-8")
                total += len(rows)
                print(f"  [{table}] {len(rows)} 条 -> {p.name}")
        print(f"共导出 {total} 条文本 -> {out}")
        return 0
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


def cmd_patch(args) -> int:
    stats = patch_mod.run(
        game_dir=args.game_dir,
        db_path=args.db,
        key_hex=args.key,
        out_dir=args.out,
        table_pattern=args.tables,
        glossary_path=args.glossary,
        skip_kana=not args.keep_kana,
        ascii_exact=not args.no_ascii_exact,
        dry_run=args.dry_run,
        install=args.install,
        verify=args.verify,
        backup_dir=args.backup_dir,
        work_dir=args.work_dir,
    )
    return 0 if stats else 1


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="bajptr",
        description=f"bajptr {__version__} — 蔚蓝档案日服（JP）译名替换 / 文本补丁工具",
    )
    p.add_argument("--version", action="version", version=f"bajptr {__version__}")
    sub = p.add_subparsers(dest="command", required=True)

    def common(sp: argparse.ArgumentParser, db_arg: bool = True) -> None:
        sp.add_argument("--game-dir", default=None, help="游戏安装目录（默认读注册表）")
        if db_arg:
            sp.add_argument("--db", default=None, help="直接指定库文件（跳过目录探测）")
        sp.add_argument("--key", default=cfg.DEFAULT_KEY, help="SQLCipher 密钥（hex），默认日服公开密钥")

    sp = sub.add_parser("paths", help="显示探测到的游戏目录 / catalog / 库文件")
    common(sp, db_arg=False)
    sp.set_defaults(func=cmd_paths)

    sp = sub.add_parser("backup", help="备份 catalog（安装前的安全网）")
    common(sp, db_arg=False)
    sp.add_argument("--out", default=cfg.DEFAULT_BACKUP_DIR, help="备份根目录")
    sp.add_argument("--with-db", action="store_true", help="连 217MB 的库文件一起备份")
    sp.set_defaults(func=cmd_backup)

    sp = sub.add_parser("restore", help="从备份目录恢复 catalog")
    common(sp, db_arg=False)
    sp.add_argument("--backup", required=True, help="备份目录（含 TableCatalog.bytes）")
    sp.add_argument("--prune", action="store_true", help="同时删除已不被 catalog 引用的旧库文件")
    sp.set_defaults(func=cmd_restore)

    sp = sub.add_parser("scan", help="在明文库里按关键词定位文本")
    common(sp)
    sp.add_argument("--keyword", required=True)
    sp.add_argument("--tables", default=cfg.TABLE_PATTERN)
    sp.add_argument("--limit", type=int, default=20)
    sp.set_defaults(func=cmd_scan)

    sp = sub.add_parser("export", help="把含中文的文本字段导出为每表一个 TSV")
    common(sp)
    sp.add_argument("--tables", default=cfg.TABLE_PATTERN)
    sp.add_argument("--out", default=cfg.DEFAULT_OUT_DIR + "/export")
    sp.set_defaults(func=cmd_export)

    sp = sub.add_parser("patch", help="字段级文本补丁（默认只输出到 output/，--install 才写游戏目录）")
    common(sp)
    sp.add_argument("--tables", default=cfg.TABLE_PATTERN)
    sp.add_argument("--glossary", default=None, help="词表 TSV（旧词<TAB>新词[<TAB>mode][<TAB>来源]）")
    sp.add_argument("--out", default=cfg.DEFAULT_OUT_DIR, help="输出目录")
    sp.add_argument("--install", action="store_true", help="写回游戏目录：新库文件 + 同步 catalog（自动备份 catalog）")
    sp.add_argument("--backup-dir", default=cfg.DEFAULT_BACKUP_DIR)
    sp.add_argument("--dry-run", action="store_true", help="只统计，不写任何文件")
    sp.add_argument("--verify", action="store_true", help="回读输出库做解密 + integrity_check")
    sp.add_argument("--keep-kana", action="store_true", help="不跳过含假名的字段（默认跳过残留日文）")
    sp.add_argument("--no-ascii-exact", action="store_true", help="纯 ASCII 字段不参与整字段匹配")
    sp.add_argument("--work-dir", default=None, help="临时明文库所在目录（默认系统临时目录）")
    sp.set_defaults(func=cmd_patch)
    return p


def main(argv: list[str] | None = None) -> int:
    _setup_stdout()
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except KeyboardInterrupt:
        print("\n已中止")
        return 130
    except Exception as exc:
        print(f"错误: {exc}")
        return 1
