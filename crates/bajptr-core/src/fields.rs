//! Byte-preserving FlatBuffer patching, ported from the project's GPL-3.0 implementation.
use crate::{Error, Result};
use serde::Serialize;

pub const MAX_STRING: usize = 1 << 20;

pub fn has_cjk(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c as u32, 0x3400..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x2fa1f))
}
pub fn has_kana(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c as u32, 0x3040..=0x30ff | 0x31f0..=0x31ff | 0xff66..=0xff9f))
}
pub fn forbidden(text: &str) -> bool {
    text.chars().any(|c| matches!(c as u32, 0..=9 | 11..=31))
}
fn u16_at(b: &[u8], at: usize) -> usize {
    u16::from_le_bytes([b[at], b[at + 1]]) as usize
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

#[derive(Debug, Clone, Serialize)]
pub struct Field<'a> {
    pub slot: usize,
    pub fpos: usize,
    pub spos: usize,
    pub text: &'a str,
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldIssue {
    pub slot: usize,
    pub message: String,
}
pub struct Parsed<'a> {
    pub fields: Vec<Field<'a>>,
    pub issues: Vec<FieldIssue>,
    pub skipped: u64,
}
pub fn parse(blob: &[u8], allow_ascii: bool) -> Result<Vec<Field<'_>>> {
    Ok(parse_with_issues(blob, allow_ascii)?.fields)
}
pub fn parse_with_issues(blob: &[u8], allow_ascii: bool) -> Result<Parsed<'_>> {
    let invalid = |message: &str| Error::Parse(message.into());
    let n = blob.len();
    if n < 16 {
        return Err(invalid("blob too small"));
    }
    let root = u32_at(blob, 0) as usize;
    if root < 4 || root > n - 4 {
        return Err(invalid("root out of range"));
    }
    let delta = i32::from_le_bytes(blob[root..root + 4].try_into().unwrap()) as i64;
    let vt = root as i64 - delta;
    if vt < 0 || vt > (n - 4) as i64 {
        return Err(invalid("vtable out of range"));
    }
    let vt = vt as usize;
    let len = u16_at(blob, vt);
    if len < 4 || !len.is_multiple_of(2) || vt.checked_add(len).is_none_or(|end| end > n) {
        return Err(invalid("bad vtable length"));
    }
    let object = u16_at(blob, vt + 2);
    if object < 4 || root.checked_add(object).is_none_or(|end| end > n) {
        return Err(invalid("table object out of range"));
    }
    if !(vt + len <= root || vt >= root + object) {
        return Err(invalid("vtable overlaps table object"));
    }
    let mut out = Vec::new();
    let mut issues = Vec::new();
    let mut skipped = 0;
    for k in 2..len / 2 {
        let off = u16_at(blob, vt + k * 2);
        if off == 0 {
            continue;
        }
        if off < 4 || off >= object {
            skipped += 1;
            if issues.len() < 20 {
                issues.push(FieldIssue {
                    slot: k - 2,
                    message: "字段偏移超出表对象，已保留该槽的原始数据。".into(),
                });
            }
            continue;
        }
        let fpos = root + off;
        if fpos + 4 > root + object {
            continue;
        }
        let Some(spos) = fpos.checked_add(u32_at(blob, fpos) as usize) else {
            continue;
        };
        if spos < 4 || spos > n - 5 {
            continue;
        }
        let size = u32_at(blob, spos) as usize;
        if size > MAX_STRING {
            continue;
        }
        let Some(end) = spos.checked_add(4 + size) else {
            continue;
        };
        if end >= n || blob[end] != 0 {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&blob[spos + 4..end]) else {
            continue;
        };
        if (!allow_ascii && !has_cjk(text)) || forbidden(text) {
            continue;
        }
        out.push(Field {
            slot: k - 2,
            fpos,
            spos,
            text,
        });
    }
    Ok(Parsed {
        fields: out,
        issues,
        skipped,
    })
}

#[derive(Default, Debug, Serialize)]
pub struct BlobStats {
    pub inplace: u64,
    pub appended: u64,
    pub bytes_appended: u64,
    pub kana_skipped: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub fields_skipped: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<FieldIssue>,
}
fn is_zero(value: &u64) -> bool {
    *value == 0
}
