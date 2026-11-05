# bajptr — 蔚蓝档案日服（JP）译名替换工具

从 [BATR](https://github.com/Agent-0808/BA-Term-Replacer)（GL 服）移植的日服版本。
CLI 形态一致（patch / backup / restore / scan / export / paths），差异在于：

* **密钥**：日服密钥 `a74336c7...`（完整值见 `src/bajptr/config.py`，可用 `--key` 覆盖）
* **库路径**：日服库名是 `<表哈希>_<crc32>`（每次更新都变），必须先从
  `BlueArchive_Data/StreamingAssets/TableBundles/Catalog/TableCatalog.bytes` 读 `ExcelDB.db` 条目的
  size/crc，再据此找到磁盘文件
* **加解密**：不依赖 `sqlcipher3`（Windows 上没有 wheel），改用纯 Python 的页级
  AES-256-CBC + HMAC-SHA512 实现（`crypt.py`），实测 53,054 页全量 HMAC 校验通过
* **安装**：`--install` 不改原库，而是写出 `<同前缀>_<新crc>` 并把 catalog 的 size/crc
  同步改掉（汉化补丁用的就是这套机制，已实测游戏内生效）；`restore` 可一键还原

## 安装

```powershell
pip install pycryptodome
$env:PYTHONPATH = "$PWD\src"      # 或者 pip install -e .
```

## 用法

```powershell
# 看探测到的路径
python -m bajptr paths --game-dir E:\YostarGames\BlueArchive_JP

# 备份 catalog（安装前的安全网，几 KB）
python -m bajptr backup --game-dir E:\YostarGames\BlueArchive_JP

# 先空跑：只统计命中，不写任何文件
python -m bajptr patch --game-dir E:\YostarGames\BlueArchive_JP --glossary glossary\example.tsv --dry-run

# 输出到 output/（不碰游戏目录）
python -m bajptr patch --game-dir E:\YostarGames\BlueArchive_JP --glossary glossary\example.tsv --verify

# 真正装进游戏（自动备份 catalog + 同步 size/crc）
python -m bajptr patch --game-dir E:\YostarGames\BlueArchive_JP --glossary glossary\example.tsv --install --verify

# 还原
python -m bajptr restore --game-dir E:\YostarGames\BlueArchive_JP --backup backup\<时间戳>
```

## 词表格式

TSV，UTF-8，空行与 `#` 注释忽略：

```
旧词<TAB>新词                # mode 缺省 exact：整字段相等才替换
旧词<TAB>新词<TAB>sub        # 子串替换（同一字段内全部命中，最长优先）
旧词<TAB>新词<TAB>re         # 正则替换（旧词列是正则，新词列可含 \1 反向引用）
旧词<TAB>新词<TAB>exact<TAB>来源  # 第四列只是备注，会进命中日志
```

运行结束会打印每条规则的命中数：**零命中优先排查**（拼错、词条已过期、或该文本不在被扫描的表里）。

## 已知边界

* 含平/片假名的字段默认跳过（`--keep-kana` 可关）——那是没被汉化覆盖的残留日文
* 纯 ASCII 字段只做整字段匹配，避免误伤资源路径（`UIs/03_Scenario/...`）
* 剧情/字幕在 `ScenarioScriptDBSchema`、`CharacterDialogSubtitleDBSchema` 等表里，表名正则已覆盖
* 游戏更新会重下库与 catalog，改完要重打
* 星级/图片类 UI 文本不在库里（是贴图），本工具改不到

## 日服事实（逆向结论，2026-11 实测）

| 项 | 值 |
|---|---|
| 密钥 | `a74336c72d78fb60cd9a8ad9b74bddea14af94dd7c331cee49732841fdc68498`（来源 `https://ba.zmkimu.com/jp`） |
| 加密参数 | SQLCipher 4：page 4096 / reserve 80 / IV 16 / HMAC-SHA512 64 / KDF PBKDF2-SHA512(salt^0x3A, 2 轮) |
| 页面布局 | page1 = salt(16) + cipher(4000) + IV(16) + HMAC(64)；其余页 = cipher(4016) + IV(16) + HMAC(64)；HMAC 输入不含 salt，含 4 字节小端页号 |
| 库文件 | `TableBundles\<前缀>_<crc32>`（本机 6993339912994747134_2795825953，217,309,184 B / 53,054 页；前缀不是名字的 xxh64，保持不变即可） |
| catalog | `TableBundles\Catalog\TableCatalog.bytes`（MemoryPack，1,030,502 B，15 条）；`ExcelDB.db` 条目 size 在名字首次出现位置 +29、crc 在 +37（各 8 字节小端） |
| `TableCatalog.hash` | 内容是版本号（本机 `1168298209`），不是校验和，改库时不要动 |
| 表名 | `LocalizeDBSchema` / `LocalizeErrorDBSchema` / `CharacterDialogSubtitleDBSchema` / `ScenarioScriptDBSchema`(423,986 行) / `AcademyMessangerDBSchema` 等；BATR 的表名正则命中 56 张 |
| blob | FlatBuffer：root uoffset → vtable → 各字段 uoffset → `[u32 长度][UTF-8][0x00]`；同一条里可能并存中文与韩文 |

> BATR 的「CRC32 追加修正」在日服无效（`docs/HOW_IT_WORKS.md:309`、`:349` 已预判）：日服 catalog 记录文件大小，只能走改 catalog 的路径。

## 验证记录

在本机 `E:\YostarGames\BlueArchive_JP`（汉化补丁版，库内文本是中文+韩文，无日文）逐项跑过：

| 环节 | 结果 |
|---|---|
| `paths` | 定位到 `_2795825953`，53,054 页 |
| `--dry-run` | 464 张表 → 命中 56 张文本表，rows=577,381，changed=1,576 / fields=1,587，kana_skipped=29,150 |
| 等长替换 + `--verify` | 输出 `<前缀>_<新crc>`，回读 `integrity_check = ok` |
| 变长替换 | appended=1,587 / +133,084 B，库 217,309,184 → 218,824,704 B（53,424 页），`integrity ok` |
| `--install` | catalog size/crc 同步；新库 53,424 页 HMAC 0 失败 |
| `restore --prune` | catalog 与库文件均回到原状态 |
| `scan` / `export` | 关键词 297 处命中 / 单表导出 416 条 TSV |
| 端到端 | 早期手工版把 1,628 处「任务」改成「测试」，**进游戏后界面显示「测试」** → 客户端不校验库完整性、不回滚 catalog |

## 许可

`flatbuf.py` 移植自 BATR（GPL-3.0），本仓库同样以 GPL-3.0 分发。