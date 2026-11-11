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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

pub type LogObserver<'a> = dyn Fn(LogLevel, &str) + 'a;

#[derive(Clone, Copy)]
pub struct Control<'a> {
    pub callback: &'a dyn Fn(Option<&str>) -> bool,
    logger: Option<&'a LogObserver<'a>>,
    cancellable: bool,
}
impl<'a> Control<'a> {
    pub fn new(callback: &'a dyn Fn(Option<&str>) -> bool) -> Self {
        Self {
            callback,
            logger: None,
            cancellable: true,
        }
    }
    pub fn with_logger(mut self, logger: &'a LogObserver<'a>) -> Self {
        self.logger = Some(logger);
        self
    }
    pub fn uncancellable(&self) -> Self {
        Self {
            cancellable: false,
            ..*self
        }
    }
    pub fn check(&self) -> Result<()> {
        if !self.cancellable || (self.callback)(None) {
            Ok(())
        } else {
            Err(Error::Cancelled)
        }
    }
    pub fn log(&self, value: &str) -> Result<()> {
        self.log_at(LogLevel::Info, value)
    }
    pub fn warn(&self, value: &str) -> Result<()> {
        self.log_at(LogLevel::Warning, value)
    }
    pub fn log_at(&self, level: LogLevel, value: &str) -> Result<()> {
        let proceed = (self.callback)(Some(value));
        if let Some(logger) = self.logger {
            logger(level, value);
        }
        if !self.cancellable || proceed {
            Ok(())
        } else {
            Err(Error::Cancelled)
        }
    }
}

pub fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

#[cfg(test)]
mod control_tests {
    use super::*;

    #[test]
    fn explicit_log_levels_survive_commit_protection_without_changing_cancellation() {
        let received = std::cell::RefCell::new(Vec::new());
        let logger =
            |level, message: &str| received.borrow_mut().push((level, message.to_string()));
        let control = Control::new(&|_| false).with_logger(&logger);
        assert!(matches!(
            control.warn("保留原始数据"),
            Err(Error::Cancelled)
        ));
        let committed = control.uncancellable();
        committed.check().unwrap();
        committed.warn("提交后提示").unwrap();
        committed.log("错误数为 0").unwrap();
        assert_eq!(
            *received.borrow(),
            vec![
                (LogLevel::Warning, "保留原始数据".into()),
                (LogLevel::Warning, "提交后提示".into()),
                (LogLevel::Info, "错误数为 0".into()),
            ]
        );
    }
}
