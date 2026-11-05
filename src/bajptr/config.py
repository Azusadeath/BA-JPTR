"""默认配置：日服（JP）区的密钥、路径与表过滤。"""

# 日服 ExcelDB 的 SQLCipher 密钥（社区公开端点 https://ba.zmkimu.com/jp 发布，可用 --key 覆盖）。
DEFAULT_KEY = "a74336c72d78fb60cd9a8ad9b74bddea14af94dd7c331cee49732841fdc68498"
KEY_URL = "https://ba.zmkimu.com/jp"

# 可能包含玩家可见文本的表（正则，忽略大小写）。日服实际命中 56 张（LocalizeDBSchema、
# CharacterDialogSubtitleDBSchema、ScenarioScriptDBSchema、AcademyMessangerDBSchema 等）。
TABLE_PATTERN = r"Localize|Dialog|Scenario|Subtitle|Talk|Mail|Name|Emoji|Voice|Messanger|SNSProfile"

STREAMING_REL = "BlueArchive_Data/StreamingAssets"
TABLEBUNDLES_REL = STREAMING_REL + "/TableBundles"
CATALOG_REL = TABLEBUNDLES_REL + "/Catalog/TableCatalog.bytes"
CATALOG_HASH_REL = TABLEBUNDLES_REL + "/Catalog/TableCatalog.hash"

# catalog 里表格条目的名字（游戏内部表名，不是磁盘文件名）
EXCELDB_ENTRY = "ExcelDB.db"

# 日服安装路径的卸载注册表项（Yostar 启动器写入）
JP_REGISTRY_KEY = (
    r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
    r"\e02a2fab-b426-5ce2-b9de-b9e7506c327e"
)

DEFAULT_OUT_DIR = "output"
DEFAULT_BACKUP_DIR = "backup"

# SQLCipher 页参数（实测日服库 2026-11：页 4096 / 保留 80 / IV 16 / HMAC 64 / PBKDF2-SHA512 2 轮）
PAGE_SIZE = 4096
RESERVE = 80
IV_SIZE = 16
HMAC_SIZE = 64
KDF_ITER = 2
KDF_SALT_MASK = 0x3A
USABLE = PAGE_SIZE - RESERVE
