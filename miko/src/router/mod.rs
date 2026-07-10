pub mod nested;
pub mod router_svc;

use crate::AppError;
#[cfg(feature = "ext")]
use crate::ext::static_svc::StaticSvcBuilder;
use crate::extractor::{from_request::FromRequest, path_params::PathParams};
use crate::handler::{DynHandler, handler_to_svc};
use crate::handler::{FnOnceTuple, Req, Resp, TypedHandler};
use crate::http::response::into_response::IntoResponse;
use crate::router::router_svc::RouterSvc;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, Response, body::Incoming};
use matchit::Router as MRouter;
use miko_core::{BoxError, IntoMethods, MikoError, encode_route};
use nested::NestLayer;
#[cfg(feature = "ext")]
use std::path::PathBuf;
use std::{collections::HashMap, sync::Arc};
use tower::{Layer, Service, util::BoxCloneService};

/// 生成各 HTTP 方法的简化注册函数（如 get/post/...）
///
/// 这些函数会将给定的 handler 绑定到指定 path 上。
macro_rules! define_method {
    ($name:ident, $m:ident) => {
        /// 将处理函数绑定到给定路径上（此函数注册指定的 HTTP 方法）
        pub fn $name<F, A, Fut, R, M>(&mut self, path: &str, handler: F) -> &mut Self
        where
            F: FnOnceTuple<A, Output = Fut> + Clone + Send + Sync + 'static,
            A: FromRequest<S, M> + Send + 'static,
            Fut: Future<Output = R> + Send + 'static,
            R: IntoResponse,
            M: Send + Sync + 'static,
        {
            let handler = Arc::new(TypedHandler::new(handler, self.state.clone())) as DynHandler;
            self.routes
                .entry(Method::$m)
                .or_default()
                .insert(encode_route(path), handler_to_svc(handler.clone()))
                .unwrap();
            self.path_map
                .entry(Method::$m)
                .or_default()
                .insert(path.to_string(), handler_to_svc(handler.clone()));
            self
        }
    };
}

/// 生成绑定现有 Service 的便捷函数（如 get_service/post_service/...）
macro_rules! define_handle_service {
    ($name:ident, $m:ident) => {
        /// 将一个 Service 直接挂载到给定路径（此函数注册指定的 HTTP 方法）
        pub fn $name(&mut self, path: &str, svc: HttpSvc<Req>) -> &mut Self {
            self.routes
                .entry(Method::$m.clone())
                .or_insert_with(|| MRouter::new())
                .insert(encode_route(path), svc.clone())
                .unwrap();
            self.path_map
                .entry(Method::$m.clone())
                .or_insert_with(|| HashMap::new())
                .insert(path.to_string(), svc.clone());
            self
        }
    };
}

/// Tower 兼容的请求与服务别名
pub type HttpReq = Request<Incoming>;
/// Tower 兼容的 Service 类型别名
pub type HttpSvc<T = HttpReq> = BoxCloneService<T, Resp, AppError>;

type MikoLayer<T = Req> = Arc<dyn Fn(HttpSvc<T>) -> HttpSvc<T> + Send + Sync>;
/// 路由器，负责注册路由、挂载中间件/服务并进行请求分发
pub struct Router<S = ()> {
    /// 已注册的路由表（按方法分类）
    pub routes: HashMap<Method, MRouter<HttpSvc<Req>>>,
    /// 共享的全局状态，可由 State<T> 提取
    pub state: Arc<S>,
    /// 待应用的中间件层
    pub layers: Vec<MikoLayer>,
    /// 用于 nest/merge 的路径映射索引
    pub path_map: HashMap<Method, HashMap<String, HttpSvc<Req>>>,
}
impl<S> Clone for Router<S> {
    fn clone(&self) -> Self {
        Self {
            routes: self.routes.clone(),
            state: self.state.clone(),
            layers: self.layers.clone(),
            path_map: self.path_map.clone(),
        }
    }
}

impl<S: Send + Sync + 'static> Router<S> {
    /// 根据方法与路径查找对应的处理 Service，并返回路径参数
    pub fn find_handler(&self, method: &Method, path: &str) -> Option<(HttpSvc<Req>, PathParams)> {
        if let Some(router) = self.routes.get(method) {
            match router.at(path) {
                Ok(matched) => {
                    let handler = matched.value.clone();
                    Some((handler, PathParams::from(&matched.params)))
                }
                Err(_e) => None,
            }
        } else {
            None
        }
    }
    /// 直接处理一个请求（内部使用），会自动写入 PathParams 并执行 Service
    pub async fn handle(&self, method: &Method, path: &str, mut req: Req) -> Resp {
        if let Some(router) = self.routes.get(method) {
            match router.at(path) {
                Ok(matched) => {
                    req.extensions_mut()
                        .insert(PathParams::from(&matched.params));
                    let mut handler = matched.value.clone();
                    handler.call(req).await.into_response()
                }
                Err(_e) => Response::builder()
                    .status(hyper::StatusCode::NOT_FOUND)
                    .body(
                        Full::new(Bytes::from("Not Found"))
                            .map_err(Into::into)
                            .boxed_unsync(),
                    )
                    .unwrap(),
            }
        } else {
            Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .body(
                    Full::new(Bytes::from("Not Found"))
                        .map_err(Into::into)
                        .boxed_unsync(),
                )
                .unwrap()
        }
    }
}

impl Router {
    /// 创建一个空路由器
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
            state: Arc::new(()),
            layers: Vec::new(),
            path_map: HashMap::new(),
        }
    }
}

impl<S: Send + Sync + 'static> Router<S> {
    /// 将处理函数挂载到指定 path
    ///
    /// - 支持一次性注册多个方法：get/post/put/delete/head/options/trace/connect/patch
    /// - 处理函数参数由一组 Extractor 决定，返回值需实现 IntoResponse
    pub fn route<F, A, Fut, R, M>(
        &mut self,
        method: impl IntoMethods,
        path: &str,
        handler: F,
    ) -> &mut Self
    where
        F: FnOnceTuple<A, Output = Fut> + Clone + Send + Sync + 'static,
        A: FromRequest<S, M> + Send + 'static,
        Fut: Future<Output = R> + Send + 'static,
        R: IntoResponse,
        M: Send + Sync + 'static,
    {
        let handler = Arc::new(TypedHandler::new(handler, self.state.clone())) as DynHandler;
        for m in method.into_methods() {
            self.routes
                .entry(m.clone())
                .or_default()
                .insert(encode_route(path), handler_to_svc(handler.clone()))
                .unwrap();
            self.path_map
                .entry(m.clone())
                .or_default()
                .insert(path.to_string(), handler_to_svc(handler.clone()));
        }
        self
    }

    define_method!(get, GET);
    define_method!(post, POST);
    define_method!(put, PUT);
    define_method!(delete, DELETE);
    define_method!(head, HEAD);
    define_method!(options, OPTIONS);
    define_method!(trace, TRACE);
    define_method!(connect, CONNECT);
    define_method!(patch, PATCH);
    define_handle_service!(get_service, GET);
    define_handle_service!(post_service, POST);
    define_handle_service!(put_service, PUT);
    define_handle_service!(delete_service, DELETE);
    define_handle_service!(head_service, HEAD);
    define_handle_service!(options_service, OPTIONS);
    define_handle_service!(trace_service, TRACE);
    define_handle_service!(connect_service, CONNECT);
    define_handle_service!(patch_service, PATCH);
}

impl<S: Send + Sync + 'static> Router<S> {
    /// 挂载全局状态，供 State<T> 提取
    ///
    /// 注意：该方法会返回新的 Router<T> 类型，请重新赋值接收
    pub fn with_state<T>(self, state: T) -> Router<T> {
        Router {
            routes: self.routes,
            state: Arc::new(state),
            layers: self.layers,
            path_map: self.path_map,
        }
    }

    /// 合并另一个 Router，所有路由与索引一并合并
    pub fn merge<T>(&mut self, mut other: Router<T>) -> &mut Self {
        let layers = std::mem::take(&mut other.layers);

        for (method, _) in other.routes.drain() {
            for (path, mut svc) in other.path_map.get_mut(&method).unwrap().drain() {
                for apply in &layers {
                    svc = apply(svc);
                }
                let boxed: HttpSvc<Req> = BoxCloneService::new(svc);
                self.routes
                    .entry(method.clone())
                    .or_default()
                    .insert(&path, boxed.clone())
                    .unwrap();
                self.path_map
                    .entry(method.clone())
                    .or_default()
                    .insert(path, boxed.clone());
            }
        }
        self
    }

    /// 将另一个 Router 挂载到指定前缀
    ///
    /// 被挂载的 Router 内部匹配到的是去除前缀后的路径与参数
    /// 被挂载 Router 的 layers 会自动应用到其所有路由
    pub fn nest<T>(&mut self, prefix: &str, mut other: Router<T>) -> &mut Self {
        let prefix = prefix.trim_end_matches('/').to_string();
        let layers = std::mem::take(&mut other.layers);

        for (method, _) in other.routes.drain() {
            for (path, mut svc) in other.path_map.get_mut(&method).unwrap().drain() {
                for apply in &layers {
                    svc = apply(svc);
                }
                let layered = NestLayer::new(&prefix).layer(svc);
                let boxed: HttpSvc<Req> = BoxCloneService::new(layered);
                let new_path = format!("{}{}", prefix, path);
                self.routes
                    .entry(method.clone())
                    .or_default()
                    .insert(&new_path, boxed.clone())
                    .unwrap();
                self.path_map
                    .entry(method.clone())
                    .or_default()
                    .insert(new_path, boxed.clone());
            }
        }
        self
    }

    /// 将一个 Service 挂载到前缀下的所有路由（常用方法）
    ///
    /// 无需显式声明 `{*rest}`，会自动追加；如需手动控制，请使用 [`Router::service`]
    pub fn nest_service(&mut self, prefix: &str, svc: HttpSvc<Req>) {
        let prefix = prefix.trim_end_matches('/').to_string();
        let layered = NestLayer::new(&prefix).layer(svc);
        let boxed: HttpSvc<Req> = BoxCloneService::new(layered);
        let methods = [
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::PATCH,
            Method::HEAD,
            Method::OPTIONS,
        ];
        let new_path = format!("{}{}", prefix, "/{*rest}");
        let new_path_index = format!("{}{}", prefix, "/");
        for method in methods {
            self.routes
                .entry(method.clone())
                .or_default()
                .insert(&new_path, boxed.clone())
                .unwrap();
            self.path_map
                .entry(method.clone())
                .or_default()
                .insert(new_path.clone(), boxed.clone());
            self.routes
                .entry(method.clone())
                .or_default()
                .insert(&new_path_index, boxed.clone())
                .unwrap();
            self.path_map
                .entry(method.clone())
                .or_default()
                .insert(new_path_index.clone(), boxed.clone());
        }
    }

    /// 将一个 Service 同时挂载到所有常用 HTTP 方法
    ///
    /// 同时也派生了若干单方法版本（如 get_service 等）
    pub fn service(&mut self, path: &str, svc: HttpSvc<Req>) {
        let methods = [
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::PATCH,
            Method::HEAD,
            Method::OPTIONS,
        ];
        for method in methods {
            self.routes
                .entry(method.clone())
                .or_default()
                .insert(encode_route(path), svc.clone())
                .unwrap();
            self.path_map
                .entry(method.clone())
                .or_default()
                .insert(path.to_string(), svc.clone());
        }
    }

    /// 追加一个中间件 Layer，稍后在 into_tower_service 时顺序应用
    pub fn with_layer<L, B>(&mut self, layer: L) -> &mut Self
    where
        L: Layer<HttpSvc<Req>> + Send + Sync + 'static,
        L::Service: Service<Req, Response = Response<B>> + Clone + Send + 'static,
        <L::Service as Service<Req>>::Error: Into<AppError> + Send + Sync + 'static,
        <L::Service as Service<Req>>::Future: Send + 'static,
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        self.layers.push(Arc::new(move |svc: HttpSvc<Req>| {
            let wrapped = layer.layer(svc);
            let standardized = tower::ServiceBuilder::new()
                .map_response(|resp: Response<B>| {
                    let (parts, body) = resp.into_parts();
                    let body = body.map_err(|e| MikoError::from(e.into())).boxed_unsync();
                    Response::from_parts(parts, body)
                })
                .map_err(Into::into)
                .service(wrapped);
            BoxCloneService::new(standardized)
        }));
        self
    }

    /// 开启捕获panic
    #[cfg(feature = "catch_panic")]
    pub fn with_catch_panic(&mut self) -> &mut Self {
        use tower_http::catch_panic::CatchPanicLayer;
        self.with_layer(CatchPanicLayer::custom(|payload| {
            AppError::from_panic(payload).into_response()
        }))
    }

    /// 将路由器转换为 Tower Service，自动应用之前注册的 Layer
    pub fn into_tower_service(self) -> HttpSvc<Req> {
        self.into_tower_service_with_container(crate::dependency_container::default_container())
    }

    /// 使用指定依赖容器转换为 Tower Service。
    pub fn into_tower_service_with_container(
        mut self,
        container: Arc<crate::dependency_container::LazyDependencyContainer>,
    ) -> HttpSvc<Req> {
        let layers = std::mem::take(&mut self.layers);
        let router_svc = RouterSvc { router: self };
        let mut svc: HttpSvc<Req> = BoxCloneService::new(router_svc);
        for apply in layers {
            svc = apply(svc);
        }
        crate::dependency_container::with_dependency_scope(svc, container)
    }

    /// 从可变借用中取出所有权，便于在构建链路中重组 Router
    pub fn take(&mut self) -> Self {
        std::mem::replace(
            self,
            Router {
                routes: HashMap::new(),
                state: self.state.clone(),
                layers: Vec::new(),
                path_map: HashMap::new(),
            },
        )
    }
}

#[cfg(feature = "test")]
impl<S: Send + Sync + 'static> Router<S> {
    /// 创建一个测试客户端，用于发送请求并断言响应
    pub fn test_client(self) -> crate::test::test_client::TestClient {
        crate::test::test_client::TestClient::new(self.into_tower_service())
    }
}

#[cfg(feature = "ext")]
impl<S: Send + Sync + 'static> Router<S> {
    /// 简易的静态文件服务
    pub fn static_svc<F>(
        &mut self,
        prefix: &str,
        root: impl Into<PathBuf>,
        option_closure: Option<F>,
    ) where
        F: FnOnce(StaticSvcBuilder) -> StaticSvcBuilder,
    {
        let builder = StaticSvcBuilder::new(root);
        let builder = if let Some(option_closure) = option_closure {
            option_closure(builder)
        } else {
            builder
        };
        self.nest_service(prefix, builder.build())
    }

    /// 允许任意跨域（permissive），适合开发或简单场景
    pub fn cors_any(&mut self) {
        use tower_http::cors::CorsLayer;
        self.with_layer(CorsLayer::permissive());
    }
}
