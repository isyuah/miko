//! ValidatedJson 提取器
//!
//! 自动解析 JSON 并验证，验证失败时自动转换为 AppError::ValidationError
//!
//! 需要启用 `validation` feature

#[cfg(feature = "validation")]
use crate::error::AppError;
#[cfg(feature = "validation")]
use crate::extractor::from_request::{FRFut, FromRequest};
#[cfg(feature = "validation")]
use crate::handler::Req;
#[cfg(feature = "validation")]
use http_body_util::BodyExt;
#[cfg(feature = "validation")]
use serde::de::DeserializeOwned;
#[cfg(feature = "validation")]
use std::sync::Arc;

/// ValidatedJson 提取器
///
/// 自动反序列化 JSON 并执行验证
///
/// # 要求
/// - `T` 必须实现 `serde::Deserialize`
/// - `T` 必须实现 `garde::Validate`
///
/// # Example
/// ```rust,ignore
/// use miko::extractor::ValidatedJson;
/// use garde::Validate;
/// use serde::Deserialize;
///
/// #[derive(Debug, Deserialize, Validate)]
/// struct CreateUser {
///     #[garde(length(min = 3, max = 50))]
///     username: String,
///
///     #[garde(email)]
///     email: String,
///
///     #[garde(length(min = 8))]
///     password: String,
/// }
///
/// async fn create_user(
///     ValidatedJson(user): ValidatedJson<CreateUser>
/// ) -> AppResult<String> {
///     // user 已经通过验证
///     Ok(format!("Created user: {}", user.username))
/// }
/// ```
#[cfg(feature = "validation")]
#[derive(Debug)]
pub struct ValidatedJson<T>(pub T);

#[cfg(feature = "validation")]
impl<S, T> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + garde::Validate + Send + Sync + 'static,
    <T as garde::Validate>::Context: Default,
    S: Send + Sync + 'static,
{
    fn from_request(mut req: Req, _state: Arc<S>) -> FRFut<Self> {
        Box::pin(async move {
            let body = req
                .body_mut()
                .collect()
                .await
                .map_err(|e| AppError::BadRequest(format!("Failed to read request body: {}", e)))?
                .to_bytes();

            let value: T = serde_json::from_slice(&body).map_err(AppError::JsonParseError)?;

            value.validate().map_err(AppError::from)?;

            Ok(ValidatedJson(value))
        })
    }
}

#[cfg(feature = "validation")]
impl<T> std::ops::Deref for ValidatedJson<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(feature = "validation")]
impl<T> std::ops::DerefMut for ValidatedJson<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
