pub use bajptr_core::LogLevel as Level;

pub struct Entry {
    pub level: Level,
    message: String,
}

impl Entry {
    pub fn new(message: String, level: Option<Level>) -> Self {
        Self {
            level: level.unwrap_or(Level::Info),
            message,
        }
    }

    pub fn formatted(&self) -> String {
        let level = match self.level {
            Level::Info => return self.message.clone(),
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
