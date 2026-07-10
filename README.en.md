# Miko

<div align="center">

**A modern, high-performance Rust web framework**

[![Crates.io](https://img.shields.io/crates/v/miko.svg)](https://crates.io/crates/miko)
[![Documentation](https://docs.rs/miko/badge.svg)](https://docs.rs/miko)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

[中文](README.md) | [English](README.en.md)

</div>

## ✨ Features

- 🚀 **High Performance** - Built on Hyper and Tokio, fully leveraging Rust's asynchronous features.
- 🎯 **Type Safe** - Complete type inference, catching errors at compile time.
- 🔌 **Modular Design** - Enable features on demand via `features`.
- 🎨 **Elegant Macros** - Provides concise and intuitive route definition macros.
- 🔄 **Dependency Injection** - Built-in dependency container, supporting automatic component assembly.
- 📝 **OpenAPI Support** - Seamless integration with `utoipa` for automatic API documentation generation.
- ✅ **Data Validation** - Integrated with `garde` for powerful data validation capabilities.
- 🌐 **WebSocket** - Native WebSocket support.
- ✅ **Built-in Testing** - Powerful TestClient for lightning-fast in-process integration testing.
- 🔍 **Unified Error Handling** - Elegant error handling mechanism.
- 🔄 **Graceful Shutdown** - Signal handling and connection draining.
- 🎭 **Tower Ecosystem** - Compatible with the Tower middleware ecosystem.

## 🚀 Quick Start

### Installation

```bash
cargo add miko --features=full
```

### Hello World

```rust
use miko::*;
use miko::macros::*;

#[get("/")]
async fn hello() -> &'static str {
    "Hello, Miko!"
}

#[miko]
async fn main() {
}
```

After running the program, visit `http://localhost:8080`.

### More Examples

```rust,ignore
use miko::{*, macros::*, extractor::{Json, Path, Query}};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct CreateUser {
    name: String,
    email: String,
}

#[derive(Serialize)]
struct User {
    id: u32,
    name: String,
    email: String,
}

// Using route macros and extractors
#[post("/users")]
async fn create_user(Json(data): Json<CreateUser>) -> Json<User> {
    Json(User {
        id: 1,
        name: data.name,
        email: data.email,
    })
}

// Path parameters
#[get("/users/{id}")]
async fn get_user(Path(id): Path<u32>) -> Json<User> {
    Json(User {
        id,
        name: "Alice".into(),
        email: "alice@example.com".into(),
    })
}
```

```rust
// Query parameters
#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: Option<String>,
    page: Option<u32>,
    per_page: Option<u32>,
}

#[get("/search")]
async fn search(Query(params): Query<SearchQuery>) -> String {
    format!("Searching for: {:?}", params)
}
```

```rust
#[tokio::main]
async fn main() {
    let router = Router::new()
        .post("/users", create_user)
        .get("/users/{id}", get_user)
        .get("/search", search);

    Application::new_(router).run().await.unwrap();
}
```

## 📚 Documentation

- **[Quick Start](docs/en/quick_start.md)** - 5-minute tutorial
- **[Basic Concepts](docs/en/basic_concepts.md)** - Detailed explanation of core concepts
- **[Routing System](docs/en/routing_system.md)** - Route definition and management
- **[Request Extractors](docs/en/request_extractors.md)** - Extracting request data
- **[Response Handling](docs/en/response_handling.md)** - Building various responses
- **[Error Handling](docs/en/error_handling.md)** - Unified error handling
- **[Middleware and Layers](docs/en/middleware_and_layers.md)** - Using middleware
- **[Dependency Injection](docs/en/dependency_injection.md)** - Component management
- **[WebSocket Support](docs/en/websocket_support.md)** - WebSocket development
- **[Configuration Management](docs/en/configuration_management.md)** - Application configuration
- **[OpenAPI Integration](docs/en/openapi_integration.md)** - API documentation generation
- **[Data Validation](docs/en/data_validation.md)** - Request data validation
- **[Integration Testing](docs/en/integration_testing.md)** - High-speed integration testing tools
- **[Advanced Features](docs/en/advanced_features.md)** - Advanced functionalities

## 🎯 Features

Miko has a modular design, allowing you to enable features as needed:

```toml,ignore
[dependencies]
# By default, core features are enabled (macros, auto-registration, extensions)
miko = "x.x"

# Or enable all features, including OpenAPI and data validation
miko = { version = "x.x", features = ["full"] }

# Or enable only the features you need
miko = { version = "x.x", features = ["utoipa", "validation"] }
```

Available features:

- `default` - Core features (`macro` + `auto` + `ext`), **enabled by default**
- `full` - Enables all features (including external extensions)
- `macro` - Enables route macros (`#[get]`, `#[post]`, etc.)
- `auto` - Enables automatic route registration and dependency injection
- `ext` - Enables extension features (quick CORS, static files, etc.)
- `test` - Enables integration testing tools (`TestClient`)
- `utoipa` - Enables OpenAPI documentation generation (automatically re-exports the `utoipa` crate)
- `validation` - Enables data validation (automatically re-exports the `garde` crate)

**Note**: When the `utoipa` or `validation` feature is enabled, you don't need to manually add these dependencies to your `Cargo.toml`. The framework automatically re-exports them:

```rust
// After enabling the utoipa feature, use it directly
use miko::{utoipa, OpenApi, ToSchema};

// After enabling the validation feature, use it directly
use miko::{garde, Validate};
```

## 🛠️ Core Components

### Route Macros

Define routes with concise macros:

```rust
#[get("/users")]
async fn list_users() -> Json<Vec<User>> { /* ... */ }

#[post("/users")]
async fn create_user(Json(data): Json<CreateUser>) -> AppResult<Json<User>> { /* ... */ }

#[put("/users/{id}")]
async fn update_user(Path(id): Path<u32>, Json(data): Json<UpdateUser>) -> AppResult<Json<User>> { /* ... */ }

#[delete("/users/{id}")]
async fn delete_user(Path(id): Path<u32>) -> AppResult<()> { /* ... */ }
```

### Dependency Injection

Use `#[component]` and `#[dep]` for dependency injection:

```rust
#[component]
impl Database {
    async fn new() -> Self {
        // Initialize database connection
        Self { /* ... */ }
    }
}

#[get("/users")]
async fn list_users(#[dep] db: Arc<Database>) -> Json<Vec<User>> {
    // Use the injected database instance
    Json(vec![])
}
```

### Declarative Middleware

Define reusable middleware using `#[middleware]`, supporting argument injection:

```rust
#[middleware]
async fn logger(#[config("app.name")] app_name: String) -> AppResult<Resp> {
    println!("Request to {}", app_name);
    _next.run(_req).await
}

#[get("/")]
#[layer(logger())]
async fn hello() -> &'static str {
    "Hello"
}
```

### OpenAPI Documentation

Automatically generate API documentation with inferred params, summary, and description. When `utoipa` + `auto` are enabled, `AutoPaths` can collect macro routes so you do not need to maintain `paths(...)` manually.

```rust
use miko::*;
use miko::openapi::AutoPaths;

#[derive(OpenApi)]
#[openapi(
    info(title = "Miko Basic Example API", version = "1.0.0"),
    modifiers(&AutoPaths)
)]
struct ApiDoc;

#[derive(Serialize, Deserialize, ToSchema)]
struct User {
    id: u32,
    name: String,
}

#[get("/users/{id}")]
#[u_tag("User Management")]
#[u_response(status = 200, description = "Success", body = User)]
async fn get_user(
    #[path] #[desc("User ID")] id: u32
) -> Json<User> {
    // ...
}
```

### Data Validation

Use `ValidatedJson` for automatic validation:

```rust
use garde::Validate;

#[derive(Deserialize, Validate)]
struct CreateUser {
    #[garde(length(min = 3, max = 50))]
    name: String,

    #[garde(contains("@"))]
    email: String,
}

#[post("/users")]
async fn create_user(
    ValidatedJson(data): ValidatedJson<CreateUser>
) -> Json<User> {
    // Data has been validated
}
```

## 🌟 Example

The `miko/examples/` directory contains a comprehensive `all-in-one` example:

- **[basic.rs](./miko/examples/basic.rs)**

This example covers most of the framework's core features, including routing, middleware, dependency injection, WebSockets, file uploads, and more. It is highly recommended to check this file to quickly understand how to use Miko.

To run the example:

```bash
cargo run --example basic --features full
```

## 🤝 Contributing

We welcome contributions of any kind. For details on how to contribute, please see [CONTRIBUTING.md](CONTRIBUTING.md).

## 📄 License

## 🔗 Related Links

- [GitHub Repository](https://github.com/isyuah/miko)
- [crates.io](https://crates.io/crates/miko)
- [Documentation](https://docs.rs/miko)

## 💬 Community & Support

- Submit an Issue: [GitHub Issues](https://github.com/isyuah/miko/issues)
- Discussion: [GitHub Discussions](https://github.com/isyuah/miko/discussions)
