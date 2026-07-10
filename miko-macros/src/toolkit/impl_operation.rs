#![cfg(feature = "auto")]
use crate::toolkit::rout_arg::is_arc;
use proc_macro2::Ident;
use proc_macro2::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{FnArg, ImplItem, ImplItemFn, Pat, PatIdent};

/// 在 `impl` 项目列表中查找名为 `new` 的构造函数并返回其引用（如果存在）。
///
/// 用于在宏中检测并提取异步构造函数以进行依赖注入分析。
pub fn get_constructor(items: &Vec<ImplItem>) -> Option<&ImplItemFn> {
    for item in items {
        if let ImplItem::Fn(method) = item
            && method.sig.ident == "new"
        {
            return Some(method);
        }
    }
    None
}

/// 从构造函数参数列表中生成依赖注入的获取语句并收集参数标识符。
///
/// 根据构造函数参数是否为 `Arc<T>` 选择共享解析或 transient 按值解析。
pub fn inject_deps(
    args: &Punctuated<FnArg, Comma>,
    depend_get_stmts: &mut Vec<TokenStream>,
    arg_idents: &mut Vec<Ident>,
) {
    for arg in args {
        if let FnArg::Typed(pat) = arg {
            let arg_ident = match &*pat.pat {
                Pat::Ident(PatIdent { ident, .. }) => ident.clone(),
                _ => {
                    panic!("service method new argument must be Typed")
                }
            };
            let (is_arc, inner) = is_arc(&pat.ty);
            if is_arc {
                let inner = inner.expect("Arc dependency should have an inner type");
                depend_get_stmts.push(quote! {
                    let #arg_ident = __resolve_context.resolve::<#inner>().await?;
                });
            } else {
                let dependency_type = &pat.ty;
                depend_get_stmts.push(quote! {
                    let #arg_ident =
                        __resolve_context.resolve_owned::<#dependency_type>().await?;
                });
            }
            arg_idents.push(arg_ident);
        }
    }
}
