pub mod app;
#[cfg(feature = "ext")]
pub mod ext;
pub mod handler;

#[cfg(feature = "macro")]
pub use miko_macros as macros;

#[cfg(feature = "auto")]
pub mod auto;
pub mod dependency_container;
pub mod endpoint;
pub mod error;
pub mod extractor;
pub mod http;
pub mod router;
#[cfg(feature = "test")]
pub mod test;
pub mod ws;

pub mod middleware;

pub use http_body_util;
pub use hyper;
#[cfg(feature = "auto")]
pub use inventory;
pub use miko_core;
pub use serde;
pub use serde_json;
// repub
pub use tokio;
pub use tower;
#[cfg(any(feature = "ext", feature = "catch_panic"))]
pub use tower_http;
pub use tracing;

#[cfg(feature = "auto")]
pub use dependency_container::{Dep, OwnedDep};

#[cfg(all(feature = "utoipa", feature = "auto"))]
pub mod openapi;

#[cfg(feature = "utoipa")]
pub use utoipa::{self, IntoParams, OpenApi, ToResponse, ToSchema};

#[cfg(feature = "validation")]
pub use garde::{self, Validate};

// 导出常用的响应类型
pub use http::response::into_response::IntoResponse;

// 导出错误处理类型
pub use error::{AppError, AppResult, ErrorResponse, ValidationErrorDetail};
