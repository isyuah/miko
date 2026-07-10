//! 测试 utoipa 集成功能
//!
//! 这些测试验证自动推断和配置合并是否正常工作

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use crate::utoipa::{
        config::{OpenApiConfig, ParamLocation, ResponseConfig},
        infer::{extract_doc_comments, infer_params_from_fn_args},
    };
    use syn::punctuated::Punctuated;
    use syn::token::Comma;
    use syn::{Attribute, FnArg, parse_quote};

    #[test]
    fn test_extract_doc_comments() {
        let attrs: Vec<Attribute> = vec![
            parse_quote!(#[doc = " 获取用户信息"]),
            parse_quote!(#[doc = ""]),
            parse_quote!(#[doc = " 根据用户 ID 返回详细信息"]),
        ];

        let (summary, description) = extract_doc_comments(&attrs);

        assert_eq!(summary, Some("获取用户信息".to_string()));
        assert_eq!(description, Some("根据用户 ID 返回详细信息".to_string()));
    }

    #[test]
    fn test_extract_single_line_doc() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[doc = " 简短描述"])];

        let (summary, description) = extract_doc_comments(&attrs);

        assert_eq!(summary, Some("简短描述".to_string()));
        assert_eq!(description, None);
    }

    #[test]
    fn test_infer_path_param() {
        let inputs: Punctuated<FnArg, Comma> = parse_quote! {
            #[path] id: i32
        };

        let (params, _) = infer_params_from_fn_args(&inputs);

        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "id");
        assert_eq!(params[0].location, ParamLocation::Path);
    }

    #[test]
    fn test_infer_query_param() {
        let inputs: Punctuated<FnArg, Comma> = parse_quote! {
            #[query] filter: UserFilter
        };

        let (params, _) = infer_params_from_fn_args(&inputs);

        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "filter");
        assert_eq!(params[0].location, ParamLocation::Query);
    }

    // 注意：由于 Miko 使用 impl IntoResponse，无法推断响应类型
    // 因此移除了 test_infer_json_response 和 test_infer_result_json_response

    #[test]
    fn test_config_merge() {
        let mut config = OpenApiConfig::new();

        // 用户配置
        config.user_summary = Some("用户摘要".to_string());
        config.user_responses.push(ResponseConfig {
            status: 404,
            description: "Not Found".to_string(),
            body: None,
            content_type: None,
        });

        // 自动推断（但响应不推断）
        config.auto_summary = Some("自动摘要".to_string());

        // 验证合并结果
        assert_eq!(config.final_summary(), Some("用户摘要"));

        let responses = config.final_responses();
        assert_eq!(responses.len(), 1); // 只有用户定义的 404
        assert_eq!(responses[0].status, 404);
    }

    #[test]
    fn test_body_attr_str_and_default_string_body() {
        // explicit #[body(str)]
        let inputs: Punctuated<FnArg, Comma> = parse_quote! {
            #[body(str)] s: String
        };
        let (_, rb1) = infer_params_from_fn_args(&inputs);
        assert!(rb1.is_some());
        assert_eq!(rb1.unwrap().content_type, "text/plain");

        // default last param String with no attrs -> text/plain
        let inputs2: Punctuated<FnArg, Comma> = parse_quote! {
            s: String
        };
        let (_, rb2) = infer_params_from_fn_args(&inputs2);
        assert!(rb2.is_some());
        assert_eq!(rb2.unwrap().content_type, "text/plain");
    }

    #[test]
    fn test_body_attr_json_and_form_extractors() {
        // explicit #[body] with non-string type -> application/json
        let inputs: Punctuated<FnArg, Comma> = parse_quote! {
            #[body] u: User
        };
        let (_, rb1) = infer_params_from_fn_args(&inputs);
        assert!(rb1.is_some());
        assert_eq!(rb1.unwrap().content_type, "application/json");

        // Json<T> extractor
        let inputs2: Punctuated<FnArg, Comma> = parse_quote! {
            data: Json<User>
        };
        let (_, rb2) = infer_params_from_fn_args(&inputs2);
        assert!(rb2.is_some());
        assert_eq!(rb2.unwrap().content_type, "application/json");

        // Form<T> extractor
        let inputs3: Punctuated<FnArg, Comma> = parse_quote! {
            form: Form<User>
        };
        let (_, rb3) = infer_params_from_fn_args(&inputs3);
        assert!(rb3.is_some());
        assert_eq!(
            rb3.unwrap().content_type,
            "application/x-www-form-urlencoded"
        );
    }

    #[test]
    fn test_dependency_is_ignored_without_hiding_body_inference() {
        let inputs: Punctuated<FnArg, Comma> = parse_quote! {
            body: String,
            #[dep] service: Arc<Service>
        };

        let (params, request_body) = infer_params_from_fn_args(&inputs);

        assert!(params.is_empty());
        assert_eq!(
            request_body
                .expect("String body should still be inferred")
                .content_type,
            "text/plain"
        );
    }
}
