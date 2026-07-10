# Integration Testing

Miko provides a powerful `TestClient` utility for performing in-process integration testing. it supports seamless
testing from local routes (`Router`) to full applications (`Application`).

## Key Advantages

- **Extreme Speed**: Bypasses the network stack and dispatches requests directly in memory.
- **No Ports Required**: Supports high-concurrency testing without port conflicts.
- **Full-Chain Coverage**: Fully triggers route resolution, request extractors, middleware (Layers), and dependency
  injection logic.
- **Consistency**: Combined with the `build` mode, ensures the test environment is 100% identical to production.

## Full Example Reference

For a complete integration test covering complex scenarios like DI, multi-layered middleware, and route nesting, please
refer to:

- **Application Code**: [`miko/examples/basic.rs`](../../miko/examples/basic.rs) (Note its use of the `#[miko(build)]`
  mode)
- **Test Code**: [`miko/tests/basic_integration.rs`](../../miko/tests/basic_integration.rs) (Demonstrates how to
  reference app code and perform comprehensive assertions)

## Enabling the Feature

Enable the `test` feature in your `Cargo.toml`:

```toml
[dev-dependencies]
miko = { version = "x.x", features = ["test"] }
```

## Recommended Practice: Using `build` Mode

This is the recommended way to perform integration testing in Miko. By adding the `build` parameter to the `#[miko]`
macro, you can wrap your app initialization into an exportable factory function, making it perfectly reusable in tests.

### 1. Refactor Your App Entry

In your `src/main.rs` or example code:

```rust,ignore
use miko::*;

#[miko(build, catch)] // Add the build parameter and enable sse/catch configs
pub async fn create_app() {
    let mut router = Router::new();

    router.get("/", || async { "Hello Miko" });
    // ... Register other routes, middleware, and DI components
}

#[tokio::main]
async fn main() {
    // Manually get and run the app in production
    let app = create_app().await;
    app.run().await.unwrap();
}
```

### 2. Write Test Cases

In the `tests/` directory, you can directly reference and test the entire application:

```rust,ignore
#[path = "../src/main.rs"] // Reference the main program
mod app_mod;

#[tokio::test]
async fn test_full_application() {
    // 1. Get the fully configured Application instance
    let mut app = app_mod::create_app().await;

    // 2. Create the test client
    let client = app.test_client();

    // 3. Send request and assert
    client.get("/").send().await.assert_text("Hello Miko");
}
```

## Basic Usage

### Testing from a Router

If you only want to test a subset of routes, you can call `test_client()` directly on a `Router`:

```rust,ignore
let mut router = Router::new();
router.get("/ping", || async { "pong" });

let client = router.test_client();
client.get("/ping").send().await.assert_text("pong");
```

## Constructing Requests & Assertions

`TestClient` provides a fluent API similar to `reqwest`:

```rust,ignore
let res = client.post("/api/user")
    .header("Authorization", "Bearer token")
    .json(&json!({ "name": "miko" }))
    .send()
    .await;

res.assert_ok();
res.assert_json(json!({ "id": 1 }));
```

### Common Assertion Methods

- `assert_ok()`: Status code is 2xx.
- `assert_status(code)`: Assert a specific status code.
- `assert_header(key, value)`: Assert a response header.
- `assert_text(expected)`: Assert response body text.
- `assert_json(expected)`: Assert response body JSON.

## Testing Dependency Injection (DI)

`Application::test_client()` and `Router::test_client()` each use an independent DI container; no manual initialization is required:

```rust
#[tokio::test]
async fn test_di() {
    let mut router = Router::new();
    router.merge(miko::auto::collect_global_router());

    let client = router.test_client();
    // ... Execute tests
}
```

## Notes

1. **Trailing Slashes**: Miko's route matching is exact. If `nest("/api", ...)` has a root route `"/"`, access `/api/`
   in tests.
2. **Body Aggregation**: `TestResponse` automatically aggregates the body stream upon creation, allowing you to call
   `.text()` or `.json()` multiple times without losing data.
