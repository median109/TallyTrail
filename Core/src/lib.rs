//! TallyTrail 纯业务核心。
//!
//! - [`api::CoreHandle`]：唯一对外入口，UI 层只允许调用该接口。
//! - [`domain`]：实体、值对象、校验规则与公共 DTO。
//! - [`data`]：SQLite 连接、迁移与 repository 实现（不对外暴露 SQL 能力）。
//!
//! 金额一律为 `i64` 分；时间一律为 UTC 秒时间戳。Core 不依赖任何 UI 库。

pub mod api;
pub mod data;
pub mod domain;

pub use api::CoreHandle;
pub use domain::*;
