use std::path::PathBuf;

use thiserror::Error;

/// Unified application error type (tech §13).
#[derive(Debug, Error)]
pub enum AppError {
    #[error("config error: {0}")]
    Config(String),

    #[error("asset error: {0}")]
    Asset(String),

    #[error("platform error: {0}")]
    Platform(String),

    #[error("render error: {0}")]
    Render(String),

    #[error("shortcut error: {0}")]
    Shortcut(String),

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// Short, user-facing message (no stack traces).
    pub fn user_message(&self) -> String {
        match self {
            AppError::Config(_) => "配置读写失败，请检查应用数据目录。".into(),
            AppError::Asset(_) => "资源加载失败，将尝试使用占位显示。".into(),
            AppError::Platform(_) => "系统窗口能力调用失败。".into(),
            AppError::Render(_) => "图形初始化或绘制失败。".into(),
            AppError::Shortcut(_) => "快捷方式操作失败。".into(),
            AppError::Io { .. } => "文件读写失败。".into(),
            AppError::Other(msg) => msg.clone(),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(source: std::io::Error) -> Self {
        AppError::Io {
            path: PathBuf::from("<unknown>"),
            source,
        }
    }
}
