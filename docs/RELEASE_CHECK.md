# 0.3.1 发布前检查（历史候选包）

检查日期：2026-11-07。当前定位：Windows x64 日服便携版。未执行外部发布。

以下为历史候选包记录。当前版本已移除内嵌中文字体，改用 Windows 系统的微软雅黑或黑体；无法读取时提示修复字体并退出。下文 PowerShell 验证脚本已迁移为 `cargo xtask verify-portable` 和 `cargo xtask smoke`，历史命令保留作记录。当前实现及验证约定见 [开发说明](DEVELOPMENT.md)。

## 运行范围

发行包包含 Rust 启动器、桌面程序、命令行程序和许可材料，不包含 Python、工具链或个人配置。
SQLite 随程序编译，中文字体提供嵌入式后备。不依赖固定 C 盘字体位置或 E 盘垃圾箱。
必须完整解压到可写目录。应用、恢复需要游戏资源目录写权限；操作前退出游戏。

本轮验证在 Windows 11 家庭版 10.0.26300 x64 上完成隔离进程检查，不是全新 Windows 系统安装。
GUI 使用 OpenGL 渲染，需要可用的显卡驱动；虚拟机、基础显示驱动和受限远程会话仍需分别验证。
本机没有可用的 Windows Sandbox、Docker 或虚拟机运行环境，因此尚不能承诺所有干净 Windows 镜像均通过。
不把更换 APPDATA/USERPROFILE 当作系统级隔离；注册表、显卡驱动和系统组件仍来自宿主系统。

## 本轮修复

1. 空白项目没有游戏目录时，路径规范化原先报错，阻断首次保存；现在允许无游戏目录先编辑、保存词表。
2. 系统字体从 SystemRoot/WINDIR 定位，找不到中文字体时使用内嵌 Noto Sans SC，并检查真实字体字形。
3. 清理默认使用系统回收站；指定垃圾箱不可用时回退回收站，避免发布程序依赖开发机 E 盘。
4. 按实际 DLL 导入移除不需要的 GCC/pthread 运行库，补齐 GPL、字体与 Rust 依赖许可材料。

## 验证方式

- Rust 回归共 29 项：格式、词表防丢失、底稿验证、恢复、范围、多选、取消、正则及便携性。
- `tools/verify-portable.ps1`：独立包副本，无已有 .gui，清空进程环境变量，仅 System32 PATH，独立用户目录，从无关工作目录启动根启动器；检查 GUI 项目加载、首次保存、导出和实际加载模块。
- `tools/smoke.ps1`：同样清空环境，在合成游戏中完成检查、查找、单条修改、预览、生成、应用、导出和恢复；核对仅生成不改游戏、恢复后目录等于原始、用户修改记录保留。
- 缺少系统中文字体与垃圾箱不可用由针对性自动测试覆盖；没有修改宿主系统字体、盘符或真实游戏。
- 测量 first_ui_seconds 表示 Rust GUI 首次构建界面的时间，不包含操作系统启动进程前的耗时，也不代表真实冷缓存测试。

机器可读结果随发布候选件保存。独立进程检查结果和新系统实测应分别记录。

### 本轮实际结果

- 29 项 Rust 测试通过；Clippy（警告视为错误）、格式检查与 release 构建通过。
- 空白便携副本启动、首次保存及导出通过；测得首次 UI 调用与项目读取均约 0.196 秒（单次本机测量）。
- 合成游戏完整流程通过，生成结果验证成功，恢复后 catalog 与原始逐字节相同，修改记录保留。
- 三个 EXE 的直接 DLL 导入均为 Windows 系统 DLL，没有 Python、GCC 或 pthread 运行库依赖。
- 实际模块中发现宿主 Windhawk 及其窗口管理模块注入，`module_audit_clean=false`；它们不在直接导入列表中，也不在发行包内。未更改或禁用宿主软件。这项结果不能当作干净操作系统上的加载证据。
- 发行包扫描没有个人配置、底稿、Python、旧 CMD 启动器、开发工具链或额外 DLL。

## 国际服对照

对照 [BA-Term-Replacer](https://github.com/Agent-0808/BA-Term-Replacer)，固定提交
[`4e9165d821a5d275dcc22dedbcf222157b787dec`](https://github.com/Agent-0808/BA-Term-Replacer/tree/4e9165d821a5d275dcc22dedbcf222157b787dec)。
只读源码，没有运行该项目，也没有获得真实国际服资源进行验证。

两者核心原理相似：解密 SQLCipher 4 数据库，读取 SQLite 的 Bytes 列，解析 FlatBuffer 字符串字段；等长原位写入，变长追加字符串并修改相对偏移，最后重建加密资源。

| 环节 | 当前日服工具 | 上游国际服工具 |
| --- | --- | --- |
| 资源位置 | StreamingAssets/TableBundles 中带前缀和 CRC 的资源 | StreamingAssets/PUB/Resource/Preload/TableBundles/ExcelDB.db |
| 密钥 | 日服默认密钥 | 另一把国际服密钥 |
| 检测安装 | 0.3.1 使用 Yostar 安装记录；0.3.2 已改为手动选择 | Steam App 3557620 |
| 写入方式 | 新建 CRC 命名资源，更新 MemoryPack catalog 的大小与 CRC | 替换 ExcelDB.db，追加 CRC 修正使 CRC 保持不变 |
| 文件尾部 | 当前接受页尾余数 0 或 4 字节 | 上游默认追加 4 字节额外内容，再追加 4 字节 CRC 修正 |
| 文字转换 | 用户词表、正则、单条及选中批量修改 | 词表和 OpenCC 繁简转换 |

参照源码：[路径](https://github.com/Agent-0808/BA-Term-Replacer/blob/4e9165d821a5d275dcc22dedbcf222157b787dec/src/batr/utils/paths.py)、[数据库](https://github.com/Agent-0808/BA-Term-Replacer/blob/4e9165d821a5d275dcc22dedbcf222157b787dec/src/batr/db.py)、[字段处理](https://github.com/Agent-0808/BA-Term-Replacer/blob/4e9165d821a5d275dcc22dedbcf222157b787dec/src/batr/flatbuf.py)、[补丁流程](https://github.com/Agent-0808/BA-Term-Replacer/blob/4e9165d821a5d275dcc22dedbcf222157b787dec/src/batr/patch.py)。

结论：当前 GUI 不支持国际服。CLI 有 `--db`/`--key` 仅表示底层允许指定数据库及密钥，不等于支持国际服安装。
合成页测试证明国际服原始密钥可用于同类页加解密，但默认 8 字节尾部被拒绝；国际服布局测试确认日服定位器拒绝该布局，且不修改输入。
国际服适配应共享字段替换内核，另外实现资源定位、文件尾部和安装策略，再用真实国际服资源验证 HMAC、CRC、SQLite 完整性及游戏内加载。

## 全新 Windows 复核清单（待执行）

1. 在新建的 Windows x64 虚拟机或新电脑上，不安装 Python、Rust、.NET 开发环境或 GCC。
2. 将便携 ZIP 完整解压到含中文和空格的可写路径。双击 BA-JPTR.exe，确认中文、图标、首屏和文件对话框正常。
3. 未选游戏目录时添加词条、导出，关闭后重开，确认词条仍在。
4. 从配套源码运行两个验证脚本，Portable 指向便携包，WorkDir 各用一个全新目录；只使用脚本创建的模拟游戏。
5. 检查查找、预览、取消、应用、恢复及回收站行为；保存报告、系统版本和显卡信息。
6. 如需承诺 Windows 10 或其他环境，分别在该系统测试；本轮结果不自动覆盖它们。

通过以上复核前，可以交付本地发布候选包，不宜宣称已完成全新操作系统兼容性验证。
