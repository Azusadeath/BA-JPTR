"""译名对照表：TSV 词表 + 命中统计。

文件格式（UTF-8，制表符分隔，空行与 # 注释行忽略）：
    旧词 <TAB> 新词 [<TAB> mode] [<TAB> 来源]
mode: exact（整字段相等，默认）| sub（子串，最长优先）| re（正则，替换为模板）

匹配顺序：exact -> sub（按旧词长度降序）-> re（按文件顺序）。每条规则独立计数，
零命中是最重要的排查线索（拼错/过期/词条已被清掉）。
"""
from __future__ import annotations

import re
from pathlib import Path


class Glossary:
    def __init__(self) -> None:
        self.rules: list[tuple[str, str, str, str]] = []  # (old, new, mode, source)
        self.hits: dict[int, int] = {}
        self._exact: dict[str, tuple[str, int]] = {}
        self._subs: list[tuple[str, str, int]] = []
        self._res: list[tuple[re.Pattern[str], str, int]] = []

    @classmethod
    def load(cls, path: str | Path) -> "Glossary":
        g = cls()
        p = Path(path)
        if not p.exists():
            raise FileNotFoundError(f"词表不存在: {p}")
        for lineno, raw in enumerate(p.read_text(encoding="utf-8").splitlines(), 1):
            line = raw.rstrip("\r")
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            parts = line.split("\t")
            if len(parts) < 2:
                raise ValueError(f"{p}:{lineno} 词表行至少需要两列（旧词<TAB>新词）")
            old, new = parts[0], parts[1]
            mode = (parts[2].strip().lower() if len(parts) > 2 and parts[2].strip() else "exact")
            source = parts[3].strip() if len(parts) > 3 else "-"
            if mode not in ("exact", "sub", "re"):
                raise ValueError(f"{p}:{lineno} 未知 mode: {mode}")
            idx = len(g.rules)
            g.rules.append((old, new, mode, source))
            g.hits[idx] = 0
            if mode == "exact":
                g._exact[old] = (new, idx)
            elif mode == "sub":
                g._subs.append((old, new, idx))
            else:
                g._res.append((re.compile(old), new, idx))
        g._subs.sort(key=lambda r: -len(r[0]))
        return g

    def __len__(self) -> int:
        return len(self.rules)

    def apply(self, text: str) -> str | None:
        """对含中文的字段应用整条词表；无变化返回 None。"""
        if text in self._exact:
            new, idx = self._exact[text]
            if new != text:
                self.hits[idx] += 1
                return new
        out = text
        for old, new, idx in self._subs:
            if old in out:
                self.hits[idx] += out.count(old)
                out = out.replace(old, new)
        for pat, repl, idx in self._res:
            out2, n = pat.subn(repl, out)
            if n:
                self.hits[idx] += n
                out = out2
        return None if out == text else out

    def apply_exact(self, text: str) -> str | None:
        """纯 ASCII 字段只做整字段匹配，避免误伤资源路径与内部标识符。"""
        hit = self._exact.get(text)
        if hit is None or hit[0] == text:
            return None
        self.hits[hit[1]] += 1
        return hit[0]

    def report(self) -> list[str]:
        rows = []
        for idx, (old, new, mode, source) in enumerate(self.rules):
            rows.append(f"{old}\t{new}\t{mode}\t{source}\t{self.hits[idx]}")
        rows.sort(key=lambda row: -int(row.rsplit("\t", 1)[1]))
        return rows
