use std::{
    collections::HashMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use miko::{
    endpoint::layer::WithState,
    ext::static_svc::StaticSvcBuilder,
    extractor::{Form, Json, Path, Query, State, multipart::MultipartResult},
    handler::{Req, Resp},
    http::response::sse::{SseSender, spawn_sse_event},
    macros::*,
    openapi::AutoPaths,
    router::Router,
    ws::server::{IntoMessage, spawn_ws_event},
    *,
};
use serde::Deserialize;
use tokio::sync::Mutex;

#[derive(OpenApi)]
#[openapi(
    info(title = "Miko Basic Example API", version = "1.0.0"),
    modifiers(&AutoPaths)
)]
struct ApiDoc;

#[derive(Deserialize)]
struct MyQuery {
    name: String,
    age: u8,
}

#[get("/")]
async fn hello_world() -> &'static str {
    "Hello, World! (macro defined route)"
}

#[get("/with_query")]
async fn hello_with_query(#[query] name: String, #[query] age: u8) -> String {
    format!(
        r"You can also use Query extractor!
    But #[query] is more convenient if you don't want to define a Query struct.
    Hello, {}! You are {} years old. (macro defined route)",
        name, age
    )
}

#[get("/with_path/{a}/{b}")]
async fn hello_with_path(#[path] a: String, Path(b): Path<i32>) -> String {
    format!(
        r"Hello from path parameters!
    #[path] and Path<T> has the same behavior.
    They will extract the value from the path and convert it to the specified type.
    But they are not named parameters, so the order matters.
    a: {}, b: {} (macro defined route)",
        a, b
    )
}

#[post("/echo", method = "get,put")] // Multiple methods are supported, it will be get,post,put
async fn echo(
    body: String, // There can only be one body extractor (or more precisely, only one extractor that implements FromRequest)
) -> String {
    format!("Echo: {}", body)
}

#[get("/json_resp")]
async fn json_resp() {
    let mut map = HashMap::new();
    map.insert("value1", 42);
    map.insert("value2", 100);
    Json(map) // Json<T> will be converted to application/json response
}

#[post("/json_req")]
async fn json_req(
    // #[body] is alias of #[body(json)], it will extract application/json request body and deserialize it to the specified type
    #[body] data: HashMap<String, i32>, // Json<T> can also be used as extractor for application/json request
) -> String {
    format!("Received JSON data: {:?}", data)
}

#[prefix("/sub")] // use `mod` to define sub routes
mod sub_routes {
    use super::*;
    // import necessary items from the parent module

    #[get("/hello")]
    async fn sub_hello() -> &'static str {
        "Hello from sub route!"
    }
}

struct ServiceComponent {
    pub name: String,
    pub version: String,
    pub data: Mutex<String>,
}
// because #[dep] is the only way to inject dependencies, so components must be defined in route that defined by macros
#[component] // define a singleton component
impl ServiceComponent {
    // only other component can be arguments of new()
    // new must be an async function
    async fn new() -> Self {
        ServiceComponent {
            name: "demo".into(),
            version: "1.0.0".into(),
            data: Mutex::new("Initial service data".into()), // Because #[dep] must be Arc<T>, so Mutex<T> is preferred for mutable data
        }
    }
    async fn operation(&self) {
        println!("ServiceComponent operation called.");
        println!("Name: {}, Version: {}", self.name, self.version);
        println!("Data: {}", self.data.lock().await);
    }
    async fn changed(&self) {
        let mut data = self.data.lock().await;
        *data = "Service data has been changed.".into();
        println!("ServiceComponent has been changed.");
    }
}

#[get("/use_dep")]
async fn use_dep(
    // muse be Arc<T>
    #[dep] service: Arc<ServiceComponent>, // inject the ServiceComponent dependency
) {
    service.operation().await;
    service.changed().await;
    service.operation().await;
    service.data.lock().await.clone()
}

#[get("/data")]
async fn get_data(#[dep] service: Arc<ServiceComponent>) -> String {
    service.data.lock().await.clone() // you can request the data to examine whether component is singleton
}

#[get("/error")]
async fn error() {
    AppError::from(tokio::io::Error::other("HAHA"))
}

#[get("/custom_error")]
async fn custom_error() {
    AppError::BadGateway("Custom Bad Gateway".into())
}

#[get("/panic")]
async fn panic() -> &'static str {
    panic!("This handler panics!");
}

#[get("/sse")]
async fn sse() {
    // you can write no return type for handlers if you are using macros, the return type will be impl IntoResponse
    // SSE example
    spawn_sse_event(|sender| async move {
        tokio::spawn(async move {
            for i in 0..5 {
                sender
                    .send(format!("data: SSE event number {}\n\n", i))
                    .await
                    .or_break();
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
    })
}

#[get("/sse2")]
async fn sse2() {
    // you can even just return a closure
    |sender: SseSender| async move {
        tokio::spawn(async move {
            for i in 0..5 {
                sender
                    .send(format!("data: SSE2 event number {}\n\n", i))
                    .await
                    .or_break();
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
    }
}

#[get("/ws")]
async fn ws(mut req: Req) {
    // usually you need to pass Req to spawn_ws_event
    spawn_ws_event(
        // Sadly, you still need to call spawn_ws_event, not like sse (this is because websocket needs to get Req and upgrade the connection)
        |mut io| async move {
            io.send("hello world").await.expect("websocket send error");
            let (mut w, mut r, _) = io.split();
            {
                let mut w = w.clone();
                tokio::spawn(async move {
                    w.send("START --".into_message())
                        .await
                        .expect("websocket send error");
                    loop {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs();
                        let msg = format!("server time: {}", now);
                        let _ = w.send(msg.into_message()).await;
                    }
                });
            }
            tokio::spawn(async move {
                while let Some(msg) = r.next().await {
                    let msg = msg.expect("websocket recv error");
                    if msg.is_text() {
                        let txt = msg.into_text().expect("websocket into text error");
                        let _ = w.send(txt.into_message()).await;
                        println!("recv text: {}", txt);
                    } else if msg.is_binary() {
                        let bin = msg.into_data();
                        println!("recv binary: {:?}", bin);
                    } else if msg.is_close() {
                        println!("websocket closed");
                        break;
                    }
                }
            });
        },
        &mut req,
        None,
    )
    .expect("failed to spawn websocket handler")
}

#[get("/layer")]
#[layer(AddHeaderLayer::new("X-Route-Layer", "Applied"))]
async fn layer_test() -> String {
    "Test route layer - check response headers for X-Route-Layer".to_string()
}

#[prefix("/layered")]
#[layer(AddHeaderLayer::new("X-Module-Layer", "Applied"))]
mod layered_module {
    use super::*;

    #[get("/test1")]
    #[layer(AddHeaderLayer::new("X-Custom-Header", "Layer-Applied"))]
    async fn test_single_layer() -> String {
        "Test single layer - check response headers for X-Custom-Header".to_string()
    }

    #[prefix("/inner")]
    #[layer(AddHeaderLayer::new("X-Inner-Layer", "Inner-Applied"))]
    mod inner {
        use super::*;

        #[get("/test_inner")]
        #[layer(AddHeaderLayer::new("X-Route-INNER-Layer", "Inner-Applied"))]
        async fn test_inner_layer() -> String {
            "Test inner module layer - check response headers for X-Inner-Layer".to_string()
        }
    }
}

#[post("/multipart")]
async fn multipart(multipart: MultipartResult) {
    format!(
        "Received multipart data: {:?}\n Files: {:?}",
        multipart.fields, multipart.files
    )
}

#[derive(Deserialize, Debug, ToSchema)]
#[allow(unused)]
struct FormStruct {
    field1: String,
    field2: i32,
}

#[post("/form")]
async fn form(Form(form_data): Form<FormStruct>) {
    format!("Received form data: {:?}", form_data)
}

struct NewResp();
impl IntoResponse for NewResp {
    fn into_response(self) -> Resp {
        "Custom Response, or you can also use Response::builder".into_response()
    }
}
#[get("/new_resp")]
async fn new_resp() -> NewResp {
    NewResp()
}

struct AppState {
    pub app_name: String,
    pub app_version: String,
}

struct AnotherAppState {
    pub description: String,
}

#[miko(sse, catch, build)]
// the sse attribute can set a panic hook that ignore error caused by `or_break()`
// the catch attribute can catch panics in handlers and convert them to 500 responses
pub async fn create_app() {
    let mut no_macro_router = Router::new();
    no_macro_router.get("/", async move || "Hello, World! (manually defined router)");
    no_macro_router.get(
        "/with_query",
        async move |Query(queries): Query<MyQuery>| {
            format!(
                "Hello, {}! You are {} years old. (manually defined router)",
                queries.name, queries.age
            )
        },
    );

    let mut router = router.with_state(AppState {
        app_name: "Miko Demo App".into(),
        app_version: "1.0.0".into(),
    });
    router.get("/with_state", async move |State(state): State<AppState>| {
        format!(
            "App Name: {}, App Version: {} (macro defined route with state)",
            state.app_name, state.app_version
        )
    });
    // noticed that State can only used by non macro defined routes
    // because the state is determined by the current state when route function(like `get`) is called;

    router.get_service(
        "/single_state",
        (async move |State(state): State<AnotherAppState>| {
            format!(
                "Description: {} (macro defined route with state)",
                state.description
            )
        })
        .with_state(AnotherAppState {
            description: "Another".into(),
        }),
    );
    // so you can have different state for different routes
    // but only one
    // and noticed that, the handler become a service when using with_state, you you need to use get_service instead of get

    router.nest("/no_macro", no_macro_router);

    // Register OpenAPI route
    // Macro-based routes can be added to the OpenAPI doc by using modifiers
    router.get("/api-docs/openapi.json", || async {
        Json(ApiDoc::openapi())
    });

    router.static_svc(
        "/static",
        "./static",
        Some(|options: StaticSvcBuilder| {
            options
                .cors_any()
                .with_spa_fallback(true)
                .with_fallback_files(["index.html", "index.htm", "index.php"])
        }),
    ); // static file service with CORS enabled and SPA fallback

    router.cors_any(); // convienient method to enable CORS for all origins
}

#[tokio::main]
#[allow(dead_code)]
async fn main() {
    tracing_subscriber::fmt::init(); // initialize logging (optional)
    let app = create_app().await;
    app.run().await.unwrap();
}

#[derive(Clone)]
struct AddHeaderLayer {
    header_name: &'static str,
    header_value: &'static str,
}

impl AddHeaderLayer {
    fn new(header_name: &'static str, header_value: &'static str) -> Self {
        Self {
            header_name,
            header_value,
        }
    }
}

impl<S> tower::Layer<S> for AddHeaderLayer {
    type Service = AddHeaderService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AddHeaderService {
            inner,
            header_name: self.header_name,
            header_value: self.header_value,
        }
    }
}

#[derive(Clone)]
struct AddHeaderService<S> {
    inner: S,
    header_name: &'static str,
    header_value: &'static str,
}

impl<S> tower::Service<miko_core::Req> for AddHeaderService<S>
where
    S: tower::Service<miko_core::Req, Response = miko_core::Resp> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: miko_core::Req) -> Self::Future {
        let mut inner = self.inner.clone();
        let header_name = self.header_name;
        let header_value = self.header_value;

        Box::pin(async move {
            let mut resp = inner.call(req).await?;
            resp.headers_mut()
                .insert(header_name, header_value.parse().unwrap());
            Ok(resp)
        })
    }
}
