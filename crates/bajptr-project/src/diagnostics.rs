use crate::Error;
use std::io::ErrorKind;

pub fn user_message(action: &str, error: &Error) -> String {
    let operation = match action {
        "load" => "读取工作区",
        "check" => "检查游戏与底稿",
        "search" | "search_more" => "查找文本",
        "export_text" | "export_rules" => "导出文件",
        "apply" => "应用到游戏",
        "restore" => "恢复初始底稿",
        "preview" | "prepare_patch" => "准备替换",
        "sources" => "同步来源表",
        "import_read" | "import_commit" => "导入词表",
        "save_rule" | "text_save" | "batch_save" | "combined_save" | "settings" => "保存记录",
        "combined_remove" => "移除替换",
        "combined_open" => "读取替换范围",
        "promotion_preview" => "核对批量修改",
        "promotion_commit" | "promotion_start" => "整理批量修改",
        "backup_create" => "保存备份",
        "backup_read" => "读取备份",
        "backup_restore" => "恢复备份",
        "open_directory" => "打开目录",
        "export_logs" => "导出日志",
        _ => "处理请求",
    };
    match error {
        Error::Context { message, .. } | Error::Invalid(message) => message.clone(),
        Error::Io(error) => format!("{operation}失败：{}", io_message(error)),
        Error::Json(_) => {
            format!("{operation}失败：记录文件无法解析。请检查文件格式，详细原因见日志。")
        }
        Error::Sql(_) => {
            format!("{operation}失败：数据库无法读取或处理。请检查底稿与输出位置，详细原因见日志。")
        }
        Error::Parse(_) => {
            format!("{operation}失败：资源内容无法解析。请检查游戏资源与底稿，详细原因见日志。")
        }
        Error::Regex(_) => "正则表达式或替换内容无法处理，请检查词条；详细原因见日志。".into(),
        Error::UnsupportedTable(_) => "此来源表的结构暂不支持，请查看日志中的来源信息。".into(),
        Error::Rule {
            index,
            original,
            source,
        } => format!(
            "第 {index} 条词条（{original}）：{}",
            user_message(action, source)
        ),
        Error::Field { slot, source } => format!("字段 {slot}：{}", user_message(action, source)),
        Error::Record {
            table,
            record,
            source,
        } => format!(
            "来源表 {table}，记录 {record}：{}",
            user_message(action, source)
        ),
        Error::Cancelled => "操作已取消。".into(),
    }
}

fn io_message(error: &std::io::Error) -> &'static str {
    match error.raw_os_error() {
        #[cfg(windows)]
        Some(32 | 33) => return "文件正被其他程序占用。请关闭占用它的程序后重试。",
        #[cfg(windows)]
        Some(112) => return "磁盘空间不足。请释放空间或选择其他位置。",
        _ => {}
    }
    match error.kind() {
        ErrorKind::PermissionDenied => "没有文件访问权限。请检查目录权限和文件的只读属性。",
        ErrorKind::NotFound => "所需文件或目录不存在。请核对路径，或重新检查游戏资源。",
        ErrorKind::AlreadyExists => "目标位置已有同名内容。请检查该位置或选择其他目录。",
        ErrorKind::NotADirectory
        | ErrorKind::IsADirectory
        | ErrorKind::InvalidInput
        | ErrorKind::InvalidFilename => "文件或目录路径无效。请检查路径层级和名称。",
        ErrorKind::StorageFull => "磁盘空间不足。请释放空间或选择其他位置。",
        ErrorKind::ReadOnlyFilesystem => "目标位置只读。请选择可写目录或解除只读限制。",
        ErrorKind::WouldBlock => "文件当前无法访问，请稍后重试。",
        _ => "文件操作未完成。请检查路径、权限和磁盘状态，详细原因见日志。",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_and_parser_details_are_kept_out_of_user_messages() {
        let io = Error::Io(std::io::Error::new(
            ErrorKind::PermissionDenied,
            "raw system detail",
        ));
        let message = user_message("save_rule", &io);
        assert!(message.contains("权限"));
        assert!(!message.contains("raw system detail"));
        assert!(io.to_string().contains("raw system detail"));
        let json = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        assert!(user_message("load", &Error::Json(json)).contains("无法解析"));
    }
}
