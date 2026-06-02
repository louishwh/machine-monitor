//! Unified error type for Tauri commands.
//! Serialises to a plain string so the frontend receives a readable message.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("密钥存储错误：{0}")]
    Keyring(String),

    #[error("密钥错误：{0}")]
    Key(String),

    #[error("网络请求失败：{0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON 解析错误：{0}")]
    Json(#[from] serde_json::Error),

    #[error("服务端错误 {status}：{body}")]
    ServerError { status: u16, body: String },

    #[error("主密钥不存在，请先生成主密钥")]
    NoMasterKey,

    #[error("服务端地址未配置")]
    NoServerUrl,

    #[error("{0}")]
    Other(String),
}

impl From<keyring::Error> for AppError {
    fn from(e: keyring::Error) -> Self {
        AppError::Keyring(e.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
