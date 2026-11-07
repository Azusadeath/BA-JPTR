use crate::{Result, invalid, storage};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
pub const KEY: &str = "a74336c72d78fb60cd9a8ad9b74bddea14af94dd7c331cee49732841fdc68498";
pub const BUNDLES: &str = "BlueArchive_Data/StreamingAssets/TableBundles";
pub const CATALOG: &str =
    "BlueArchive_Data/StreamingAssets/TableBundles/Catalog/TableCatalog.bytes";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub size_offset: usize,
    pub crc_offset: usize,
    pub size: u64,
    pub crc: u32,
}
pub fn parse(bytes: &[u8]) -> Result<Entry> {
    if bytes.len() < 5 || bytes[0] != 2 {
        return Err(invalid("资源目录格式不受支持，请检查游戏资源或更新工具。"));
    }
    let count = i32::from_le_bytes(bytes[1..5].try_into().unwrap());
    if !(1..=100000).contains(&count) {
        return Err(invalid("资源目录条目数量无效。"));
    }
    let name = b"ExcelDB.db";
    let mut string = Vec::new();
    string.extend_from_slice(&(!(name.len() as i32)).to_le_bytes());
    string.extend_from_slice(&(name.len() as i32).to_le_bytes());
    string.extend_from_slice(name);
    let mut prefix = string.clone();
    prefix.push(8);
    prefix.extend_from_slice(&string);
    let mut found = bytes[5..]
        .windows(prefix.len())
        .enumerate()
        .filter(|(_, b)| *b == prefix);
    let start = found
        .next()
        .ok_or_else(|| invalid("资源目录中没有支持布局的 ExcelDB.db 条目。"))?
        .0
        + 5;
    if found.next().is_some() {
        return Err(invalid("ExcelDB.db 条目不唯一，拒绝猜测。"));
    }
    let at = start + prefix.len();
    if at + 16 > bytes.len() {
        return Err(invalid("资源目录条目越界。"));
    }
    let size = i64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    let crc = i64::from_le_bytes(bytes[at + 8..at + 16].try_into().unwrap());
    if size <= 0 || !(0..=u32::MAX as i64).contains(&crc) {
        return Err(invalid("资源目录大小或 CRC 无效。"));
    }
    Ok(Entry {
        size_offset: at,
        crc_offset: at + 8,
        size: size as u64,
        crc: crc as u32,
    })
}
pub fn version(path: &Path) -> Result<String> {
    let mut bytes = fs::read(path)?;
    let entry = parse(&bytes)?;
    bytes[entry.size_offset..entry.size_offset + 8].fill(0);
    bytes[entry.crc_offset..entry.crc_offset + 8].fill(0);
    bytes.extend_from_slice(b"\0VERSION\0");
    let hash = path.with_file_name("TableCatalog.hash");
    if hash.is_file() {
        bytes.extend(fs::read(hash)?);
    }
    Ok(storage::digest(&bytes))
}
pub struct Game {
    pub dir: PathBuf,
    pub catalog: PathBuf,
    pub bytes: Vec<u8>,
    pub entry: Entry,
    pub bundle: PathBuf,
}
pub fn locate(value: &str) -> Result<Game> {
    if value.trim().is_empty() {
        return Err(invalid("请先选择游戏安装目录。"));
    }
    let dir = storage::resolved(Path::new(value.trim()))?;
    let catalog = dir.join(CATALOG);
    if !catalog.is_file() {
        return Err(invalid(
            "这个文件夹不是日服游戏目录，请选择包含 BlueArchive_Data 的文件夹。",
        ));
    }
    let bytes = fs::read(&catalog)?;
    let entry = parse(&bytes)?;
    let suffix = format!("_{}", entry.crc);
    let mut candidates = Vec::new();
    for item in fs::read_dir(dir.join(BUNDLES))? {
        let item = item?;
        if item.file_name().to_string_lossy().ends_with(&suffix)
            && item.file_type()?.is_file()
            && item.metadata()?.len() == entry.size
        {
            candidates.push(item.path());
        }
    }
    if candidates.len() != 1 {
        return Err(invalid(
            "游戏资源与目录记录不一致或存在多个候选文件，请先修复或更新游戏。",
        ));
    }
    Ok(Game {
        dir,
        catalog,
        bytes,
        entry,
        bundle: candidates.remove(0),
    })
}
pub fn publish_catalog(game: &Game, size: u64, crc: u32) -> Result<()> {
    if fs::read(&game.catalog)? != game.bytes {
        return Err(invalid("游戏资源目录已变化，请重新预览。"));
    }
    let mut bytes = game.bytes.clone();
    bytes[game.entry.size_offset..game.entry.size_offset + 8]
        .copy_from_slice(&(size as i64).to_le_bytes());
    bytes[game.entry.crc_offset..game.entry.crc_offset + 8]
        .copy_from_slice(&(crc as i64).to_le_bytes());
    storage::write_locked(&game.catalog, &bytes)?;
    Ok(())
}
pub fn resource_identity(game: &Game) -> Result<Value> {
    source_identity(game, &game.bundle)
}
pub fn source_identity(game: &Game, bundle: &Path) -> Result<Value> {
    let meta = bundle.metadata()?;
    let mut file = fs::File::open(bundle)?;
    let mut hasher = Sha256::new();
    hasher.update(&game.bytes);
    let mut first = vec![0; meta.len().min(4096) as usize];
    file.read_exact(&mut first)?;
    hasher.update(&first);
    file.seek(SeekFrom::Start(meta.len().saturating_sub(4096)))?;
    let mut last = Vec::new();
    file.read_to_end(&mut last)?;
    hasher.update(&last);
    let ns = |time: std::io::Result<std::time::SystemTime>| {
        time.ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|t| t.as_nanos() as u64)
            .unwrap_or(0)
    };
    Ok(json!([
        storage::resolved(bundle)?.to_string_lossy(),
        meta.len(),
        ns(meta.modified()),
        ns(meta.created()),
        game.entry.crc,
        format!("{:x}", hasher.finalize())
    ]))
}
