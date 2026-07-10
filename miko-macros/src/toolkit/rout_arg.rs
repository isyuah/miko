use crate::toolkit::attr::StrAttrMap;
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use std::collections::HashMap;
use std::fmt;
use std::fmt::{Debug, Formatter};
use syn::{FnArg, Meta, Type, TypePath};
#[allow(dead_code)]
#[derive(Clone)]
pub struct RouteFnArg {
    pub ident: syn::Ident,
    pub ty: Type,
    pub attrs: Vec<syn::Attribute>,
    pub is_option: bool,
    pub mark: HashMap<String, StrAttrMap>,
    pub origin: FnArg,
}
impl Debug for RouteFnArg {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteFnArg")
            .field("ident", &self.ident)
            .field("ty", &self.ty.to_token_stream().to_string())
            .field("mark", &self.mark)
            .finish()
    }
}

impl RouteFnArg {
    /// 从函数参数的 Punctuated 列表中解析出 RouteFnArg 向量。
    ///
    /// 该函数会处理 `FnArg::Typed` 参数，提取参数标识符、类型及自定义属性（如 `#[path]`、`#[body]`、`#[dep]`、`#[config]` 等），
    /// 并将解析结果打包为 `RouteFnArg`，以便后续宏展开使用。
    pub fn from_punctuated(
        inputs: &mut syn::punctuated::Punctuated<FnArg, syn::token::Comma>,
    ) -> Vec<RouteFnArg> {
        let mut out = Vec::new();
        for input in inputs {
            let input_clone = input.clone();
            if let FnArg::Typed(pat) = input {
                let mut mark = HashMap::new();
                let ident = match &*pat.pat {
                    syn::Pat::Ident(pat_ident) => Some(pat_ident.ident.clone()),
                    syn::Pat::TupleStruct(pat_ts) => {
                        let mut pat_ts = pat_ts.clone();
                        while let Some(syn::Pat::TupleStruct(pat_tsn)) = pat_ts.elems.first() {
                            pat_ts = pat_tsn.clone();
                        }
                        if let syn::Pat::Ident(pat_ident) = pat_ts.elems.first().unwrap() {
                            Some(pat_ident.ident.clone())
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                let (is_option, _option_ty) = is_option(&pat.ty);
                if ident.is_none() {
                    panic!("RouteFnArg must have an ident");
                }
                for attr in &pat.attrs {
                    let mut sam = StrAttrMap::new();
                    if let Meta::List(list) = &attr.meta {
                        let tk = list.tokens.clone();
                        sam = syn::parse2(tk).unwrap();
                    }
                    let ident_str = attr.path().get_ident().unwrap().to_string();
                    mark.insert(ident_str, sam);
                }
                let rfa = RouteFnArg {
                    ident: ident.unwrap(),
                    ty: *pat.ty.clone(),
                    is_option,
                    attrs: vec![],
                    mark,
                    origin: input_clone,
                };
                out.push(rfa);
            }
        }
        out
    }

    pub fn marked_by(&self, key: &str) -> bool {
        self.mark.contains_key(key)
    }
}

pub trait IntoFnArgs {
    fn gen_fn_args(&self, callback: impl FnMut(&RouteFnArg) -> FnArgResult) -> Vec<FnArg>;
}

impl IntoFnArgs for Vec<RouteFnArg> {
    fn gen_fn_args(&self, mut callback: impl FnMut(&RouteFnArg) -> FnArgResult) -> Vec<FnArg> {
        let mut out = Vec::new();
        for rfa in self {
            let mut clone = rfa.origin.clone();
            match callback(rfa) {
                FnArgResult::Remove => {}
                FnArgResult::Keep => out.push(rfa.origin.clone()),
                FnArgResult::Replace(new) => out.push(new),
                FnArgResult::RemoveAttr => {
                    if let FnArg::Typed(ref mut pat) = clone {
                        pat.attrs.clear();
                    }
                    out.push(clone);
                }
            }
        }
        out
    }
}

/// 判断给定类型是否为 `Option<T>`，
/// 返回 (is_option, Some(inner_type)) 或 (false, None)
pub fn is_option(ty: &Type) -> (bool, Option<Type>) {
    let Type::Path(TypePath { path, .. }) = ty else {
        return (false, None);
    };
    let last = path.segments.last().unwrap();
    if last.ident == "Option" {
        match &last.arguments {
            syn::PathArguments::AngleBracketed(args) => {
                let ty = args.args.first().unwrap();
                let ty = match ty {
                    syn::GenericArgument::Type(ty) => ty,
                    _ => panic!("Option must have a type"),
                };
                (true, Some(ty.clone()))
            }
            _ => (false, None),
        }
    } else {
        (false, None)
    }
}

/// 判断给定类型是否为 `Arc<T>`，
/// 返回 (is_arc, Some(inner_type)) 或 (false, None)
pub fn is_arc(ty: &Type) -> (bool, Option<Type>) {
    let Type::Path(TypePath { path, .. }) = ty else {
        return (false, None);
    };
    let last = path.segments.last().unwrap();
    if last.ident == "Arc" {
        match &last.arguments {
            syn::PathArguments::AngleBracketed(args) => {
                let ty = args.args.first().unwrap();
                let ty = match ty {
                    syn::GenericArgument::Type(ty) => ty,
                    _ => panic!("Arc must have a type"),
                };
                (true, Some(ty.clone()))
            }
            _ => (false, None),
        }
    } else {
        (false, None)
    }
}

#[allow(dead_code)]
pub enum FnArgResult {
    Remove,
    Keep,
    Replace(FnArg),
    RemoveAttr,
}

/// 将 `#[dep] value: Arc<T>` 转换为内部 `Dep<T>` 提取器，并恢复用户变量。
pub fn build_dep_extractors(rfa: &[RouteFnArg]) -> (Vec<FnArg>, Vec<TokenStream>) {
    let mut dep_inputs = Vec::new();
    let mut dep_stmts = Vec::new();

    for (index, rfa) in rfa.iter().enumerate() {
        if rfa.mark.contains_key("dep") {
            let dep_ty = rfa.ty.clone();
            let (is_arc, inner) = is_arc(&dep_ty);
            let dep_ident = rfa.ident.clone();
            let extractor_ident = format_ident!("__miko_dep_{index}");
            let input = if is_arc {
                let inner = inner.expect("Arc dependency should have an inner type");
                syn::parse2(quote! {
                    ::miko::dependency_container::Dep(#extractor_ident):
                        ::miko::dependency_container::Dep<#inner>
                })
            } else {
                syn::parse2(quote! {
                    ::miko::dependency_container::OwnedDep(#extractor_ident):
                        ::miko::dependency_container::OwnedDep<#dep_ty>
                })
            }
            .expect("generated dependency extractor should be valid");
            dep_inputs.push(input);
            dep_stmts.push(quote! {
                let #dep_ident = #extractor_ident;
            });
        }
    }

    (dep_inputs, dep_stmts)
}

/// 为中间件中的 `#[dep]` 参数生成基于当前请求作用域的解析语句。
pub fn build_dep_injector(
    rfa: &[RouteFnArg],
    request_ident: &syn::Ident,
    dep_stmts: &mut Vec<TokenStream>,
) {
    for rfa in rfa {
        if rfa.mark.contains_key("dep") {
            let dep_ty = rfa.ty.clone();
            let (is_arc, inner) = is_arc(&dep_ty);
            let dep_ident = rfa.ident.clone();
            if is_arc {
                let inner = inner.expect("Arc dependency should have an inner type");
                dep_stmts.push(quote! {
                    let #dep_ident =
                        ::miko::dependency_container::resolve_from_request::<#inner>(
                            &#request_ident
                        ).await?;
                });
            } else {
                dep_stmts.push(quote! {
                    let #dep_ident =
                        ::miko::dependency_container::resolve_owned_from_request::<#dep_ty>(
                            &#request_ident
                        ).await?;
                });
            }
        }
    }
}

/// 为带有 `#[config(...)]` 的参数生成从配置读取并解析值的语句。
///
/// 支持所有实现 `serde::de::DeserializeOwned` 的类型,包括基础类型、集合、自定义结构体等。
pub fn build_config_value_injector(
    rfa: &Vec<RouteFnArg>,
    config_value_stmts: &mut Vec<TokenStream>,
) {
    for rfa in rfa {
        let mark_item = rfa.mark.get("config");
        if let Some(item) = mark_item {
            if let Some(path) = item.get_or_default("path") {
                let (is_option, inner) = is_option(&rfa.ty);
                let parse_expr = if is_option {
                    parse_expr_by_type(&inner.unwrap(), path, rfa.ident.clone(), false)
                } else {
                    parse_expr_by_type(&rfa.ty, path, rfa.ident.clone(), true)
                };
                config_value_stmts.push(parse_expr);
            } else {
                panic!("config param must be like #[config(\"xx\")] or #[config(path=\"xx\")] ");
            }
        }
    }
}

fn parse_expr_by_type(ty: &Type, path: String, ident: syn::Ident, unwrap: bool) -> TokenStream {
    if unwrap {
        quote! {
            let #ident = ::miko::app::config::get_settings_value::<#ty>(#path)?;
        }
    } else {
        quote! {
            let #ident = ::miko::app::config::get_settings_value::<#ty>(#path);
        }
    }
}

#[allow(unused)]
pub fn build_clone_stmts(rfa: &Vec<RouteFnArg>, stmts: &mut Vec<TokenStream>) {
    for r in rfa {
        build_clone_stmt(r, stmts);
    }
}
pub fn build_clone_stmt(rfa: &RouteFnArg, stmts: &mut Vec<TokenStream>) {
    let ident = &rfa.ident;
    stmts.push(quote! {
        let #ident = #ident.clone();
    })
}
