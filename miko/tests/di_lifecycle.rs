use hyper::StatusCode;
use miko::app::Application;
use miko::dependency_container::{LazyDependencyContainer, RequestScope, ResolveError};
use miko::macros::*;
use miko::router::Router;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

static TRANSIENT_CONSTRUCTS: AtomicUsize = AtomicUsize::new(0);
static SINGLETON_CONSTRUCTS: AtomicUsize = AtomicUsize::new(0);
static REQUEST_CONSTRUCTS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, PartialEq, Eq)]
struct TransientProbe {
    id: usize,
}

#[component(transient)]
impl TransientProbe {
    async fn new() -> Self {
        let id = TRANSIENT_CONSTRUCTS.fetch_add(1, Ordering::SeqCst) + 1;
        Self { id }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SingletonProbe {
    id: usize,
}

#[component]
impl SingletonProbe {
    async fn new() -> Self {
        let id = SINGLETON_CONSTRUCTS.fetch_add(1, Ordering::SeqCst) + 1;
        Self { id }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct RequestProbe {
    id: usize,
}

#[component(request)]
impl RequestProbe {
    async fn new() -> Self {
        let id = REQUEST_CONSTRUCTS.fetch_add(1, Ordering::SeqCst) + 1;
        Self { id }
    }
}

struct RequestConsumer {
    probe: Arc<RequestProbe>,
}

#[component(request)]
impl RequestConsumer {
    async fn new(probe: Arc<RequestProbe>) -> Self {
        Self { probe }
    }
}

struct OwnedTransientConsumer {
    probe_id: usize,
}

#[component(transient)]
impl OwnedTransientConsumer {
    async fn new(probe: TransientProbe) -> Self {
        Self { probe_id: probe.id }
    }
}

struct SingletonCaptive;

#[component]
impl SingletonCaptive {
    async fn new(_probe: Arc<RequestProbe>) -> Self {
        Self
    }
}

struct TransientRequestBridge;

#[component(transient)]
impl TransientRequestBridge {
    async fn new(_probe: Arc<RequestProbe>) -> Self {
        Self
    }
}

struct IndirectSingletonCaptive;

#[component]
impl IndirectSingletonCaptive {
    async fn new(_bridge: Arc<TransientRequestBridge>) -> Self {
        Self
    }
}

struct CycleA;
struct CycleB;

#[component(transient)]
impl CycleA {
    async fn new(_dependency: Arc<CycleB>) -> Self {
        Self
    }
}

#[component(transient)]
impl CycleB {
    async fn new(_dependency: Arc<CycleA>) -> Self {
        Self
    }
}

struct MissingProbe;

#[get("/request-scope")]
async fn request_scope_handler(
    #[dep] first: Arc<RequestProbe>,
    #[dep] second: Arc<RequestProbe>,
    #[dep] consumer: Arc<RequestConsumer>,
) -> String {
    format!(
        "{}:{}:{}",
        first.id,
        Arc::ptr_eq(&first, &second),
        Arc::ptr_eq(&first, &consumer.probe)
    )
}

#[get("/missing-dependency")]
async fn missing_dependency_handler(#[dep] _missing: Arc<MissingProbe>) {}

#[get("/owned-transient")]
async fn owned_transient_handler(
    #[dep] first: TransientProbe,
    #[dep] second: TransientProbe,
) -> String {
    format!("{}:{}", first.id, second.id)
}

#[middleware]
async fn request_scope_middleware(
    #[dep] probe: Arc<RequestProbe>,
) -> miko::AppResult<miko::handler::Resp> {
    let probe_id = probe.id.to_string();
    let mut response = _next.run(_req).await?;
    response.headers_mut().insert(
        "x-request-probe-id",
        probe_id.parse().expect("probe id is a valid header value"),
    );
    Ok(response)
}

#[tokio::test]
async fn component_lifetimes_behave_as_configured() {
    TRANSIENT_CONSTRUCTS.store(0, Ordering::SeqCst);
    SINGLETON_CONSTRUCTS.store(0, Ordering::SeqCst);
    REQUEST_CONSTRUCTS.store(0, Ordering::SeqCst);

    let container = LazyDependencyContainer::new_();

    let transient_a = container.get::<TransientProbe>().await;
    let transient_b = container.get::<TransientProbe>().await;
    assert_ne!(Arc::as_ptr(&transient_a), Arc::as_ptr(&transient_b));
    assert_eq!(TRANSIENT_CONSTRUCTS.load(Ordering::SeqCst), 2);

    let owned_consumer = RequestScope::new(container.clone())
        .resolve_owned::<OwnedTransientConsumer>()
        .await
        .unwrap();
    assert!(owned_consumer.probe_id > 0);

    let singleton_a = container.get::<SingletonProbe>().await;
    let singleton_b = container.get::<SingletonProbe>().await;
    assert_eq!(Arc::as_ptr(&singleton_a), Arc::as_ptr(&singleton_b));
    assert_eq!(SINGLETON_CONSTRUCTS.load(Ordering::SeqCst), 1);

    let first_scope = RequestScope::new(container.clone());
    let request_a = first_scope.resolve::<RequestProbe>().await.unwrap();
    let request_b = first_scope.resolve::<RequestProbe>().await.unwrap();
    let consumer = first_scope.resolve::<RequestConsumer>().await.unwrap();
    assert!(Arc::ptr_eq(&request_a, &request_b));
    assert!(Arc::ptr_eq(&request_a, &consumer.probe));

    let second_scope = RequestScope::new(container.clone());
    let request_c = second_scope.resolve::<RequestProbe>().await.unwrap();
    assert!(!Arc::ptr_eq(&request_a, &request_c));

    let concurrent_scope = RequestScope::new(container.clone());
    let (concurrent_a, concurrent_b) = tokio::join!(
        concurrent_scope.resolve::<RequestProbe>(),
        concurrent_scope.resolve::<RequestProbe>()
    );
    assert!(Arc::ptr_eq(&concurrent_a.unwrap(), &concurrent_b.unwrap()));

    assert!(matches!(
        container.try_get::<RequestProbe>().await,
        Err(ResolveError::MissingRequestScope { .. })
    ));
    assert!(matches!(
        first_scope.resolve_owned::<SingletonProbe>().await,
        Err(ResolveError::OwnedRequiresTransient { .. })
    ));
    assert!(matches!(
        first_scope.resolve_owned::<RequestProbe>().await,
        Err(ResolveError::OwnedRequiresTransient { .. })
    ));
    assert!(matches!(
        first_scope.resolve::<SingletonCaptive>().await,
        Err(ResolveError::LifetimeViolation { .. })
    ));
    assert!(matches!(
        first_scope.resolve::<IndirectSingletonCaptive>().await,
        Err(ResolveError::LifetimeViolation { .. })
    ));
    assert!(matches!(
        first_scope.resolve::<CycleA>().await,
        Err(ResolveError::CircularDependency { .. })
    ));

    let mut router = Router::new();
    router.get("/request-scope", request_scope_handler);
    router.get("/missing-dependency", missing_dependency_handler);
    router.get("/owned-transient", owned_transient_handler);
    router.with_layer(request_scope_middleware());
    let client = router.test_client();

    let first_response = client.get("/request-scope").send().await;
    first_response.assert_ok();
    let first_body = first_response.text();
    let first_id = first_body
        .split(':')
        .next()
        .expect("handler response contains an id");
    assert_eq!(first_response.headers()["x-request-probe-id"], first_id);
    assert!(first_body.ends_with(":true:true"));

    let second_response = client.get("/request-scope").send().await;
    second_response.assert_ok();
    let second_body = second_response.text();
    let second_id = second_body
        .split(':')
        .next()
        .expect("handler response contains an id");
    assert_eq!(second_response.headers()["x-request-probe-id"], second_id);
    assert_ne!(first_id, second_id);

    client
        .get("/missing-dependency")
        .send()
        .await
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);

    let owned_response = client.get("/owned-transient").send().await;
    owned_response.assert_ok();
    let owned_body = owned_response.text();
    let mut ids = owned_body.split(':');
    assert_ne!(ids.next(), ids.next());
}

#[test]
fn applications_own_independent_dependency_containers() {
    let first = Application::new_(Router::new());
    let second = Application::new_(Router::new());

    assert!(!Arc::ptr_eq(first.dependencies(), second.dependencies()));
}
