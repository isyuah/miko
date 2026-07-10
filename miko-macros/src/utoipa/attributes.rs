/// 用户配置属性宏的解析
/// 支持：#[u_response], #[u_tag], #[u_summary] 等
use crate::utoipa::config::ResponseConfig;
use syn::parse::{Parse, ParseStream};
use syn::*;

/// 解析 #[u_response(status = 404, description = "Not found", body = ErrorResponse)]
#[derive(Debug, Clone)]
pub struct UResponseAttr {
    pub status: u16,
    pub description: String,
    pub body: Option<Type>,
    pub content_type: Option<String>,
}

impl Parse for UResponseAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut status = 200;
        let mut description = String::new();
        let mut body = None;
        let mut content_type = None;

        // 直接解析键值对，不需要额外的括号
        // 因为 parse_args 已经处理了外层括号
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "status" => {
                    let lit: LitInt = input.parse()?;
                    status = lit.base10_parse()?;
                }
                "description" => {
                    let lit: LitStr = input.parse()?;
                    description = lit.value();
                }
                "body" => {
                    body = Some(input.parse()?);
                }
                "content_type" => {
                    let lit: LitStr = input.parse()?;
                    content_type = Some(lit.value());
                }
                _ => {
                    return Err(Error::new(key.span(), format!("Unknown key: {}", key)));
                }
            }

            // 可选的逗号
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(UResponseAttr {
            status,
            description,
            body,
            content_type,
        })
    }
}

impl From<UResponseAttr> for ResponseConfig {
    fn from(attr: UResponseAttr) -> Self {
        ResponseConfig {
            status: attr.status,
            description: attr.description,
            body: attr.body,
            content_type: attr.content_type,
        }
    }
}

/// 解析 #[u_tag("用户管理")]
#[derive(Debug, Clone)]
pub struct UTagAttr {
    pub tag: String,
}

impl Parse for UTagAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let lit: LitStr = input.parse()?;
        Ok(UTagAttr { tag: lit.value() })
    }
}

/// 解析 #[u_summary("获取用户信息")]
#[derive(Debug, Clone)]
pub struct USummaryAttr {
    pub summary: String,
}

impl Parse for USummaryAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let lit: LitStr = input.parse()?;
        Ok(USummaryAttr {
            summary: lit.value(),
        })
    }
}

/// 解析 #[u_description("详细描述")]
#[derive(Debug, Clone)]
pub struct UDescriptionAttr {
    pub description: String,
}

impl Parse for UDescriptionAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let lit: LitStr = input.parse()?;
        Ok(UDescriptionAttr {
            description: lit.value(),
        })
    }
}

/// 解析 #[u_request_body(content = Multipart, content_type = "multipart/form-data", description = "文件上传")]
#[derive(Debug, Clone)]
pub struct URequestBodyAttr {
    pub content: Type,
    pub description: Option<String>,
    pub content_type: Option<String>,
}

impl Parse for URequestBodyAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut content = None;
        let mut description = None;
        let mut content_type = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "content" => {
                    content = Some(input.parse()?);
                }
                "description" => {
                    let lit: LitStr = input.parse()?;
                    description = Some(lit.value());
                }
                "content_type" => {
                    let lit: LitStr = input.parse()?;
                    content_type = Some(lit.value());
                }
                _ => {
                    return Err(Error::new(key.span(), format!("Unknown key: {}", key)));
                }
            }

            // 可选的逗号
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        let content =
            content.ok_or_else(|| Error::new(input.span(), "Missing required field: content"))?;

        Ok(URequestBodyAttr {
            content,
            description,
            content_type,
        })
    }
}

/// 解析 #[u_param(name = "id", description = "用户ID", example = 123)]
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UParamAttr {
    pub name: String,
    pub description: Option<String>,
    pub example: Option<Expr>,
    pub deprecated: bool,
}

impl Parse for UParamAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut name = String::new();
        let mut description = None;
        let mut example = None;
        let mut deprecated = false;

        // 直接解析，不需要额外的括号
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "name" => {
                    let lit: LitStr = input.parse()?;
                    name = lit.value();
                }
                "description" => {
                    let lit: LitStr = input.parse()?;
                    description = Some(lit.value());
                }
                "example" => {
                    example = Some(input.parse()?);
                }
                "deprecated" => {
                    let lit: LitBool = input.parse()?;
                    deprecated = lit.value;
                }
                _ => {
                    return Err(Error::new(key.span(), format!("Unknown key: {}", key)));
                }
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(UParamAttr {
            name,
            description,
            example,
            deprecated,
        })
    }
}

/// 从函数属性中提取所有 utoipa 相关配置
pub fn parse_utoipa_attrs(attrs: &[Attribute]) -> crate::utoipa::config::OpenApiConfig {
    let mut config = crate::utoipa::config::OpenApiConfig::new();

    for attr in attrs {
        let path = attr.path();

        if path.is_ident("u_response") {
            if let Ok(resp) = attr.parse_args::<UResponseAttr>() {
                config.user_responses.push(resp.into());
            }
        } else if path.is_ident("u_tag") {
            if let Ok(tag) = attr.parse_args::<UTagAttr>() {
                config.user_tags.push(tag.tag);
            }
        } else if path.is_ident("u_summary") {
            if let Ok(summary) = attr.parse_args::<USummaryAttr>() {
                config.user_summary = Some(summary.summary);
            }
        } else if path.is_ident("u_description") {
            if let Ok(desc) = attr.parse_args::<UDescriptionAttr>() {
                config.user_description = Some(desc.description);
            }
        } else if path.is_ident("u_deprecated") {
            config.deprecated = true;
        } else if path.is_ident("u_request_body")
            && let Ok(req_body) = attr.parse_args::<URequestBodyAttr>()
        {
            config.user_request_body = Some(crate::utoipa::config::RequestBodyConfig {
                ty: req_body.content,
                description: req_body.description,
                required: true,
                content_type: req_body
                    .content_type
                    .unwrap_or_else(|| "application/json".to_string()),
            });
        }
    }

    config
}
