use std::fmt;

/// Core 统一错误类型。CLI / FFI 只负责映射展示，不得自行解释业务错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    NotFound,
    ValidationError(String),
    HasActiveDependents,
    InvalidDirection,
    MixedCurrencySettlement,
    CategoryInUse,
    AmountOverflow,
    DbError(String),
}

impl CoreError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self::ValidationError(message.into())
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::NotFound => write!(f, "记录不存在"),
            CoreError::ValidationError(message) => write!(f, "校验失败: {message}"),
            CoreError::HasActiveDependents => {
                write!(f, "存在未作废的关联交易，不能作废当前交易")
            }
            CoreError::InvalidDirection => write!(f, "交易方向与交易类型不匹配"),
            CoreError::MixedCurrencySettlement => {
                write!(f, "同一笔交易的结算账户币种必须一致")
            }
            CoreError::CategoryInUse => write!(f, "分类已被引用或仍有子分类，不能执行该操作"),
            CoreError::AmountOverflow => write!(f, "金额计算溢出 i64"),
            CoreError::DbError(message) => write!(f, "数据库错误: {message}"),
        }
    }
}

impl std::error::Error for CoreError {}

impl From<rusqlite::Error> for CoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::DbError(value.to_string())
    }
}

impl From<rusqlite_migration::Error> for CoreError {
    fn from(value: rusqlite_migration::Error) -> Self {
        Self::DbError(value.to_string())
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
