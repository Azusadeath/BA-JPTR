#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

impl Level {
    fn detect(message: &str) -> Self {
        let lower = message.to_lowercase();
        if ["错误", "失败", "异常", "损坏", "error", "panic"]
            .iter()
            .any(|word| lower.contains(word))
        {
            Self::Error
        } else if [
            "警告",
            "跳过",
            "不支持",
            "未保存",
            "取消",
            "warning",
            "warn",
        ]
        .iter()
        .any(|word| lower.contains(word))
        {
            Self::Warning
        } else {
            Self::Info
        }
    }
}

pub struct Entry {
    pub level: Level,
    message: String,
}

impl Entry {
    pub fn new(message: String, level: Option<Level>) -> Self {
        Self {
            level: level.unwrap_or_else(|| Level::detect(&message)),
            message,
        }
    }

    pub fn formatted(&self) -> String {
        let level = match self.level {
            Level::Info => "INFO ",
            Level::Warning => "WARN ",
            Level::Error => "ERROR",
        };
        self.message
            .lines()
            .map(|line| format!("[{level}] {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
