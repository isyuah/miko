use crate::AppError;
use crate::extractor::from_request::{FRFut, FromRequest};
use crate::handler::Req;
use bytes::Bytes;
use futures::TryStreamExt;
use http_body_util::BodyExt;
use hyper::HeaderMap;
use mime_guess::Mime;
use std::collections::HashMap;
use std::fs::Metadata;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio_util::io::StreamReader;

/// 原始 Multipart 访问器（底层包装 multer::Multipart），用于自定义解析流程
pub struct Multipart(pub multer::Multipart<'static>);

/// Multipart 解析结果，包含字段和值、文件集合
#[derive(Debug)]
pub struct MultipartResult {
    pub fields: HashMap<String, Vec<String>>,
    pub files: HashMap<String, Vec<MultipartFile>>,
}

/// 表示一个上传的文件（已落盘到临时文件）
#[derive(Debug)]
pub struct MultipartFile {
    pub filename: String,
    pub size: usize,
    pub content_type: Option<Mime>,
    pub linker: MultipartFileDiskLinker,
}

/// 已保存到磁盘的临时文件链接器，提供便捷操作
#[derive(Debug)]
pub struct MultipartFileDiskLinker {
    pub file: File,
    pub file_path: PathBuf,
    #[allow(dead_code)]
    temp_file: Arc<NamedTempFile>,
}

impl MultipartFileDiskLinker {
    /// 将临时文件复制到指定路径
    pub async fn transfer_to(&self, path: impl Into<PathBuf>) -> Result<u64, std::io::Error> {
        tokio::fs::copy(self.file_path.clone(), path.into()).await
    }
    /// 以 UTF-8 读取整个文件内容为字符串
    pub async fn read_to_string(&mut self) -> Result<String, std::io::Error> {
        let mut buf = String::new();
        self.file.read_to_string(&mut buf).await?;
        Ok(buf)
    }
    /// 读取所有字节并关闭文件句柄
    pub async fn read_and_drop_file(mut self) -> Result<Bytes, std::io::Error> {
        let mut buf = Vec::new();
        self.file.read_to_end(&mut buf).await?;
        Ok(Bytes::from(buf))
    }
    /// 读取文件元数据
    pub async fn metadata(&self) -> std::io::Result<Metadata> {
        self.file.metadata().await
    }
}

impl<S> FromRequest<S> for MultipartResult {
    fn from_request(req: Req, _state: Arc<S>) -> FRFut<Self> {
        Box::pin(async move {
            let mut form = HashMap::new();
            let mut files = HashMap::new();
            let boundary = parse_boundary(req.headers())?;
            let body = req.into_body().into_data_stream();
            let mut multipart = multer::Multipart::new(body, boundary);
            while let Some(field) = multipart.next_field().await? {
                let name = field.name().map(str::to_owned).ok_or_else(|| {
                    AppError::MultipartParseError(
                        "Multipart field is missing the required name".to_string(),
                    )
                })?;
                if let Some(filename) = field.file_name() {
                    let filename = filename.to_string();
                    let content_type = field.content_type().cloned();
                    let temp_file = NamedTempFile::new()?;
                    let file_path = temp_file.path().to_path_buf();
                    let mut async_file_writer = File::options()
                        .read(true)
                        .write(true)
                        .open(file_path.clone())
                        .await?;
                    let mut reader =
                        StreamReader::new(field.into_stream().map_err(std::io::Error::other));
                    tokio::io::copy(&mut reader, &mut async_file_writer).await?;
                    let fil = MultipartFile {
                        filename,
                        size: async_file_writer.metadata().await?.len() as usize,
                        content_type,
                        linker: MultipartFileDiskLinker {
                            file: async_file_writer,
                            file_path,
                            temp_file: Arc::new(temp_file),
                        },
                    };

                    files.entry(name).or_insert(vec![]).push(fil);
                } else {
                    let value = field.text().await?;
                    form.entry(name).or_insert(vec![]).push(value);
                }
            }
            Ok(MultipartResult {
                fields: form,
                files,
            })
        })
    }
}

impl<S> FromRequest<S> for Multipart {
    fn from_request(req: Req, _state: Arc<S>) -> FRFut<Self> {
        Box::pin(async move {
            let boundary = parse_boundary(req.headers())?;
            let body = req.into_body().into_data_stream();
            let multipart = multer::Multipart::new(body, boundary);
            Ok(Multipart(multipart))
        })
    }
}

fn parse_boundary(headers: &HeaderMap) -> Result<String, AppError> {
    let content_type = headers
        .get("Content-Type")
        .ok_or_else(|| AppError::MultipartParseError("Missing Content-Type header".to_string()))?
        .to_str()
        .map_err(|err| AppError::MultipartParseError(err.to_string()))?;

    multer::parse_boundary(content_type).map_err(AppError::from)
}
