use crate::error::AppError;
use crate::extractor::from_request::{FRPFut, FromRequestParts};
use crate::handler::{Req, Resp};
use crate::router::HttpSvc;
use hyper::http::request::Parts;
use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::{Mutex, OnceCell};
use tower::Service;
use tower::util::BoxCloneService;

pub type DependencyValue = Box<dyn Any + Send + Sync>;
pub type SharedDependencyInstance = Arc<dyn Any + Send + Sync>;
pub type DependencyInitResult = Result<DependencyValue, ResolveError>;
pub type DependencyInstanceFuture =
    Pin<Box<dyn Future<Output = DependencyInitResult> + Send + 'static>>;
pub type DependencyFactory = fn(ResolveContext) -> DependencyInstanceFuture;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DependencyLifetime {
    Singleton,
    Request,
    Transient,
}

#[derive(Clone, Copy, Debug, Eq)]
struct ServiceKey {
    type_id: TypeId,
    name: &'static str,
}

impl ServiceKey {
    fn of<T: 'static>(name: &'static str) -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            name,
        }
    }
}

impl PartialEq for ServiceKey {
    fn eq(&self, other: &Self) -> bool {
        self.type_id == other.type_id && self.name == other.name
    }
}

impl Hash for ServiceKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.type_id.hash(state);
        self.name.hash(state);
    }
}

#[derive(Clone, Debug)]
struct ResolveFrame {
    key: ServiceKey,
    type_name: &'static str,
    lifetime: DependencyLifetime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveError {
    NotRegistered {
        type_name: &'static str,
        name: &'static str,
    },
    TypeMismatch {
        type_name: &'static str,
    },
    MissingRequestScope {
        type_name: &'static str,
    },
    RequestScopeNotInstalled,
    OwnedRequiresTransient {
        type_name: &'static str,
        lifetime: DependencyLifetime,
    },
    CircularDependency {
        chain: Vec<&'static str>,
    },
    LifetimeViolation {
        owner: &'static str,
        dependency: &'static str,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRegistered { type_name, name } => {
                write!(
                    f,
                    "dependency `{type_name}` with name `{name}` is not registered"
                )
            }
            Self::TypeMismatch { type_name } => {
                write!(f, "resolved dependency does not match `{type_name}`")
            }
            Self::MissingRequestScope { type_name } => {
                write!(
                    f,
                    "request-scoped dependency `{type_name}` was resolved outside a request"
                )
            }
            Self::RequestScopeNotInstalled => {
                write!(f, "dependency request scope has not been installed")
            }
            Self::OwnedRequiresTransient {
                type_name,
                lifetime,
            } => {
                write!(
                    f,
                    "owned dependency `{type_name}` requires transient lifetime, found {lifetime:?}"
                )
            }
            Self::CircularDependency { chain } => {
                write!(f, "circular dependency detected: {}", chain.join(" -> "))
            }
            Self::LifetimeViolation { owner, dependency } => {
                write!(
                    f,
                    "singleton dependency `{owner}` cannot depend on request-scoped `{dependency}`"
                )
            }
        }
    }
}

impl std::error::Error for ResolveError {}

impl From<ResolveError> for AppError {
    fn from(error: ResolveError) -> Self {
        tracing::error!(error = %error, "dependency resolution failed");
        Self::InternalServerError("Dependency resolution failed".to_string())
    }
}

pub struct DependencyDefFn(pub fn() -> DependencyDef);

pub struct DependencyDef {
    pub type_id: TypeId,
    pub type_name: &'static str,
    pub prewarm: bool,
    pub name: &'static str,
    pub init_fn: DependencyFactory,
    pub lifetime: DependencyLifetime,
}

#[cfg(feature = "auto")]
inventory::collect!(DependencyDefFn);

#[derive(Clone)]
pub struct DependencyEntry {
    factory: DependencyFactory,
    type_name: &'static str,
    lifetime: DependencyLifetime,
    prewarm: bool,
    instance: Option<Arc<OnceCell<SharedDependencyInstance>>>,
}

impl DependencyEntry {
    fn new(
        factory: DependencyFactory,
        type_name: &'static str,
        lifetime: DependencyLifetime,
        prewarm: bool,
    ) -> Self {
        let instance = if lifetime == DependencyLifetime::Singleton {
            Some(Arc::new(OnceCell::new()))
        } else {
            None
        };
        Self {
            factory,
            type_name,
            lifetime,
            prewarm,
            instance,
        }
    }
}

pub struct LazyDependencyContainer {
    registry: HashMap<ServiceKey, DependencyEntry>,
}

impl LazyDependencyContainer {
    pub fn new() -> Self {
        Self {
            registry: HashMap::new(),
        }
    }

    #[cfg(feature = "auto")]
    pub fn new_() -> Arc<Self> {
        let mut container = Self::new();
        for dependency in inventory::iter::<DependencyDefFn> {
            let dependency = dependency.0();
            container.registry.insert(
                ServiceKey {
                    type_id: dependency.type_id,
                    name: dependency.name,
                },
                DependencyEntry::new(
                    dependency.init_fn,
                    dependency.type_name,
                    dependency.lifetime,
                    dependency.prewarm,
                ),
            );
        }
        Arc::new(container)
    }

    fn insert_entry<T: 'static + Send + Sync>(
        &mut self,
        name: &'static str,
        prewarm: bool,
        lifetime: DependencyLifetime,
        factory: DependencyFactory,
    ) {
        self.registry.insert(
            ServiceKey::of::<T>(name),
            DependencyEntry::new(factory, type_name::<T>(), lifetime, prewarm),
        );
    }

    pub fn register_with_lifetime_<T: 'static + Send + Sync>(
        &mut self,
        name: &'static str,
        prewarm: bool,
        lifetime: DependencyLifetime,
        factory: DependencyFactory,
    ) {
        self.insert_entry::<T>(name, prewarm, lifetime, factory);
    }

    pub fn register_with_lifetime<T: 'static + Send + Sync>(
        &mut self,
        prewarm: bool,
        lifetime: DependencyLifetime,
        factory: DependencyFactory,
    ) {
        self.insert_entry::<T>("___", prewarm, lifetime, factory);
    }

    pub fn register_<T: 'static + Send + Sync>(
        &mut self,
        name: &'static str,
        prewarm: bool,
        factory: DependencyFactory,
    ) {
        self.register_with_lifetime_::<T>(name, prewarm, DependencyLifetime::Singleton, factory);
    }

    pub fn register<T: 'static + Send + Sync>(
        &mut self,
        prewarm: bool,
        factory: DependencyFactory,
    ) {
        self.register_with_lifetime::<T>(prewarm, DependencyLifetime::Singleton, factory);
    }

    pub async fn try_get_<T: 'static + Send + Sync>(
        self: &Arc<Self>,
        name: &'static str,
    ) -> Result<Arc<T>, ResolveError> {
        ResolveContext::root(self.clone(), None)
            .resolve_named::<T>(name)
            .await
    }

    pub async fn try_get<T: 'static + Send + Sync>(
        self: &Arc<Self>,
    ) -> Result<Arc<T>, ResolveError> {
        self.try_get_::<T>("___").await
    }

    /// Panics if dependency resolution fails; prefer `try_get_` for fallible resolution.
    pub async fn get_<T: 'static + Send + Sync>(self: &Arc<Self>, name: &'static str) -> Arc<T> {
        self.try_get_::<T>(name)
            .await
            .expect("dependency resolution failed")
    }

    /// Panics if dependency resolution fails; prefer `try_get` for fallible resolution.
    pub async fn get<T: 'static + Send + Sync>(self: &Arc<Self>) -> Arc<T> {
        self.get_::<T>("___").await
    }

    async fn resolve_typed<T: 'static + Send + Sync>(
        self: &Arc<Self>,
        name: &'static str,
        context: ResolveContext,
    ) -> Result<Arc<T>, ResolveError> {
        let instance = self
            .resolve_shared_entry(ServiceKey::of::<T>(name), context)
            .await?;
        Arc::downcast::<T>(instance).map_err(|_| ResolveError::TypeMismatch {
            type_name: type_name::<T>(),
        })
    }

    async fn resolve_typed_owned<T: 'static + Send + Sync>(
        self: &Arc<Self>,
        name: &'static str,
        context: ResolveContext,
    ) -> Result<T, ResolveError> {
        let value = self
            .resolve_owned_entry(ServiceKey::of::<T>(name), context)
            .await?;
        value
            .downcast::<T>()
            .map(|value| *value)
            .map_err(|_| ResolveError::TypeMismatch {
                type_name: type_name::<T>(),
            })
    }

    fn prepare_resolution(
        &self,
        key: ServiceKey,
        context: ResolveContext,
    ) -> Result<(&DependencyEntry, ResolveContext), ResolveError> {
        let entry = self.registry.get(&key).ok_or(ResolveError::NotRegistered {
            type_name: context.requested_type_name,
            name: key.name,
        })?;

        if entry.lifetime == DependencyLifetime::Request
            && let Some(owner) = context
                .stack
                .iter()
                .rev()
                .find(|frame| frame.lifetime == DependencyLifetime::Singleton)
        {
            return Err(ResolveError::LifetimeViolation {
                owner: owner.type_name,
                dependency: entry.type_name,
            });
        }

        if let Some(cycle_start) = context.stack.iter().position(|frame| frame.key == key) {
            let mut chain = context.stack[cycle_start..]
                .iter()
                .map(|frame| frame.type_name)
                .collect::<Vec<_>>();
            chain.push(entry.type_name);
            return Err(ResolveError::CircularDependency { chain });
        }

        let child_context = context.push(key, entry.type_name, entry.lifetime);
        Ok((entry, child_context))
    }

    async fn resolve_shared_entry(
        self: &Arc<Self>,
        key: ServiceKey,
        context: ResolveContext,
    ) -> Result<SharedDependencyInstance, ResolveError> {
        let (entry, child_context) = self.prepare_resolution(key, context)?;
        match entry.lifetime {
            DependencyLifetime::Singleton => {
                let cell = entry
                    .instance
                    .as_ref()
                    .expect("singleton entries always have an instance cell");
                let factory = entry.factory;
                let instance = cell
                    .get_or_try_init(|| async move {
                        let value = factory(child_context).await?;
                        Ok::<SharedDependencyInstance, ResolveError>(Arc::from(value))
                    })
                    .await?;
                Ok(instance.clone())
            }
            DependencyLifetime::Request => {
                let scope = child_context.request_scope.clone().ok_or(
                    ResolveError::MissingRequestScope {
                        type_name: entry.type_name,
                    },
                )?;
                let cell = scope.instance_cell(key).await;
                let factory = entry.factory;
                let instance = cell
                    .get_or_try_init(|| async move {
                        let value = factory(child_context).await?;
                        Ok::<SharedDependencyInstance, ResolveError>(Arc::from(value))
                    })
                    .await?;
                Ok(instance.clone())
            }
            DependencyLifetime::Transient => {
                let value = (entry.factory)(child_context).await?;
                Ok(Arc::from(value))
            }
        }
    }

    async fn resolve_owned_entry(
        self: &Arc<Self>,
        key: ServiceKey,
        context: ResolveContext,
    ) -> DependencyInitResult {
        let (entry, child_context) = self.prepare_resolution(key, context)?;
        if entry.lifetime != DependencyLifetime::Transient {
            return Err(ResolveError::OwnedRequiresTransient {
                type_name: entry.type_name,
                lifetime: entry.lifetime,
            });
        }
        (entry.factory)(child_context).await
    }

    pub async fn prewarm_all(self: &Arc<Self>) -> Result<(), ResolveError> {
        for (key, entry) in &self.registry {
            if entry.prewarm && entry.lifetime == DependencyLifetime::Singleton {
                let context = ResolveContext::root(self.clone(), None)
                    .with_requested_type_name(entry.type_name);
                self.resolve_shared_entry(*key, context).await?;
            }
        }
        Ok(())
    }
}

impl Default for LazyDependencyContainer {
    fn default() -> Self {
        Self::new()
    }
}

pub fn default_container() -> Arc<LazyDependencyContainer> {
    #[cfg(feature = "auto")]
    {
        LazyDependencyContainer::new_()
    }
    #[cfg(not(feature = "auto"))]
    {
        Arc::new(LazyDependencyContainer::new())
    }
}

#[derive(Clone)]
pub struct RequestScope {
    container: Arc<LazyDependencyContainer>,
    instances: Arc<Mutex<HashMap<ServiceKey, Arc<OnceCell<SharedDependencyInstance>>>>>,
}

impl RequestScope {
    pub fn new(container: Arc<LazyDependencyContainer>) -> Self {
        Self {
            container,
            instances: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn instance_cell(&self, key: ServiceKey) -> Arc<OnceCell<SharedDependencyInstance>> {
        let mut instances = self.instances.lock().await;
        instances
            .entry(key)
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone()
    }

    pub async fn resolve<T: 'static + Send + Sync>(&self) -> Result<Arc<T>, ResolveError> {
        ResolveContext::root(self.container.clone(), Some(self.clone()))
            .resolve::<T>()
            .await
    }

    pub async fn resolve_named<T: 'static + Send + Sync>(
        &self,
        name: &'static str,
    ) -> Result<Arc<T>, ResolveError> {
        ResolveContext::root(self.container.clone(), Some(self.clone()))
            .resolve_named::<T>(name)
            .await
    }

    pub async fn resolve_owned<T: 'static + Send + Sync>(&self) -> Result<T, ResolveError> {
        ResolveContext::root(self.container.clone(), Some(self.clone()))
            .resolve_owned::<T>()
            .await
    }

    pub async fn resolve_owned_named<T: 'static + Send + Sync>(
        &self,
        name: &'static str,
    ) -> Result<T, ResolveError> {
        ResolveContext::root(self.container.clone(), Some(self.clone()))
            .resolve_owned_named::<T>(name)
            .await
    }

    fn from_parts(parts: &Parts) -> Result<Self, ResolveError> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(ResolveError::RequestScopeNotInstalled)
    }

    fn from_request(req: &Req) -> Result<Self, ResolveError> {
        req.extensions()
            .get::<Self>()
            .cloned()
            .ok_or(ResolveError::RequestScopeNotInstalled)
    }
}

#[derive(Clone)]
pub struct ResolveContext {
    container: Arc<LazyDependencyContainer>,
    request_scope: Option<RequestScope>,
    stack: Vec<ResolveFrame>,
    requested_type_name: &'static str,
}

impl ResolveContext {
    fn root(container: Arc<LazyDependencyContainer>, request_scope: Option<RequestScope>) -> Self {
        Self {
            container,
            request_scope,
            stack: Vec::new(),
            requested_type_name: "<unknown>",
        }
    }

    fn with_requested_type_name(mut self, requested_type_name: &'static str) -> Self {
        self.requested_type_name = requested_type_name;
        self
    }

    fn push(&self, key: ServiceKey, type_name: &'static str, lifetime: DependencyLifetime) -> Self {
        let mut child = self.clone();
        child.stack.push(ResolveFrame {
            key,
            type_name,
            lifetime,
        });
        child.requested_type_name = type_name;
        child
    }

    pub async fn resolve<T: 'static + Send + Sync>(&self) -> Result<Arc<T>, ResolveError> {
        self.resolve_named::<T>("___").await
    }

    pub async fn resolve_named<T: 'static + Send + Sync>(
        &self,
        name: &'static str,
    ) -> Result<Arc<T>, ResolveError> {
        self.container
            .resolve_typed::<T>(
                name,
                self.clone().with_requested_type_name(type_name::<T>()),
            )
            .await
    }

    pub async fn resolve_owned<T: 'static + Send + Sync>(&self) -> Result<T, ResolveError> {
        self.resolve_owned_named::<T>("___").await
    }

    pub async fn resolve_owned_named<T: 'static + Send + Sync>(
        &self,
        name: &'static str,
    ) -> Result<T, ResolveError> {
        self.container
            .resolve_typed_owned::<T>(
                name,
                self.clone().with_requested_type_name(type_name::<T>()),
            )
            .await
    }
}

pub struct Dep<T>(pub Arc<T>);
pub struct OwnedDep<T>(pub T);

impl<S, T> FromRequestParts<S> for Dep<T>
where
    S: Send + Sync + 'static,
    T: Send + Sync + 'static,
{
    fn from_request_parts(req: &mut Parts, _state: Arc<S>) -> FRPFut<'_, Self> {
        let scope = RequestScope::from_parts(req);
        Box::pin(async move {
            let scope = scope.map_err(AppError::from)?;
            scope.resolve::<T>().await.map(Self).map_err(AppError::from)
        })
    }
}

impl<S, T> FromRequestParts<S> for OwnedDep<T>
where
    S: Send + Sync + 'static,
    T: Send + Sync + 'static,
{
    fn from_request_parts(req: &mut Parts, _state: Arc<S>) -> FRPFut<'_, Self> {
        let scope = RequestScope::from_parts(req);
        Box::pin(async move {
            let scope = scope.map_err(AppError::from)?;
            scope
                .resolve_owned::<T>()
                .await
                .map(Self)
                .map_err(AppError::from)
        })
    }
}

pub fn resolve_from_request<T: 'static + Send + Sync>(
    req: &Req,
) -> Pin<Box<dyn Future<Output = Result<Arc<T>, AppError>> + Send>> {
    let scope = RequestScope::from_request(req);
    Box::pin(async move {
        let scope = scope.map_err(AppError::from)?;
        scope.resolve::<T>().await.map_err(AppError::from)
    })
}

pub fn resolve_owned_from_request<T: 'static + Send + Sync>(
    req: &Req,
) -> Pin<Box<dyn Future<Output = Result<T, AppError>> + Send>> {
    let scope = RequestScope::from_request(req);
    Box::pin(async move {
        let scope = scope.map_err(AppError::from)?;
        scope.resolve_owned::<T>().await.map_err(AppError::from)
    })
}

#[derive(Clone)]
struct DependencyScopeService {
    inner: HttpSvc<Req>,
    container: Arc<LazyDependencyContainer>,
}

impl Service<Req> for DependencyScopeService {
    type Response = Resp;
    type Error = AppError;
    type Future = Pin<Box<dyn Future<Output = Result<Resp, AppError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Req) -> Self::Future {
        if req.extensions().get::<RequestScope>().is_none() {
            req.extensions_mut()
                .insert(RequestScope::new(self.container.clone()));
        }
        let mut inner = self.inner.clone();
        Box::pin(async move { inner.call(req).await })
    }
}

pub fn with_dependency_scope(
    inner: HttpSvc<Req>,
    container: Arc<LazyDependencyContainer>,
) -> HttpSvc<Req> {
    BoxCloneService::new(DependencyScopeService { inner, container })
}
