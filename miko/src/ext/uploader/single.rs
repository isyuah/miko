use crate::AppError;
use crate::ext::uploader::{FileField, UploadedFile};
use crate::extractor::from_request::FromRequest;
use crate::extractor::multipart::Multipart;
use crate::handler::Req;
use crate::http::response::into_response::IntoResponse;
use hyper::StatusCode;
use miko_core::Resp;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::Service;

/// 单文件上传服务（自动选择第一个文件字段进行处理）
#[derive(Clone)]
pub struct SingleUploader<H> {
    pub(crate) inner: Arc<H>,
}

/// 上传处理器：将一个上传字段处理为最终的 UploadedFile
pub trait UploaderProcesser {
    fn process(
        &self,
        file_field: FileField,
    ) -> impl Future<Output = Result<UploadedFile, anyhow::Error>> + Send + Sync + 'static;
}

impl<H> Service<Req> for SingleUploader<H>
where
    H: UploaderProcesser + Clone + Send + Sync + 'static,
{
    type Response = Resp;
    type Error = AppError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, req: Req) -> Self::Future {
        let inner = self.inner.clone();
        Box::pin(async move {
            let Multipart(mut multipart) = match Multipart::from_request(req, Arc::new(())).await {
                Ok(multipart) => multipart,
                Err(error) => return Ok(error.into_response()),
            };
            let file_field = loop {
                let field = match multipart.next_field().await {
                    Ok(Some(field)) => field,
                    Ok(None) => {
                        return Ok(
                            AppError::BadRequest("No file field found".to_string()).into_response()
                        );
                    }
                    Err(error) => return Ok(AppError::from(error).into_response()),
                };

                let Some(original_filename) = field.file_name().map(str::to_owned) else {
                    continue;
                };

                break FileField {
                    original_filename,
                    content_type: field.content_type().cloned(),
                    field,
                };
            };

            match inner.process(file_field).await {
                Ok(file) => Ok(format!("uploaded file {}", file.original_filename).into_response()),
                Err(e) => Ok((StatusCode::BAD_REQUEST, e.into_response()).into_response()),
            }
        })
    }
}
