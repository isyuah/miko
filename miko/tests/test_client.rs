use hyper::StatusCode;
use miko::extractor::Path;
use miko::macros::*;
use miko::router::Router;
use serde_json::json;

struct TestService {
    ran: i32,
}
#[component]
impl TestService {
    pub async fn new() -> Self {
        Self { ran: 42 }
    }
}

#[get("/macro")]
async fn macro_route(#[dep] svc: Arc<TestService>) {
    format!("Svc: {}", svc.ran)
}

#[tokio::test]
async fn test_test_client() {
    let mut router = Router::new();
    router.get("/hello", || async move { "world" });
    router.post("/echo", |req: String| async move { req });
    router.post("/path/{id}", |Path(pa): Path<String>| async move {
        format!("/path/{}", pa)
    });
    router.get("/macro", macro_route);
    let client = router.test_client();
    let r1 = client.get("/hello").send().await;
    r1.assert_ok();
    r1.assert_text("world");
    let r2 = client.post("/echo").text("test body").send().await;
    r2.assert_ok();
    r2.assert_text("test body");
    let payload = json!({
        "msg": "hi"
    });
    let r3 = client.post("/echo").json(&payload).send().await;
    r3.assert_ok();
    r3.assert_json(payload);
    let r4 = client.get("/not_found").send().await;
    r4.assert_status(StatusCode::NOT_FOUND);
    let r5 = client.post("/path/123").send().await;
    r5.assert_ok();
    r5.assert_text("/path/123");
    let r6 = client.get("/macro").send().await;
    r6.assert_ok();
    r6.assert_text("Svc: 42");
}
