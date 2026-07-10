/// 自动推断 OpenAPI 信息的工具函数
use crate::utoipa::config::{OpenApiConfig, ParamConfig, ParamLocation, ResponseConfig};
use syn::*;

/// 从函数属性中提取文档注释
pub fn extract_doc_comments(attrs: &[Attribute]) -> (Option<String>, Option<String>) {
    let mut lines = Vec::new();

    for attr in attrs {
        if attr.path().is_ident("doc")
            && let Ok(meta) = attr.meta.require_name_value()
            && let Expr::Lit(ExprLit {
                lit: Lit::Str(lit_str),
                ..
            }) = &meta.value
        {
            let line = lit_str.value().trim().to_string();
            if !line.is_empty() {
                lines.push(line);
            }
        }
    }

    if lines.is_empty() {
        return (None, None);
    }

    // 第一行作为 summary
    let summary = lines.first().cloned();

    // 其余行作为 description
    let description = if lines.len() > 1 {
        Some(lines[1..].join("\n"))
    } else {
        None
    };

    (summary, description)
}

/// 从函数参数推断参数配置
/// 返回 (params, request_body)
/// 支持：
/// 1. #[path], #[query], #[header] 标记
/// 2. Miko 提取器：Path<T>, Query<T>, Json<T>, Form<T>
pub fn infer_params_from_fn_args(
    inputs: &punctuated::Punctuated<FnArg, token::Comma>,
) -> (
    Vec<ParamConfig>,
    Option<crate::utoipa::config::RequestBodyConfig>,
) {
    let mut params = Vec::new();
    let mut request_body = None;

    for arg in inputs {
        if let FnArg::Typed(pat_type) = arg {
            // 依赖和配置参数属于框架内部注入，不应出现在 OpenAPI 中。
            if has_attr(&pat_type.attrs, "dep") || has_attr(&pat_type.attrs, "config") {
                continue;
            }

            // 检查是否是 #[body]
            if has_attr(&pat_type.attrs, "body") {
                // 提取类型
                let ty = (*pat_type.ty).clone();
                let description = extract_desc_from_attrs(&pat_type.attrs);
                let mut content_type = "application/json".to_string();
                use crate::toolkit::attr::StrAttrMap;
                for attr in &pat_type.attrs {
                    if attr.path().is_ident("body")
                        && let Meta::List(list) = &attr.meta
                        && let Ok(sam) = syn::parse2::<StrAttrMap>(list.tokens.clone())
                        && sam.map.contains_key("str")
                    {
                        content_type = "text/plain".to_string();
                    }
                }

                request_body = Some(crate::utoipa::config::RequestBodyConfig {
                    ty,
                    description,
                    required: true,
                    content_type,
                });
                continue;
            }

            // 尝试从类型推断（Miko 提取器）
            let (extractor_info, inner_type) = analyze_extractor_type(&pat_type.ty);

            // 特殊处理：检查是否是 Json<T> 或 Form<T>
            if let Type::Path(type_path) = &*pat_type.ty
                && let Some(last_segment) = type_path.path.segments.last()
            {
                let type_name = last_segment.ident.to_string();
                if matches!(type_name.as_str(), "Json" | "Form") {
                    let description = extract_desc_from_attrs(&pat_type.attrs);
                    let content_type = if type_name.as_str() == "Form" {
                        "application/x-www-form-urlencoded".to_string()
                    } else {
                        "application/json".to_string()
                    };

                    request_body = Some(crate::utoipa::config::RequestBodyConfig {
                        ty: inner_type.unwrap_or_else(|| (*pat_type.ty).clone()),
                        description,
                        required: true,
                        content_type,
                    });
                    continue;
                } else if matches!(type_name.as_str(), "Multipart" | "MultipartResult") {
                    let description = extract_desc_from_attrs(&pat_type.attrs);
                    request_body = Some(crate::utoipa::config::RequestBodyConfig {
                        ty: parse_quote!(::miko::serde_json::Value),
                        description,
                        required: true,
                        content_type: "multipart/form-data".to_string(),
                    });
                    continue;
                }
            }

            // 检查是否有 #[path] 或 #[query] 等标记
            let location = determine_param_location(&pat_type.attrs).or(extractor_info);

            if let Some(loc) = location {
                // 提取参数名
                if let Some(param_name) = extract_extractor_ident(&pat_type.pat) {
                    // 提取 #[desc] 描述
                    let description = extract_desc_from_attrs(&pat_type.attrs);

                    // 获取实际类型(可能包含 extractor 的内部类型)
                    let base_type = inner_type.unwrap_or_else(|| (*pat_type.ty).clone());

                    // 判断类型是否是 Option<T>
                    // 注意:我们保持类型为 Option<T>,不提取内部类型
                    // utoipa 会自动从 Option<T> 推断 required=false
                    let is_optional = is_option_type(&base_type);

                    params.push(ParamConfig {
                        name: param_name,
                        ty: base_type, // 保持原始类型,可能是 Option<T>
                        location: loc,
                        description,
                        required: !is_optional, // Option<T> 类型为可选参数
                        deprecated: false,
                        example: None,
                    });
                }
            }
        }
    }
    // 如果还没有 request_body，检查最后一个非注入参数是否为 String。
    if request_body.is_none()
        && let Some(last_arg) = inputs.iter().rev().find(|arg| {
            let FnArg::Typed(pat_type) = arg else {
                return false;
            };
            !has_attr(&pat_type.attrs, "dep") && !has_attr(&pat_type.attrs, "config")
        })
        && let FnArg::Typed(pat_type) = last_arg
        && pat_type.attrs.is_empty()
    {
        let ty = (*pat_type.ty).clone();
        if is_string_type(&ty) {
            request_body = Some(crate::utoipa::config::RequestBodyConfig {
                ty,
                description: None,
                required: true,
                content_type: "text/plain".to_string(),
            });
        }
    }

    (params, request_body)
}

/// 分析提取器类型，返回 (位置, 内部类型)
/// 支持：Path<T>, Query<T>, Json<T>, Form<T>, State<T>
/// 特殊返回：如果是 Json/Form，返回 (None, Some(T))，调用者应该将其作为 request body
fn analyze_extractor_type(ty: &Type) -> (Option<ParamLocation>, Option<Type>) {
    if let Type::Path(type_path) = ty
        && let Some(last_segment) = type_path.path.segments.last()
    {
        let extractor_name = last_segment.ident.to_string();

        // 先检查是否是已知的提取器类型
        let location = match extractor_name.as_str() {
            "Path" => Some(ParamLocation::Path),
            "Query" => Some(ParamLocation::Query),
            "Json" | "Form" => None, // 返回 None 表示是 request body
            "State" | "Extension" | "Extensions" | "Method" | "Uri" => None, // 忽略这些
            _ => return (None, None), // 不是提取器，直接返回
        };

        // 只有当确认是提取器时，才提取泛型参数
        let inner_type = if let PathArguments::AngleBracketed(args) = &last_segment.arguments {
            args.args.first().and_then(|arg| {
                if let GenericArgument::Type(inner) = arg {
                    Some(inner.clone())
                } else {
                    None
                }
            })
        } else {
            None
        };

        // 对于 Json/Form，我们在外部特殊处理
        if matches!(extractor_name.as_str(), "Json" | "Form") {
            return (None, inner_type);
        }

        return (location, inner_type);
    }

    (None, None)
}

/// 检查属性列表中是否有指定的属性
fn has_attr(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident(name))
}

/// 检查类型是否是 Option<T>
fn is_option_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty
        && let Some(last_segment) = type_path.path.segments.last()
    {
        return last_segment.ident == "Option";
    }
    false
}

/// 判断类型是否是 String
fn is_string_type(ty: &Type) -> bool {
    match ty {
        Type::Path(path) => {
            path.path
                .segments
                .last()
                .map(|s| s.ident == "String")
                .unwrap_or(false)
                || path.path.is_ident("String")
        }
        _ => false,
    }
}

/// 根据参数属性确定参数位置
fn determine_param_location(attrs: &[Attribute]) -> Option<ParamLocation> {
    for attr in attrs {
        if attr.path().is_ident("path") {
            return Some(ParamLocation::Path);
        } else if attr.path().is_ident("query") {
            return Some(ParamLocation::Query);
        } else if attr.path().is_ident("header") {
            return Some(ParamLocation::Header);
        }
    }
    None
}

/// 从属性中提取 #[desc("...")] 描述
fn extract_desc_from_attrs(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("desc")
            && let Ok(lit) = attr.parse_args::<LitStr>()
        {
            return Some(lit.value());
        }
    }
    None
}

/// 从返回类型推断响应配置
/// 注意：由于 miko 使用 IntoResponse trait，实际上无法可靠地推断响应类型
/// 这个函数保留但可能返回 None，建议用户使用 #[u_response] 明确指定
#[allow(unused_variables)]
pub fn infer_response_from_return_type(_output: &ReturnType) -> Option<ResponseConfig> {
    // 由于返回类型是 impl IntoResponse，我们无法推断具体类型
    // 用户应该使用 #[u_response] 明确指定响应
    None
}

/// 从类型中提取响应体类型
/// 支持：Result<Json<T>, E>, Json<T>, Response<T> 等
#[allow(dead_code)]
fn extract_response_body_type(ty: &Type) -> Option<Type> {
    if let Type::Path(type_path) = ty {
        let last_segment = type_path.path.segments.last()?;

        match last_segment.ident.to_string().as_str() {
            "Result" => {
                // Result<Json<User>, Error> -> 提取 Ok 类型
                if let PathArguments::AngleBracketed(args) = &last_segment.arguments
                    && let Some(GenericArgument::Type(ok_type)) = args.args.first()
                {
                    return extract_response_body_type(ok_type);
                }
            }
            "Json" => {
                // Json<User> -> 提取 User
                if let PathArguments::AngleBracketed(args) = &last_segment.arguments
                    && let Some(GenericArgument::Type(inner_type)) = args.args.first()
                {
                    return Some(inner_type.clone());
                }
            }
            "Response" => {
                // Response<Body> -> 提取 Body
                if let PathArguments::AngleBracketed(args) = &last_segment.arguments
                    && let Some(GenericArgument::Type(inner_type)) = args.args.first()
                {
                    return Some(inner_type.clone());
                }
            }
            _ => {}
        }
    }

    None
}

/// 从路径字符串推断路径参数
/// 例如："/users/{id}/posts/{post_id}" -> ["id", "post_id"]
#[allow(dead_code)]
pub fn extract_path_params(path: &str) -> Vec<String> {
    let mut params = Vec::new();
    let mut in_brace = false;
    let mut current_param = String::new();

    for ch in path.chars() {
        match ch {
            '{' => {
                in_brace = true;
                current_param.clear();
            }
            '}' => {
                if in_brace && !current_param.is_empty() {
                    params.push(current_param.clone());
                }
                in_brace = false;
            }
            _ => {
                if in_brace {
                    current_param.push(ch);
                }
            }
        }
    }

    params
}

/// 从函数名推断路径
/// 例如：get_user -> /user, get_users_by_id -> /users/{id}
#[allow(dead_code)]
pub fn infer_path_from_fn_name(fn_name: &str) -> String {
    // 移除方法前缀
    let name = fn_name
        .trim_start_matches("get_")
        .trim_start_matches("post_")
        .trim_start_matches("put_")
        .trim_start_matches("delete_")
        .trim_start_matches("patch_");

    // 将下划线转换为斜杠
    let path = name.replace('_', "/");

    format!("/{}", path)
}

/// 完整的自动推断流程
pub fn infer_openapi_config(
    fn_attrs: &[Attribute],
    fn_inputs: &punctuated::Punctuated<FnArg, token::Comma>,
    fn_output: &ReturnType,
) -> OpenApiConfig {
    let mut config = OpenApiConfig::new();

    // 提取文档注释
    let (summary, description) = extract_doc_comments(fn_attrs);
    config.auto_summary = summary;
    config.auto_description = description;

    // 推断参数和请求体
    let (params, request_body) = infer_params_from_fn_args(fn_inputs);
    config.auto_params = params;
    config.auto_request_body = request_body;

    // 推断响应（当前返回 None）
    config.auto_response = infer_response_from_return_type(fn_output);

    config
}

/// 提取提取器ident
pub fn extract_extractor_ident(pat: &Pat) -> Option<String> {
    match pat {
        Pat::Ident(p) => Some(p.ident.to_string()),
        Pat::TupleStruct(p) if p.elems.len() == 1 => {
            if let Pat::Ident(inner_ident) = &p.elems[0] {
                Some(inner_ident.ident.to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}
