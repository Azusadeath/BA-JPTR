//! The game's existing page protocol, not a replacement encryption format.
use crate::{Control, Result, invalid};
use aes::Aes256;
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::NoPadding};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha512;
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};

pub const PAGE: usize = 4096;
pub const USABLE: usize = 4016;
const HEADER: &[u8; 16] = b"SQLite format 3\0";
type Hmac512 = Hmac<Sha512>;

#[derive(Debug, Serialize, Deserialize)]
pub struct Info {
    pub pages: u64,
    pub salt: String,
    pub ivs: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Inspection {
    pub ok: u64,
    pub bad: u64,
    pub crc: u32,
}

pub fn key(value: &str) -> Result<[u8; 32]> {
    let value = value.trim();
    if value.len() != 64 {
        return Err(invalid("密钥需要 64 个十六进制字符。"));
    }
    let mut out = [0u8; 32];
    for (i, part) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let s = std::str::from_utf8(part).map_err(|_| invalid("密钥格式无效。"))?;
        out[i] = u8::from_str_radix(s, 16).map_err(|_| invalid("密钥格式无效。"))?;
    }
    Ok(out)
}
fn hkey(key: &[u8; 32], salt: &[u8]) -> [u8; 32] {
    let masked: Vec<_> = salt.iter().map(|v| v ^ 0x3a).collect();
    let mut result = [0; 32];
    pbkdf2::pbkdf2_hmac::<Sha512>(key, &masked, 2, &mut result);
    result
}
fn mac(hkey: &[u8; 32], cipher: &[u8], iv: &[u8], page: u32) -> Hmac512 {
    let mut mac = Hmac512::new_from_slice(hkey).unwrap();
    mac.update(cipher);
    mac.update(iv);
    mac.update(&page.to_le_bytes());
    mac
}
fn pages(path: &Path) -> Result<u64> {
    let size = path.metadata()?.len();
    if size < PAGE as u64 || !matches!(size % PAGE as u64, 0 | 4) {
        return Err(invalid("加密数据库页不完整或尾部格式不受支持。"));
    }
    Ok(size / PAGE as u64)
}

pub fn inspect(path: &Path, key: &[u8; 32], control: &Control<'_>) -> Result<Inspection> {
    let count = pages(path)?;
    let mut stream = BufReader::new(File::open(path)?);
    let mut page = [0u8; PAGE];
    let mut key_mac = [0; 32];
    let mut crc = crc32fast::Hasher::new();
    let mut result = Inspection {
        ok: 0,
        bad: 0,
        crc: 0,
    };
    for number in 1..=count {
        if number % 128 == 1 {
            control.log(&format!("正在校验数据库：第 {number}/{count} 页…"))?;
            control.check()?;
        }
        stream.read_exact(&mut page)?;
        crc.update(&page);
        let off = if number == 1 {
            key_mac = hkey(key, &page[..16]);
            16
        } else {
            0
        };
        if mac(
            &key_mac,
            &page[off..USABLE],
            &page[USABLE..USABLE + 16],
            number as u32,
        )
        .verify_slice(&page[USABLE + 16..])
        .is_ok()
        {
            result.ok += 1;
        } else {
            result.bad += 1;
        }
    }
    let mut tail = Vec::new();
    stream.read_to_end(&mut tail)?;
    crc.update(&tail);
    result.crc = crc.finalize();
    Ok(result)
}

pub fn decrypt(input: &Path, output: &Path, key: &[u8; 32], control: &Control<'_>) -> Result<Info> {
    control.check()?;
    let count = pages(input)?;
    if input == output {
        return Err(invalid("输入和输出文件不能相同。"));
    }
    let mut stream = BufReader::new(File::open(input)?);
    let mut out = BufWriter::new(File::create(output)?);
    let mut page = [0u8; PAGE];
    let mut salt = [0; 16];
    let mut key_mac = [0; 32];
    let mut ivs = Vec::with_capacity(count as usize);
    for number in 1..=count {
        if number % 128 == 1 {
            control.log(&format!("正在解密数据库：第 {number}/{count} 页…"))?;
            control.check()?;
        }
        stream.read_exact(&mut page)?;
        let off = if number == 1 {
            salt.copy_from_slice(&page[..16]);
            key_mac = hkey(key, &salt);
            16
        } else {
            0
        };
        let iv: [u8; 16] = page[USABLE..USABLE + 16].try_into().unwrap();
        mac(&key_mac, &page[off..USABLE], &iv, number as u32)
            .verify_slice(&page[USABLE + 16..])
            .map_err(|_| {
                invalid(format!(
                    "数据库第 {number} 页 HMAC 校验失败，未接受此资源。"
                ))
            })?;
        cbc::Decryptor::<Aes256>::new(key.into(), (&iv).into())
            .decrypt_padded_mut::<NoPadding>(&mut page[off..USABLE])
            .map_err(|_| invalid("数据库密文长度无效。"))?;
        if number == 1 {
            if page[16..18] != [0x10, 0] || page[20] != 80 {
                return Err(invalid("数据库页大小或保留区与支持格式不符。"));
            }
            out.write_all(HEADER)?;
        }
        out.write_all(&page[off..USABLE])?;
        out.write_all(&[0; 80])?;
        ivs.push(B64.encode(iv));
    }
    out.flush()?;
    out.get_ref().sync_all()?;
    Ok(Info {
        pages: count,
        salt: B64.encode(salt),
        ivs,
    })
}

pub fn encrypt(
    input: &Path,
    output: &Path,
    key: &[u8; 32],
    salt: &[u8],
    ivs: &[Vec<u8>],
    control: &Control<'_>,
) -> Result<Info> {
    encrypt_checked(input, output, key, salt, ivs, control).map(|(info, _)| info)
}

/// Encrypt a database and return the inspection values established while
/// writing it. This avoids a second full read of the generated resource.
pub fn encrypt_checked(
    input: &Path,
    output: &Path,
    key: &[u8; 32],
    salt: &[u8],
    ivs: &[Vec<u8>],
    control: &Control<'_>,
) -> Result<(Info, Inspection)> {
    let size = input.metadata()?.len();
    if input == output || salt.len() != 16 || size < PAGE as u64 || size % PAGE as u64 != 0 {
        return Err(invalid("明文数据库页不完整、salt 长度无效或输出覆盖输入。"));
    }
    if ivs.iter().any(|iv| iv.len() != 16) {
        return Err(invalid("IV 长度必须是 16 字节。"));
    }
    let count = size / PAGE as u64;
    let key_mac = hkey(key, salt);
    let mut stream = BufReader::new(File::open(input)?);
    let mut out = BufWriter::new(File::create(output)?);
    let mut page = [0u8; PAGE];
    let mut crc = crc32fast::Hasher::new();
    for number in 1..=count {
        if number % 128 == 1 {
            control.log(&format!("正在加密数据库：第 {number}/{count} 页…"))?;
            control.check()?;
        }
        stream.read_exact(&mut page)?;
        let mut iv = [0; 16];
        if let Some(value) = ivs.get(number as usize - 1) {
            iv.copy_from_slice(value);
        } else {
            getrandom::fill(&mut iv).map_err(|_| invalid("无法生成随机 IV。"))?;
        }
        let off = if number == 1 { 16 } else { 0 };
        let len = USABLE - off;
        cbc::Encryptor::<Aes256>::new(key.into(), (&iv).into())
            .encrypt_padded_mut::<NoPadding>(&mut page[off..USABLE], len)
            .map_err(|_| invalid("明文页长度无效。"))?;
        if number == 1 {
            out.write_all(salt)?;
            crc.update(salt);
        }
        out.write_all(&page[off..USABLE])?;
        crc.update(&page[off..USABLE]);
        out.write_all(&iv)?;
        crc.update(&iv);
        let digest = mac(&key_mac, &page[off..USABLE], &iv, number as u32)
            .finalize()
            .into_bytes();
        out.write_all(&digest)?;
        crc.update(&digest);
    }
    out.flush()?;
    out.get_ref().sync_all()?;
    Ok((
        Info {
            pages: count,
            salt: B64.encode(salt),
            ivs: Vec::new(),
        },
        Inspection {
            ok: count,
            bad: 0,
            crc: crc.finalize(),
        },
    ))
}
