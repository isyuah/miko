use crate::extractor::body::deal_with_body_attr;
use crate::extractor::path::deal_with_path_attr;
use crate::route::layer::extract_layer_attrs;
use crate::route::{RouteAttr, build_register_expr};
use crate::toolkit::exactors::build_struct_from_query;
use crate::toolkit::rout_arg::{
    FnArgResult, IntoFnArgs, RouteFnArg, build_config_value_injector, build_dep_extractors,
};
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
#[cfg(all(feature = "utoipa", feature = "auto"))]
use quote::format_ident;
use quote::quote;
use syn::{ItemFn, Stmt, parse_quote};

#[cfg(feature = "utoipa")]
use crate::utoipa::{
    attributes::parse_utoipa_attrs,
    generator::{HttpMethod, generate_utoipa_path_attr},
    infer::infer_openapi_config,
};

/// 处理 `#[route(...)]` 系列宏的核心处理器。
///
/// 主要职责：
/// - 修改函数签名（例如自动设置返回类型）；
/// - 解析并重写参数以注入 path/body/query/dep/config 等提取器或获取语句；
/// - 生成临时的 Query 结构体（如需）；
/// - 提取并处理 layer 属性；
/// - 将用户函数体和自动生成的注入语句合并为最终的宏展开。
pub fn route_handler(args: RouteAttr, mut fn_item: ItemFn) -> TokenStream {
    let fn_name = fn_item.sig.ident.clone();
    let layer_attrs = extract_layer_attrs(&fn_item.attrs);
    fn_item.attrs.retain(|attr| !attr.path().is_ident("layer"));

    // utoipa: 在处理前保存原始签名和属性用于推断
    #[cfg(feature = "utoipa")]
    let original_attrs = fn_item.attrs.clone();
    #[cfg(feature = "utoipa")]
    let original_inputs = fn_item.sig.inputs.clone();
    #[cfg(feature = "utoipa")]
    let original_output = fn_item.sig.output.clone();

    // 自动返回值
    let sig = &mut fn_item.sig;
    if matches!(sig.output, syn::ReturnType::Default) {
        sig.output = parse_quote!(-> impl ::miko::http::response::into_response::IntoResponse)
    }
    let inject_segs: Vec<Stmt> = Vec::new();
    let rfa = RouteFnArg::from_punctuated(&mut sig.inputs);
    //处理路由
    let path_inputs = rfa.gen_fn_args(deal_with_path_attr);
    //处理body
    let body_inputs = rfa.gen_fn_args(deal_with_body_attr);
    let plain_inputs = rfa.gen_fn_args(|rfa| {
        if rfa.mark.is_empty() {
            FnArgResult::Keep
        } else {
            FnArgResult::Remove
        }
    });
    // 处理 dep，将其转换为内部 Dep<T> 提取器
    let (dep_inputs, dep_stmts) = build_dep_extractors(&rfa);
    // 处理config_value
    let mut config_value_stmts = Vec::new();
    build_config_value_injector(&rfa, &mut config_value_stmts);
    // 清空参数
    sig.inputs.clear();
    // 获取无修饰参数
    // 组装path
    sig.inputs.extend(path_inputs);
    // 构建 Query 结构体和解构提取器
    let q_struct_ident = Ident::new(&format!("__{}_QueryStruct", fn_name), Span::call_site());
    // 重组Query
    let (q_struct, q_struct_exactor) = build_struct_from_query(&rfa, q_struct_ident);
    if q_struct.is_some() {
        sig.inputs.push(q_struct_exactor.unwrap());
    }
    // 依赖只读取请求扩展，必须放在可能消费 body 的普通提取器之前
    sig.inputs.extend(dep_inputs);
    // 组装plain_inputs
    sig.inputs.extend(plain_inputs);
    // 最后组装body
    sig.inputs.extend(body_inputs);
    // 展开
    let user_stmts = &fn_item.block.stmts.clone();
    let inventory_collect: Option<proc_macro2::TokenStream> = if cfg!(feature = "auto") {
        Some(build_register_expr(&args, &fn_name.clone(), &layer_attrs))
    } else {
        None
    };

    // utoipa: 生成 OpenAPI 文档
    #[cfg(feature = "utoipa")]
    let utoipa_attr =
        generate_utoipa_attr(&args, &original_attrs, &original_inputs, &original_output);
    #[cfg(all(feature = "utoipa", feature = "auto"))]
    let openapi_collect = {
        let openapi_ident = format_ident!("__miko_openapi_{}", fn_name);
        quote! {
            #[allow(non_camel_case_types)]
            #[derive(::miko::OpenApi)]
            #[openapi(paths(#fn_name))]
            struct #openapi_ident;

            ::miko::inventory::submit! {
                ::miko::openapi::OpenApiRoute {
                    openapi: <#openapi_ident as ::miko::utoipa::OpenApi>::openapi,
                    module_path: module_path!(),
                }
            }
        }
    };
    #[cfg(all(feature = "utoipa", not(feature = "auto")))]
    let openapi_collect = proc_macro2::TokenStream::new();

    #[cfg(feature = "utoipa")]
    {
        quote! {
          #q_struct

          #utoipa_attr
          #sig {
            #(#inject_segs)*
            #(#dep_stmts)*
            #(#config_value_stmts)*
            #(#user_stmts)*
          }

          #inventory_collect
          #openapi_collect

        }
        .into()
    }

    #[cfg(not(feature = "utoipa"))]
    {
        quote! {
          #q_struct

          #sig {
            #(#inject_segs)*
            #(#dep_stmts)*
            #(#config_value_stmts)*
            #(#user_stmts)*
          }

          #inventory_collect

        }
        .into()
    }
}

/// route_handler 的变体: 只生成 OpenAPI,不注册路由
/// 用于 #[miko::path] 宏
#[cfg(feature = "utoipa")]
pub fn route_handler_no_register(args: RouteAttr, mut fn_item: ItemFn) -> TokenStream {
    let fn_name = fn_item.sig.ident.clone();
    let _layer_attrs = extract_layer_attrs(&fn_item.attrs);
    fn_item.attrs.retain(|attr| !attr.path().is_ident("layer"));

    // 保存原始签名用于 OpenAPI 推断
    let original_attrs = fn_item.attrs.clone();
    let original_inputs = fn_item.sig.inputs.clone();
    let original_output = fn_item.sig.output.clone();

    // 自动返回值
    let sig = &mut fn_item.sig;
    if matches!(sig.output, syn::ReturnType::Default) {
        sig.output = parse_quote!(-> impl ::miko::http::response::into_response::IntoResponse)
    }
    let inject_segs: Vec<Stmt> = Vec::new();
    let rfa = RouteFnArg::from_punctuated(&mut sig.inputs);
    //处理路由
    let path_inputs = rfa.gen_fn_args(deal_with_path_attr);
    //处理body
    let body_inputs = rfa.gen_fn_args(deal_with_body_attr);
    let plain_inputs = rfa.gen_fn_args(|rfa| {
        if rfa.mark.is_empty() {
            FnArgResult::Keep
        } else {
            FnArgResult::Remove
        }
    });
    // 处理 dep，将其转换为内部 Dep<T> 提取器
    let (dep_inputs, dep_stmts) = build_dep_extractors(&rfa);
    // 处理config_value
    let mut config_value_stmts = Vec::new();
    build_config_value_injector(&rfa, &mut config_value_stmts);
    // 清空参数
    sig.inputs.clear();
    // 获取无修饰参数
    // 组装path
    sig.inputs.extend(path_inputs);
    // 构建 Query 结构体和解构提取器
    let q_struct_ident = Ident::new(&format!("__{}_QueryStruct", fn_name), Span::call_site());
    // 重组Query
    let (q_struct, q_struct_exactor) = build_struct_from_query(&rfa, q_struct_ident);
    if q_struct.is_some() {
        sig.inputs.push(q_struct_exactor.unwrap());
    }
    sig.inputs.extend(dep_inputs);
    // 组装plain_inputs
    sig.inputs.extend(plain_inputs);
    // 最后组装body
    sig.inputs.extend(body_inputs);
    // 展开
    let user_stmts = &fn_item.block.stmts.clone();

    // 生成 OpenAPI 文档 (不生成 inventory 注册)
    let utoipa_attr =
        generate_utoipa_attr(&args, &original_attrs, &original_inputs, &original_output);

    quote! {
      #q_struct

      #utoipa_attr
      #sig {
        #(#inject_segs)*
        #(#dep_stmts)*
        #(#config_value_stmts)*
        #(#user_stmts)*
      }
    }
    .into()
}

#[cfg(feature = "utoipa")]
fn generate_utoipa_attr(
    args: &RouteAttr,
    original_attrs: &[syn::Attribute],
    original_inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    original_output: &syn::ReturnType,
) -> proc_macro2::TokenStream {
    // 1. 解析用户配置
    let mut user_config = parse_utoipa_attrs(original_attrs);

    // 2. 自动推断
    let inferred = infer_openapi_config(original_attrs, original_inputs, original_output);

    // 3. 合并配置
    user_config.auto_summary = inferred.auto_summary;
    user_config.auto_description = inferred.auto_description;
    user_config.auto_params = inferred.auto_params;
    user_config.auto_response = inferred.auto_response;
    user_config.auto_request_body = inferred.auto_request_body;

    // 4. 确定 HTTP 方法
    let method = if let Some(ref methods) = args.method {
        if let Some(first_method) = methods.first() {
            HttpMethod::from_hyper_method(first_method)
        } else {
            HttpMethod::Get
        }
    } else {
        HttpMethod::Get
    };

    // 5. 生成 utoipa::path 宏
    generate_utoipa_path_attr(&method, &args.path, &user_config)
}
