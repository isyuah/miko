use crate::toolkit::rout_arg::RouteFnArg;
use proc_macro2::Ident;
use quote::quote;
use syn::{FnArg, ItemStruct, parse_quote};

/// 根据带有 `#[query]` 标记的参数构建一个临时的查询结构体和对应的提取器参数。
///
/// - `rfa`：函数参数解析出的 RouteFnArg 列表；
/// - `struct_name`：为生成的结构体提供标识符。
///
/// 返回值为 (Option<ItemStruct>, Option<FnArg>)，当没有带 `#[query]` 的参数时返回 (None, None)。
pub fn build_struct_from_query(
    rfa: &Vec<RouteFnArg>,
    struct_name: Ident,
) -> (Option<ItemStruct>, Option<FnArg>) {
    let mut fields = Vec::new();
    let mut idents = Vec::new();
    for rfa in rfa {
        if rfa.mark.contains_key("query") {
            let name = &rfa.ident;
            let ty = rfa.ty.clone();
            idents.push(name.clone());

            // 提取参数上的 #[desc] 注释
            let desc_attr = extract_desc_attr(&rfa.attrs);

            fields.push(quote! {
                #desc_attr
                pub #name: #ty
            })
        }
    }
    if !fields.is_empty() {
        // 根据是否启用 utoipa feature 决定是否派生 IntoParams
        #[cfg(feature = "utoipa")]
        let derives = quote! {
            #[derive(::miko::serde::Deserialize, ::miko::utoipa::IntoParams)]
        };

        #[cfg(not(feature = "utoipa"))]
        let derives = quote! {
            #[derive(::miko::serde::Deserialize)]
        };

        let q_struct: ItemStruct = parse_quote! {
            #derives
            struct #struct_name {
                #(#fields),*
            }
        };
        let stmt: FnArg = parse_quote! {
            ::miko::extractor::Query(#struct_name { #(#idents),* }): ::miko::extractor::Query<#struct_name>
        };
        (Some(q_struct), Some(stmt))
    } else {
        (None, None)
    }
}

/// 从属性中提取 #[desc("...")] 并转换为 utoipa 的 #[schema(description = "...")]
fn extract_desc_attr(attrs: &[syn::Attribute]) -> proc_macro2::TokenStream {
    #[cfg(feature = "utoipa")]
    {
        for attr in attrs {
            if attr.path().is_ident("desc")
                && let Ok(lit) = attr.parse_args::<syn::LitStr>()
            {
                return quote! {
                    #[schema(description = #lit)]
                };
            }
        }
    }

    #[cfg(not(feature = "utoipa"))]
    let _ = attrs; // 避免未使用参数警告

    quote! {}
}
