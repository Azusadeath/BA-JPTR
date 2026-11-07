//! Shared engine. UI adapters own user confirmation and publish only verified output.
pub mod codec;
pub mod database;
pub mod fields;
pub mod protocol;
pub mod rules;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("FlatBuffer 解析失败：{0}")]
    Parse(String),
    #[error("来源表结构暂不支持：{0}")]
    UnsupportedTable(String),
    #[error("第 {index} 条词条（{original}）：{source}")]
    Rule {
        index: usize,
        original: String,
        source: Box<Error>,
    },
    #[error("字段 {slot}：{source}")]
    Field { slot: usize, source: Box<Error> },
    #[error("来源表 {table}，记录 {record}：{source}")]
    Record {
        table: String,
        record: String,
        source: Box<Error>,
    },
    #[error("{0}")]
    Regex(String),
    #[error("已取消处理。")]
    Cancelled,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Progress {
    pub message: String,
}

pub struct Control<'a> {
    pub callback: &'a dyn Fn(Option<&str>) -> bool,
}
impl Control<'_> {
    pub fn check(&self) -> Result<()> {
        if (self.callback)(None) {
            Ok(())
        } else {
            Err(Error::Cancelled)
        }
    }
    pub fn log(&self, value: &str) -> Result<()> {
        if (self.callback)(Some(value)) {
            Ok(())
        } else {
            Err(Error::Cancelled)
        }
    }
}

pub fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
