# 开发与验证

普通用户用法见 [README](../README.md)。本文件保留构建、数据格式和回归验证说明，不进入用户发行包。

Windows 原生工具。GUI、项目服务、命令行、启动器和正则均使用 Rust，运行、构建与打包不需要 Python。
当前仅面向已安装汉化补丁的 Windows x64 日服 PC 客户端；原始日服未测试，国际服和移动端不支持。
本工具很大程度受到 [Agent-0808](https://github.com/Agent-0808) 的 [BATR](https://github.com/Agent-0808/BA-Term-Replacer) 启发，文本字段处理参考并移植其相关实现，并另行适配日服资源布局，沿用 GPL-3.0-only 许可。

## 使用

双击 **bajptr.exe**。启动器和主程序均已嵌入多尺寸图标，便携包解压后直接运行。
手动选择包含 `BlueArchive_Data` 的游戏目录。首次使用时确认将当前游戏文本设为初始底稿，再添加词条或导入 TSV，点击 **预览并应用**，后台准备补丁就绪后，在预览窗口确认 **应用到游戏**。
不读取安装记录或自动寻找游戏路径；已手动选择的目录会保存，下次启动自动检查该目录的资源和底稿。
界面不提供独立生成补丁入口；应用时若生成、校验通过而写入游戏失败，自动将完整补丁保存到 `output/` 并提示原因和位置，不标记为已应用。词条自动保存，撤销、重做、导入、导出通过按钮操作。
Ctrl / Shift 用于列表多选；没有 Ctrl+S 等应用命令快捷键。
查找关键词框聚焦时可回车执行查找，输入法组词与提交候选时不触发；应用与恢复仍须各自确认。
布局验收可用环境变量 `BAJPTR_GUI_SIZE=900x620` 指定初始内容尺寸（不得小于最小尺寸）；未设置时仍为 1080×680。仅控制本次窗口，不写设置文件。
文本输入的 Ctrl+C / Ctrl+X / Ctrl+V 使用系统剪贴板。eframe 关闭默认功能时须显式启用 clipboard；否则只会使用进程内部的模拟剪贴板，无法与其他应用交换文本。

每次使用当前完整词表与单条修改，从本版本唯一的**初始底稿**重新生成。
先应用“暴击→爆炸”，再改词条为“暴击→袭击”，下次会得到“袭击”。删除词条后再次应用会撤销其旧效果。
同一轮只匹配原文，不级联处理新文字：整段相同优先，包含文字按最长原词优先，正则按词表顺序处理未占用片段。
例如“暴击→爆炸”和“暴击伤害→毁灭”分别得到“爆炸”和“毁灭”。

**来源表**支持中文用途名称、多选、筛选、全选、清空、反选。筛选后的操作只影响可见项。
GUI 仅保存明确的来源范围；清空选择后不能保存，不提供“不限来源”。全选、清空、反选均作用于当前列表，筛选时保留隐藏选择。
首次兼容旧词条时，按同一游戏上次记录的来源表固定范围，保留专用规则覆盖关系；完全被覆盖的旧词条保留为空固定集合 `[]`，不参与替换。迁移前保留词表副本，缺少可靠旧记录时不猜测范围。
`.gui/source-history.json` 按游戏目录记录已知来源表和待确认命中。确认新底稿后只扫描新增表，按包含、整段相同或正则方式在未替换原文上检测，并遵循假名跳过选项；提醒不会修改任何来源范围。提醒可进入词条编辑，未标记已读时重启仍保留。扫描失败不把新增表标记为已检查。
进入恢复页仍检查底稿；同一游戏、同一底稿已完成来源同步时复用该结果，避免切页重复写来源记录并使目录按钮闪灰。更换游戏目录、保存底稿或底稿版本标识变化后重新同步；实际同步和写入期间保留按钮禁用保护。
CLI 仍兼容旧词表的全局规则；同原词、同方式的指定来源规则覆盖该来源内的全局规则，相交范围的重复规则会报错。

## 查找与单条修改

查找读取底稿原文，结果显示通用词表和单条修改共同生成的本次文本。双击结果编辑完整文本，保存后再预览、应用。
Enter 可换行，留空可清空该段。只打开再保存、没有改动时，不创建额外修改。
Ctrl 点选、Shift 连选后使用 **替换选中文本**，预览确认后保存，只影响实际选中的结果。
搜索统计包含全部命中；界面每次加载 500 条，可点击“加载更多”继续查看，不会把未加载的结果当成不存在。搜索遵循跳过日文假名选项。未搜索或关键词改动后，来源选项列出全部表；搜索完成后，按完整匹配集合提供来源表，不受分页和当前所选来源限制。扫描返回 matching_tables，项目层按关键词与跳过假名选项缓存全局命中表；更换底稿时缓存失效。修改跳过假名选项会清空已有搜索。来源表筛选会自动重新查找指定表，批量替换只作用于当前已加载并选中的文字。

关键词输入一旦改变，立即清空 GUI 旧结果和选择，取消/忽略未完成搜索，不采用逐字整库扫描。后续记录更新不得重新带回已清空的旧搜索。替换、查找及恢复滚动区域使用独立 ID，查找按搜索 nonce 隔离位置。
批量操作保存查找词、替换词和所选文本定位，来源列标注“批量修改”。同一游戏、同一资源版本中，保存相同查找词和替换词会合并为主表中的一条替换项，范围为所选文本的并集；重复选择沿用每段操作前的文本，不重复替换上次的结果。编辑合并条目时重算整个合并范围，若改为另一已有替换也会合并；撤销一次保存只回退该次新增或调整的范围。早期没有操作信息的记录保持兼容，不自动归组。
单条修改绑定游戏、版本、来源表、记录、字段与底稿原文，优先于通用词表，不受跳过日文选项影响。
同一字段允许一项单条记录与一项批量记录并存，同类重复仍拒绝；应用前按字段汇总，批量 replacement 是最终值。批量保存不覆盖已有单条，单条保存会更新对应批量的 before 并重算 replacement；删除单条用当前词表结果重算批量，删除批量仅移除批量记录。保持整段覆盖语义，不做局部差异或词表重放。格式仍为 2，旧程序遇到同字段双记录会拒绝读取，应使用新版维护此类文件。
批量文本范围读取已保存的完整成员，不依赖搜索结果分页。支持按原文、before、replacement、来源表筛选，完整内容在选择后显示。GUI 不提供复制定位、记录/字段编号展示或编号筛选；底层仍用完整记录定位执行修改与校验。remove_batch_member 强制校验 revision、当前游戏、批量 ID 和成员位置，只移除该批量记录；保留同字段单条记录及其他成员。最后一段移除后批量条目消失；每次移除进入共用撤销记录，需预览应用才写入游戏。
单条管理仅一条时直接呈现前后全文对照，无需选中列表；多条时搜索与数量同排，左侧虚拟摘要列表、右侧全文对照。筛选或移除后按真实 edit index 选择仍可见的条目。编辑为主按钮，移除为普通次级按钮，关闭靠右；沿用独立编辑器及原有保存、撤销语义。
游戏更新后旧版本记录保留但失效，不套用到新资源。词表和单条修改共用本次打开期间的 100 步撤销、重做。

## 正则模板

点击 **正则模板…**，填写普通文字即可：多个叫法统一、只替换开头、只替换结尾、替换两段文字之间、改名称并保留数字。
每项都有说明、示例与试写。保存后可继续填写模板，导入的外部表达式不会被猜测还原。
常用分组、环视、反向引用、Unicode 字符类、行末与空匹配由 Rust 直接执行。
替换分组仍使用 `\1`、`\g<name>`，`$` 保持字面含义。
复杂表达式有回溯和输出长度限制，超限会停止处理，禁止发布不完整补丁。
未支持的语法明确报错，例如字符名形式 `\N{…}` 应改为对应文字；没有 Python 回退，不承诺所有正则方言完全相同。

## 底稿与保护

底稿包含该版本首次使用工具时的完整加密数据库、目录记录和版本标记，包含当时已安装的汉化内容，不代表官方日文资源。
本版本只使用已保存的当前格式底稿。没有底稿时先弹窗询问，确认后才保存并验证；关闭或取消弹窗不会创建底稿。
启动、切换目录或点击 **检查底稿** 时检查资源版本；发现新版时提示更新，同版本内容被外部更改时也提示确认。
工具自身应用替换后的资源不会被误当作更新。新底稿通过 HMAC、CRC 和 SQLite 完整性校验后才移出旧底稿；失败时保留旧底稿。
更新提示打开后资源再次变化，会要求重新检查。缺失底稿且当前资源已被本工具修改时，禁止把替换结果重新保存成初始底稿。
具体行为与验证范围见 [底稿确认与更新](BASELINE_CONFIRMATION.md)。
**恢复**页可查看和恢复底稿，词表与单条修改保留，不创建逐次恢复记录。
新版底稿验证通过后才移出旧版；底稿损坏或不完整时停止，不拿当前已修改资源重新建立替代品。

| 位置 | 内容 |
| --- | --- |
| `.gui/rules.tsv` | 通用词表 |
| `.gui/text-edits.json` | 单条及选中批量修改 |
| `.gui/settings.json` | 目录和选项 |
| `.gui/template-drafts.json` | 模板表单 |
| `.gui/application-state.json` | 应用状态摘要 |
| `.gui/tables-cache.json` | 可失效的表名缓存 |
| `backup/_baselines/` | 按游戏及版本隔离的底稿 |
| `output/` | 生成的补丁 |

本机旧数据已一次性转换。运行时只读取统一的 format 2 JSON 与五列 TSV，不保留旧格式解析。
坏词表不会被当成空表保存；跨进程锁和修订检查防止旧窗口覆盖新内容。
安装前重新核对资源和修改记录；明文库通过 SQLite 完整性检查后加密，发布暂存文件通过每页 HMAC 与 CRC 校验后才切换资源与目录字段。
恢复只切换 size / CRC，不回滚版本 hash，不按前缀删除其他资源。请先退出游戏再应用或恢复。

## 响应与缓存

GUI 直接调用同进程 Rust 后台服务，保留即时反馈、取消和旧结果显示。
数据库保持只读临时明文文件，SQLite 页缓存上限 64 MiB，不再把全库复制到内存；查找与预览结果分别缓存，按容量与数量限制淘汰。
当前游戏差异文字缓存上限 16 MiB，超限流式比较。变化预览展示前 200 处，统计包含全部变化。

GUI 在收到有效 preview 后立即显示结果，并向同一后台 Worker 提交可取消的 prepare_patch。准备结果仅保存在 work/prepared，不发布到游戏或用户 output；完整性、HMAC 和 CRC 校验沿用生成流程。Engine 缓存至多一份加密补丁，不保留成功构建的明文工作库；缓存绑定规则、编辑、选项、底稿及游戏资源身份。关闭预览后继续准备并保留，当前会话再次预览可复用；修改记录或选项时清理，退出 Engine 时清理。GUI apply 显式要求已准备补丁，确认时核对文档、底稿、资源身份及准备文件，再沿用同一发布/失败导出路径，不重新重建加密。未准备就绪、资源变化或准备文件损坏时拒绝提交。原 CLI generate/apply 路径仍可直接生成。
生成和安装重新读取并验证。成功后由带标记验证的独立 Rust 进程整理临时文件，耗时清理不阻塞结果返回；失败暂存内容保留供恢复。
旧底稿和临时文件默认移入 Windows 回收站。可用 `BAJPTR_TRASH_DIR` 指定其他目录，不可用时仍回退到回收站。

## 便携运行与发布检查

解压完整便携包到可写目录，再双击 `bajptr.exe`；不要只复制一个 EXE，也不要直接在压缩包中运行。
不需要安装 Python、Rust、.NET 或额外 GCC 运行库。SQLite 静态编译，缺少系统中文字体时使用内嵌字体。
运行与数据保存需要便携目录可写；应用到游戏需要游戏资源目录可写。
本轮实测系统为 Windows 11 x64；其他 Windows 版本、ARM、macOS 和 Linux 尚未验证。
隔离进程检查不能代替全新 Windows 虚拟机测试。检查记录、国际服差异及复核步骤见 [发布前检查](RELEASE_CHECK.md)。

## 构建与验证

开发时先执行受影响的包或测试，最终验收再完整运行一次测试、Lint 和构建。测试沿用 release 配置，避免切换配置导致重复编译；测试通过且代码未变时无需重复跑全套。

```powershell
powershell -NoProfile -File tools/build.ps1 -Test -Package bajptr-project -TestTarget workflow -TestFilter source_filter
powershell -NoProfile -File tools/build.ps1 -Test -Package bajptr-desktop -TestFilter scope
```

`-Package` 选择包，`-TestTarget` 选择集成测试文件，`-TestFilter` 按名称筛选测试（后两项要求 `-Test`）。来源筛选等同一场景优先复用既有夹具；不为窗口宽度、文案或私有实现常量增加镜像测试。写入、撤销、取消、损坏资源与恢复等行为回归保留。

需要 Rust 1.95+，以及所选 Windows 目标对应的 C 工具链和资源编译器（GNU：MinGW/GCC、windres；MSVC：C++ Build Tools、Windows SDK）。
本轮使用 x86_64-pc-windows-gnu 构建。存在 `.toolchain/` 时自动使用项目工具链，否则使用系统 Cargo；首次构建需下载锁定依赖。SQLite 随项目编译。

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build.ps1 -Test
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build.ps1 -Lint
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build.ps1 -FormatCheck
powershell -NoProfile -ExecutionPolicy Bypass -File tools/package.ps1 -Destination D:\bajptr-portable
powershell -NoProfile -ExecutionPolicy Bypass -File tools/package-source.ps1 -Destination D:\bajptr-source
```

默认锁定 Cargo.lock，只有有意更新依赖时使用 `-UpdateLock`。
二进制按摘要发布，再原子更新 `native/engine.json`，不覆盖运行中的版本。
便携包不包含个人词表、底稿、工具链或 Python。
`tools/verify-portable.ps1 -Portable <便携目录> -WorkDir <新测试目录>` 检查空白项目首次启动、保存和导出。
`tools/smoke.ps1 -Portable <便携目录> -WorkDir <新测试目录>` 完整检查模拟游戏。两者都清空子进程环境变量，仅保留必要 Windows 变量，并使用独立用户目录。

## 命令行

```powershell
$cli = Join-Path $PWD ('native/' + (Get-Content native/engine.json -Raw | ConvertFrom-Json).cli)
& $cli --help
& $cli paths --game-dir E:\YostarGames\BlueArchive_JP
& $cli backup --game-dir E:\YostarGames\BlueArchive_JP
& $cli patch --game-dir E:\YostarGames\BlueArchive_JP --glossary .gui/rules.tsv --dry-run
& $cli patch --game-dir E:\YostarGames\BlueArchive_JP --glossary .gui/rules.tsv --install
& $cli restore --game-dir E:\YostarGames\BlueArchive_JP
& $cli scan --game-dir E:\YostarGames\BlueArchive_JP --keyword 任务
& $cli export --game-dir E:\YostarGames\BlueArchive_JP --out output/export
```

CLI patch 只处理指定 TSV，不自动包含 GUI 单条修改。支持 `--tables`、`--backup-dir`、`--keep-kana`、`--no-ascii-exact`、`--work-dir`。
CLI 必须显式指定 `--game-dir`；没有当前版本底稿时先执行 `backup`，`patch` 和 `restore` 不会隐式建立底稿。
直接数据库处理使用 `--db`，可配合 `--key`；此时不能安装到游戏。
校验始终执行，底稿始终包含数据库。旧参数 `--verify`、`--with-db`、`--prune` 和历史恢复参数 `--backup` 已移除。

## 词表格式

UTF-8 TSV，可带 BOM，忽略空行和 `#` 注释。固定五列：原词、替换词、方式、备注、来源表。
方式必须填写：`exact` 为整段相同，`sub` 为包含文字，`re` 为正则。
第五列使用完整英文表名或 JSON 字符串数组，区分大小写，不是通配符。`[]` 表示保留记录但不选择任何来源。旧格式的留空全局规则仍可读取，GUI 在来源核对时迁移为固定范围；CLI 保留旧格式兼容。
默认跳过整段含假名的文本；纯 ASCII 只做整段相同，保护资源路径。贴图文字无法修改，游戏更新后需重新应用。
更多架构与验证信息见 [Rust 迁移说明](RUST_MIGRATION.md)。
