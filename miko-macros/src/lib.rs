use crate::route::RouteAttr;
use crate::route::core::route_handler;
use crate::toolkit::attr::StrAttrMap;
#[cfg(feature = "auto")]
use crate::toolkit::impl_operation::{get_constructor, inject_deps};
use crate::toolkit::rout_arg::{
    FnArgResult, IntoFnArgs, RouteFnArg, build_clone_stmt, build_config_value_injector,
    build_dep_injector,
};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{ItemFn, ItemMod, parse_macro_input};

mod extractor;
mod mod_transform;
mod route;
mod toolkit;

#[cfg(feature = "utoipa")]
mod utoipa;

/// 标准路由属性宏（用于自定义路由）
///
/// 用法：在处理请求的函数上使用 `#[route(...)]` 或派生宏如 `#[get(...)]`。
/// 该宏根据属性（path/method）和参数注解生成路由处理器。
///
/// 参数标注：
/// - `#[path]`：从路径中提取（如 `/users/{id}`）；
/// - `#[query]`：从查询字符串构建结构并注入；
/// - `#[body]`：从请求体反序列化（默认 JSON；标记 `str` 可保留为 String）；
/// - `#[dep]`：注入全局依赖（参数类型通常为 `Arc<T>`，需先注册该组件）；
/// - `#[config("key")]`/`#[config(path = "key")]`：从应用配置读取并解析为参数类型。
/// - `#[desc("描述")]`：为参数添加描述（启用 utoipa 时会生成 OpenAPI 文档）；
///
/// 注意：
/// - 仅当同时启用 `auto` feature 且应用通过 `#[miko]` 启动时，框架才会自动收集并注册由这些宏生成的路由；
/// - 若未启用 `auto`，`route`/派生宏及 `#[dep]` 不会触发框架级的自动注册或依赖注入——此时需要在你的初始化代码中手动注册路由与依赖；
///
/// 建议：处理器应声明为 `async fn`；若未显式返回类型，宏会自动设置为实现 `IntoResponse` 的类型。
///
/// 示例：
/// ```rust,ignore
/// #[get("/hello/{id}")]
/// async fn hello(
///     #[path] #[desc("用户ID")] id: i32
/// ) -> impl miko::http::response::into_response::IntoResponse {
///     // 处理请求
/// }
/// ```
#[proc_macro_attribute]
pub fn route(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as RouteAttr);
    let fn_item = parse_macro_input!(item as ItemFn);
    route_handler(args, fn_item)
}

/// # Miko宏
/// 自动配置
/// - 展开出#\[tokio::main]
/// - 注册依赖[仅限auto]
/// - 加载配置到_config
/// - 新建router: Router
/// - > 用户代码
/// - 收集定义#\[get]等宏定义的路由并注册
/// - 运行app
#[proc_macro_attribute]
pub fn miko(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;
    let str_attr_map = parse_macro_input!(attr as StrAttrMap);
    let user_statements = &input_fn.block.stmts;
    let set_panic_hook = if str_attr_map.map.contains_key("sse") {
        Some(quote! {
            ::miko::http::response::sse::set_sse_panic_hook();
        })
    } else {
        None
    };
    let catch_panic = if str_attr_map.map.contains_key("catch") {
        if cfg!(feature = "catch_panic") {
            Some(quote! {
                router.with_catch_panic();
            })
        } else {
            return quote! {
                compile_error!("`catch` attribute requires `catch_panic` feature to be enabled");
            }
            .into();
        }
    } else {
        None
    };
    let build_sign = str_attr_map.map.contains_key("build");
    if build_sign {
        quote! {
            #fn_vis async fn #fn_name() -> ::miko::app::Application {
                #set_panic_hook
                let mut _config = ::miko::app::config::ServerSettings::from_global_settings();
                let mut router = ::miko::router::Router::new();
                #catch_panic

                #( #user_statements )*

                router.merge(::miko::auto::collect_global_router());
                ::miko::app::Application::new(_config, router.take())
            }
        }
    } else {
        quote! {
            #[::miko::tokio::main]
            async fn main() {
                #set_panic_hook
                let mut _config = ::miko::app::config::ServerSettings::from_global_settings();
                let mut router = ::miko::router::Router::new();
                #catch_panic

                #( #user_statements )*

                router.merge(::miko::auto::collect_global_router());
                let app = ::miko::app::Application::new(_config, router.take());
                app.run().await.unwrap();
            }
        }
    }
    .into()
}
macro_rules! derive_route_macro {
    ($macro_name: ident, $method_ident:ident) => {
        #[doc = concat!("简写：等价于 `#[route(..., method = \"", stringify!($method_ident), "\" )]`。\n\n",
                         "仅当启用 `auto` feature 且应用通过 `#[miko]` 启动时，框架才会自动注册由该宏生成的路由；\n",
                         "否则该宏仅生成处理函数，路由需在初始化代码中手动注册。")]
        #[proc_macro_attribute]
        pub fn $macro_name(attr: TokenStream, item: TokenStream) -> TokenStream {
            let mut args = syn::parse_macro_input!(attr as RouteAttr);
            let fn_item = syn::parse_macro_input!(item as ItemFn);
            let method_to_add = ::hyper::Method::$method_ident;
            match &mut args.method {
                Some(existing_methods) => {
                    existing_methods.push(method_to_add);
                }
                None => {
                    args.method = Some(vec![method_to_add]);
                }
            }
            route_handler(args, fn_item)
        }
    };
}

derive_route_macro!(get, GET);
derive_route_macro!(post, POST);
derive_route_macro!(put, PUT);
derive_route_macro!(delete, DELETE);
derive_route_macro!(patch, PATCH);
derive_route_macro!(head, HEAD);
derive_route_macro!(options, OPTIONS);
derive_route_macro!(trace, TRACE);
derive_route_macro!(connect, CONNECT);

#[cfg(feature = "auto")]
/// 组件宏：将 `impl` 中的构造函数注册为可由框架管理的可注入组件。
///
/// 使用：
/// - 在 `impl` 上添加 `#[component]`（可带 `prewarm`）以将该类型注册为预热组件；
/// - 使用 `#[component(request)]` 声明每个 HTTP 请求内复用的组件；
/// - 使用 `#[component(transient)]` 声明每次解析都重新创建的组件；
/// - 构造函数应为 `async fn new(...) -> Self`；`Arc<T>` 可解析任意生命周期，按值 `T` 仅可解析 transient 组件；
/// - 注册后的组件可在处理器参数上使用 `#[dep]` 标注注入（当启用 `auto` 时）。
///
/// `prewarm` 生效条件：仅在应用通过 `#[miko]` 启动（并启用 `auto`）时才会在启动阶段触发预热。
///
/// 示例：
/// ```rust,ignore
/// #[component(prewarm)]
/// impl MyService {
///     async fn new(dep: std::sync::Arc<Other>) -> Self { /* ... */ }
/// }
///
/// // 在处理器中注入：
/// async fn handler(#[dep] svc: std::sync::Arc<MyService>) { /* ... */ }
/// ```
#[proc_macro_attribute]
pub fn component(attr: TokenStream, input: TokenStream) -> TokenStream {
    use quote::format_ident;
    use syn::{ItemImpl, TypePath};
    let args = syn::parse_macro_input!(attr as StrAttrMap);
    let input_struct = parse_macro_input!(input as ItemImpl);
    let prewarm = args.get("prewarm").is_some();
    let mut lifetime = "singleton".to_string();
    let mut lifetime_specified = false;

    let mut set_lifetime = |mode: &str| {
        let normalized = mode.to_ascii_lowercase();
        match normalized.as_str() {
            "singleton" | "request" | "transient" => {
                if lifetime_specified && lifetime != normalized {
                    panic!(
                        "Conflicting #[component] lifetime: both '{}' and '{}' specified",
                        lifetime, normalized
                    );
                }
                lifetime = normalized;
                lifetime_specified = true;
            }
            _ => panic!(
                "Invalid #[component] lifetime '{}'. Expected `singleton`, `request`, or `transient`.",
                mode
            ),
        }
    };

    if let Some(mode) = args.get("mode") {
        set_lifetime(mode);
    } else if let Some(default_mode) = args.default.as_ref() {
        set_lifetime(default_mode);
    }

    if args.map.contains_key("singleton") {
        set_lifetime("singleton");
    }
    if args.map.contains_key("transient") {
        set_lifetime("transient");
    }
    if args.map.contains_key("request") {
        set_lifetime("request");
    }

    if prewarm && lifetime != "singleton" {
        panic!("`#[component(prewarm)]` is only valid for singleton components");
    }

    let lifetime_tokens = match lifetime.as_str() {
        "singleton" => quote!(::miko::dependency_container::DependencyLifetime::Singleton),
        "request" => quote!(::miko::dependency_container::DependencyLifetime::Request),
        "transient" => quote!(::miko::dependency_container::DependencyLifetime::Transient),
        _ => unreachable!(),
    };
    let mut depend_get_stmts = Vec::new();
    let mut arg_idents = Vec::new();
    let type_ident = match *input_struct.self_ty.clone() {
        syn::Type::Path(TypePath { path, .. }) => path
            .segments
            .last()
            .map(|seg| seg.ident.clone())
            .unwrap_or_else(|| format_ident!("UnknowType")),
        _ => format_ident!("UnknowType"),
    };
    if let Some(method) = get_constructor(&input_struct.items) {
        if method.sig.asyncness.is_none() {
            panic!("service method new must be async")
        }
        let args = &method.sig.inputs;
        inject_deps(args, &mut depend_get_stmts, &mut arg_idents);
    }
    quote! {
        #input_struct
        ::miko::inventory::submit! {
            ::miko::dependency_container::DependencyDefFn(|| {
                ::miko::dependency_container::DependencyDef {
                    type_id: std::any::TypeId::of::<#type_ident>(),
                    type_name: std::any::type_name::<#type_ident>(),
                    prewarm: #prewarm,
                    name: "___",
                    lifetime: #lifetime_tokens,
                    init_fn: |__resolve_context| {
                        Box::pin(async move {
                            #(#depend_get_stmts)*
                            let val: #type_ident = #type_ident::new(#(#arg_idents),*).await;
                            Ok(
                                ::std::boxed::Box::new(val)
                                    as ::std::boxed::Box<dyn ::std::any::Any + Send + Sync>
                            )
                        })
                    }
                }
            })
        }
    }
    .into()
}

// ==================== Utoipa 辅助宏 ====================

#[cfg(feature = "utoipa")]
/// 标记响应信息
///
/// 用法：
/// ```rust,ignore
/// #[u_response(status = 404, description = "用户不存在", body = ErrorResponse)]
/// ```
#[proc_macro_attribute]
pub fn u_response(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // 这个宏不做任何转换，只是作为标记供 route 宏读取
    item
}

#[cfg(feature = "utoipa")]
/// 标记 API 标签
///
/// 用法：
/// ```rust,ignore
/// #[u_tag("用户管理")]
/// ```
#[proc_macro_attribute]
pub fn u_tag(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[cfg(feature = "utoipa")]
/// 标记 API 摘要
///
/// 用法：
/// ```rust,ignore
/// #[u_summary("获取用户信息")]
/// ```
#[proc_macro_attribute]
pub fn u_summary(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[cfg(feature = "utoipa")]
/// 标记 API 详细描述
///
/// 用法：
/// ```rust,ignore
/// #[u_description("根据用户 ID 获取详细信息")]
/// ```
#[proc_macro_attribute]
pub fn u_description(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[cfg(feature = "utoipa")]
/// 标记自定义请求体
///
/// 用于处理无法自动推断的请求体类型，比如 `Multipart`。
/// 当使用此宏时，会覆盖自动推断的请求体配置。
///
/// 参数：
/// - `content`: 请求体的类型（必需）
/// - `content_type`: Content-Type 头（可选，默认为 "application/json"）
/// - `description`: 请求体描述（可选）
///
/// 用法：
/// ```rust,ignore
/// use miko::http::extractor::Multipart;
///
/// #[post("/upload")]
/// #[u_request_body(
///     content = Multipart,
///     content_type = "multipart/form-data",
///     description = "文件上传"
/// )]
/// async fn upload_file(multipart: Multipart) -> impl IntoResponse {
///     // 处理文件上传
/// }
/// ```
#[proc_macro_attribute]
pub fn u_request_body(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[cfg(feature = "utoipa")]
/// 标记参数补充信息
///
/// 用法：
/// ```rust,ignore
/// #[u_param(name = "id", description = "用户ID", example = 123)]
/// ```
#[proc_macro_attribute]
pub fn u_param(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[cfg(feature = "utoipa")]
/// 标记 API 已弃用
///
/// 用法：
/// ```rust,ignore
/// #[u_deprecated]
/// ```
#[proc_macro_attribute]
pub fn u_deprecated(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// 为参数添加描述
///
/// 用于给函数参数添加描述信息，在启用 utoipa feature 时会生成到 OpenAPI 文档中。
///
/// 用法:
/// ```rust,ignore
/// #[get("/users/{id}")]
/// async fn get_user(
///     #[path] #[desc("用户ID")] id: i32,
///     #[query] #[desc("页码")] page: Option<i32>
/// ) -> impl IntoResponse {
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn desc(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // 这个宏不做任何转换，只是作为标记供其他宏读取
    item
}

// 防止覆盖 builtin 宏
// /// 标记路径参数
// ///
// /// 用于标记从 URL 路径中提取的参数。
// ///
// /// 用法:
// /// ```rust,ignore
// /// #[get("/users/:id")]
// /// async fn get_user(#[path] id: i32) -> impl IntoResponse {
// ///     // ...
// /// }
// /// ```
// #[proc_macro_attribute]
// pub fn path(_attr: TokenStream, item: TokenStream) -> TokenStream {
//     // 这个宏不做任何转换，只是作为标记供 route 宏读取
//     item
// }

/// 标记查询参数
///
/// 用于标记从 URL 查询字符串中提取的参数。
///
/// 用法:
/// ```rust,ignore
/// #[get("/users")]
/// async fn list_users(
///     #[query] page: Option<i32>,
///     #[query] page_size: Option<i32>
/// ) -> impl IntoResponse {
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn query(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // 这个宏不做任何转换，只是作为标记供 route 宏读取
    item
}

/// 标记请求体参数
///
/// 用于标记从请求体中提取的参数。
///
/// 用法:
/// ```rust,ignore
/// #[post("/users")]
/// async fn create_user(#[body] user: User) -> impl IntoResponse {
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn body(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // 这个宏不做任何转换，只是作为标记供 route 宏读取
    item
}

/// 标记 Tower Layer
///
/// 用于在路由处理函数或模块上应用 Tower Layer 中间件。
///
/// **在函数上使用：** 可以使用多个 `#[layer]` 属性，它们将从上到下声明，从内到外应用。
/// **在模块上使用：** 为模块内的所有路由自动添加指定的 layer。
///
/// 用法:
/// ```rust,ignore
/// use tower_http::timeout::TimeoutLayer;
/// use std::time::Duration;
///
/// // 单个 layer（函数级）
/// #[get("/users/{id}")]
/// #[layer(TimeoutLayer::new(Duration::from_secs(30)))]
/// async fn get_user(#[path] id: i32) -> impl IntoResponse {
///     // ...
/// }
///
/// // 多个 layer（函数级）
/// #[post("/users")]
/// #[layer(TimeoutLayer::new(Duration::from_secs(30)))]
/// #[layer(CompressionLayer::new())]
/// async fn create_user(#[body] user: User) -> impl IntoResponse {
///     // 调用链: CompressionLayer -> TimeoutLayer -> handler
/// }
///
/// // 模块级 layer
/// #[layer(AuthLayer::new())]
/// mod protected {
///     #[get("/data")]
///     async fn get_data() { }  // 自动应用 AuthLayer
/// }
/// ```
#[proc_macro_attribute]
pub fn layer(attr: TokenStream, item: TokenStream) -> TokenStream {
    if let Ok(mut mod_item) = syn::parse::<ItemMod>(item.clone()) {
        let layer_attr = parse_macro_input!(attr as mod_transform::ModLayerAttr);
        mod_transform::apply_transform_to_module(
            &mut mod_item,
            mod_transform::TransformOp::Layer(layer_attr.expr),
        );
        return quote! { #mod_item }.into();
    }
    item
}

#[cfg(feature = "utoipa")]
/// 仅生成 OpenAPI 文档，不自动注册路由
///
/// 用于手动注册路由的场景。该宏生成 utoipa::path 属性，但不会通过 inventory 自动注册路由。
/// 你需要手动将这个函数注册到 Router 中。
///
/// 与 `#[get]`, `#[post]` 等宏的区别：
/// - `#[get]` 等宏: 自动注册路由 + 生成 OpenAPI (需要 auto feature)
/// - `#[miko_path]`: 只生成 OpenAPI，需要手动注册路由
///
/// 用法:
/// ```rust,ignore
/// // 1. 使用 miko_path 宏生成 OpenAPI
/// #[miko::miko_path(path = "/manual")]
/// #[u_tag("Manual")]
/// #[u_response(status = 200, body = User)]
/// async fn manual_handler() -> Json<User> {
///     // ...
/// }
///
/// // 2. 手动注册路由
/// let router = Router::new()
///     .get("/manual", manual_handler);
///
/// // 3. OpenApi 定义中可以引用
/// #[derive(miko::OpenApi)]
/// #[openapi(
///     paths(manual_handler),  // 仍然可以在这里引用
/// )]
/// struct ApiDoc;
/// ```
#[proc_macro_attribute]
pub fn miko_path(attr: TokenStream, item: TokenStream) -> TokenStream {
    // 解析 HTTP 方法和路径
    // 格式: #[miko_path(path = "/xxx")] 或 #[miko_path(path = "/xxx", ...)]
    let args = parse_macro_input!(attr as RouteAttr);
    let fn_item = parse_macro_input!(item as ItemFn);

    // 为了简化，我们生成一个不带 inventory 的版本
    use crate::route::core::route_handler_no_register;
    route_handler_no_register(args, fn_item)
}

/// # Prefix 宏：模块路由前缀
///
/// 用法：在 `mod` 块上使用 `#[prefix("/api")]`，会自动为模块内的所有路由添加前缀。
///
/// **注意：和Router::nest不同，prefix只是简单地在内部路由路径前添加前缀，并不会将运行时内部路由获取到的路径进行修改。**
///
/// 行为：
/// - 对模块内直接的函数（如果有路由宏）添加路径前缀
/// - 对模块内的嵌套 mod 也应用相同的前缀（如果嵌套 mod 内部没有 prefix，会继续附加）
/// - 支持路径的自动合并（处理多余的斜杠）
///
/// 示例：
/// ```rust,ignore
/// #[prefix("/api")]
/// mod api {
///     #[get("/users")]
///     async fn get_users() { }  // 实际注册为 GET /api/users
/// }
/// ```
#[proc_macro_attribute]
pub fn prefix(attr: TokenStream, item: TokenStream) -> TokenStream {
    let prefix_attr = parse_macro_input!(attr as mod_transform::PrefixAttr);
    let mut mod_item = parse_macro_input!(item as ItemMod);
    mod_transform::apply_transform_to_module(
        &mut mod_item,
        mod_transform::TransformOp::Prefix(prefix_attr.path),
    );
    quote! { #mod_item }.into()
}

/// 中间件
#[proc_macro_attribute]
pub fn middleware(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let vis = &input_fn.vis;
    let block = &input_fn.block;
    let attrs = &input_fn.attrs;
    let args = RouteFnArg::from_punctuated(&mut input_fn.sig.inputs);
    let mut req_ident = format_ident!("_req");
    let mut next_ident = format_ident!("_next");
    let mut config_stmts = Vec::new();
    let mut deps_stmts = Vec::new();
    let mut clone_stmts = Vec::new();
    let outer_args = args.gen_fn_args(|rfa| {
        // 判断是否是req, next
        if let syn::Type::Path(path) = &rfa.ty {
            if path.path.segments.last().unwrap().ident == "Req" {
                req_ident = rfa.ident.clone();
                return FnArgResult::Remove;
            } else if path.path.segments.last().unwrap().ident == "Next" {
                next_ident = rfa.ident.clone();
                return FnArgResult::Remove;
            }
        }
        // 判断是否是 #[config] #[dep]
        if !rfa.mark.is_empty() {
            if rfa.marked_by("config") || rfa.marked_by("dep") {
                return FnArgResult::Remove;
            } else {
                panic!("middleware only support mark #[config] or #[dep]");
            }
        }
        // 其余变量
        build_clone_stmt(rfa, &mut clone_stmts);
        FnArgResult::Keep
    });
    build_dep_injector(&args, &req_ident, &mut deps_stmts);
    build_config_value_injector(&args, &mut config_stmts);
    let mut inputs = input_fn.sig.inputs;
    inputs.clear();
    inputs.extend(outer_args);
    quote! {
        #(#attrs)*
        #vis fn #fn_name (#inputs) -> ::miko::middleware::FromFnLayer<impl Fn(::miko::miko_core::Req, ::miko::middleware::Next) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ::miko::AppResult<::miko::miko_core::Resp>> + Send>> + Clone> {
            ::miko::middleware::middleware_from_fn(move |#req_ident: ::miko::miko_core::Req, #next_ident: ::miko::middleware::Next| {
                #( #clone_stmts )*
                Box::pin(async move {
                    #( #deps_stmts )*
                    #( #config_stmts )*
                    #block
                }) as ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ::miko::AppResult<::miko::miko_core::Resp>> + Send>>
            })
        }
    }.into()
}
