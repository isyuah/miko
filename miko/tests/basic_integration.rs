use hyper::StatusCode;
#[allow(unused_imports)]
use miko::macros::{get, u_tag};
use serde_json::json;

#[get("/openapi-explicit-tag")]
#[u_tag("explicit")]
async fn openapi_explicit_tag() {}

#[path = "../examples/basic.rs"]
mod basic_example;

#[tokio::test]
async fn test_basic_integration() {
    let mut app = basic_example::create_app().await;
    let client = app.test_client();
    // --- 测试 1: 宏定义的根路由 ---
    client
        .get("/")
        .send()
        .await
        .assert_text("Hello, World! (macro defined route)");

    // --- 测试 2: Query 参数 ---
    client.get("/with_query?name=Alice&age=18")
        .send()
        .await
        .assert_text("You can also use Query extractor!\n    But #[query] is more convenient if you don't want to define a Query struct.\n    Hello, Alice! You are 18 years old. (macro defined route)");

    // --- 测试 3: Path 参数 ---
    client.get("/with_path/foo/123")
        .send()
        .await
        .assert_text("Hello from path parameters!\n    #[path] and Path<T> has the same behavior.\n    They will extract the value from the path and convert it to the specified type.\n    But they are not named parameters, so the order matters.\n    a: foo, b: 123 (macro defined route)");

    // --- 测试 4: JSON 请求与响应 ---
    let resp = client
        .post("/json_req")
        .json(&json!({"key": 1, "value": 99}))
        .send()
        .await;
    resp.assert_ok();
    // 这里断言部分文本，因为 HashMap 顺序不确定
    let text = resp.text();
    assert!(text.contains("Received JSON data"));
    assert!(text.contains("\"key\": 1"));

    // --- 测试 5: 嵌套路由与模块 (Prefix) ---
    client
        .get("/sub/hello")
        .send()
        .await
        .assert_text("Hello from sub route!");

    // --- 测试 6: 依赖注入 (Dep) ---
    // 第一次调用，数据初始化
    client.get("/use_dep").send().await.assert_ok();
    // 获取数据验证单例
    client
        .get("/data")
        .send()
        .await
        .assert_text("Service data has been changed.");

    // --- 测试 7: 自定义错误 ---
    let err_resp = client.get("/custom_error").send().await;
    err_resp.assert_status(StatusCode::BAD_GATEWAY);

    // --- 测试 8: Panic 捕获 (由 #[miko(catch)] 提供) ---
    let panic_resp = client.get("/panic").send().await;
    panic_resp.assert_status(StatusCode::INTERNAL_SERVER_ERROR);

    // --- 测试 9: Layer (中间件) ---
    let layer_resp = client.get("/layer").send().await;
    layer_resp.assert_header("X-Route-Layer", "Applied");

    // 嵌套 Layer
    let nested_layer_resp = client.get("/layered/inner/test_inner").send().await;
    nested_layer_resp.assert_header("X-Module-Layer", "Applied");
    nested_layer_resp.assert_header("X-Inner-Layer", "Inner-Applied");
    nested_layer_resp.assert_header("X-Route-INNER-Layer", "Inner-Applied");

    // --- 测试 10: State (手动定义的路由) ---
    client.get("/with_state").send().await.assert_text(
        "App Name: Miko Demo App, App Version: 1.0.0 (macro defined route with state)",
    );

    // --- 测试 11: 嵌套的手动 Router (/no_macro) ---
    client
        .get("/no_macro/")
        .send()
        .await
        .assert_text("Hello, World! (manually defined router)");

    // --- 测试 12: OpenAPI 自动收集路径、默认 tag 与组件 schema ---
    let openapi: serde_json::Value = client.get("/api-docs/openapi.json").send().await.json();
    assert!(openapi.pointer("/paths/~1form/post").is_some());
    let default_tag = openapi
        .pointer("/paths/~1form/post/tags/0")
        .and_then(serde_json::Value::as_str)
        .expect("OpenAPI operation should have a default module tag");
    assert!(default_tag.ends_with("basic_example"));
    assert_eq!(
        openapi.pointer("/paths/~1openapi-explicit-tag/get/tags/0"),
        Some(&json!("explicit"))
    );
    assert!(openapi.pointer("/components/schemas/FormStruct").is_some());
}
