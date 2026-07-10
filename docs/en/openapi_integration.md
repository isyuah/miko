# OpenAPI Integration

> **Requires `utoipa` feature**
> Since scanning and generating utoipa paths is implemented via macros like `#[get]`, be sure to place `#[get]` at the
> very top.

Miko integrates the [utoipa](https://github.com/juhaku/utoipa) library to automatically generate OpenAPI 3.0
documentation for your routes.

## Features Provided by Miko

### 1. Automatic Inference of OpenAPI Information

Miko's route macros (`#[get]`, `#[post]`, etc.) automatically analyze handler functions to infer and generate the
following OpenAPI information:

- **Path Parameters**: Automatically identifies parameter names and types from `#[path]` annotations.
- **Query Parameters**: Automatically identifies the query parameter structure from `#[query]` annotations.
- **Request Body**: Automatically identifies the request body type from extractors like `Json<T>`.
- **Doc Comments**: Automatically extracts `///` comments as API descriptions (first line → summary, subsequent lines →
  detailed description).

⚠️ **Note**: Miko **does not automatically infer the response body**, because the return type is `impl IntoResponse`,
making it impossible to determine the specific response model. You must use `#[u_response]` to explicitly label it.

```rust
/// Get user information
///
/// Query detailed user information by user ID
#[get("/users/{id}")]
async fn get_user(
    #[path] id: u32,           // ✅ Automatically generated: name "id", type integer
    #[query] filter: Filter,   // ✅ Automatically generated: query parameter structure
    Json(data): Json<User>,    // ✅ Automatically generated: request body application/json
) -> Json<User> {
    // ✅ Automatically extracted doc comments: summary = "Get user information", description = "Query detailed..."
    // ❌ But the response body needs manual labeling (see example below)
}
```

**Doc Comment Extraction Rules**:

- The first `///` comment line → OpenAPI `summary`.
- Subsequent `///` comment lines → OpenAPI `description`.
- You can override automatically extracted content using `#[u_summary]` and `#[u_description]` macros.

### 2. Documentation Annotation Macros

Miko provides a series of macros to supplement OpenAPI documentation information:

| Macro               | Purpose                          | Example                                    |
|---------------------|----------------------------------|--------------------------------------------|
| `#[u_tag]`          | Set API tag grouping             | `#[u_tag("User Management")]`              |
| `#[u_response]`     | Define response status and model | `#[u_response(status = 200, body = User)]` |
| `#[u_summary]`      | Set API summary                  | `#[u_summary("Get user information")]`     |
| `#[u_description]`  | Set detailed description         | `#[u_description("Query user by ID")]`     |
| `#[u_request_body]` | Customize request body type      | `#[u_request_body(content = Multipart)]`   |
| `#[u_param]`        | Supplement parameter information | `#[u_param(name = "id", example = 123)]`   |
| `#[u_deprecated]`   | Mark API as deprecated           | `#[u_deprecated]`                          |
| `#[desc]`           | Add description to a parameter   | `#[path] #[desc("User ID")] id: u32`       |

## Quick Start

### 1. Add Dependencies

```toml
[dependencies]
miko = { version = "0.3.5", features = ["full"] }

[dev-dependencies]
utoipa-scalar = { version = "0.2", features = ["axum"] }
```

### 2. Define Schema

Derive `ToSchema` for your data structures:

```rust
use miko::*;
use miko::macros::*;

#[derive(Serialize, Deserialize, ToSchema)]
struct User {
    #[schema(example = 1)]
    id: u32,

    #[schema(example = "Alice")]
    name: String,

    #[schema(example = "alice@example.com")]
    email: String,
}
```

### 3. Add Route Documentation

**The response body must be labeled using `#[u_response]`**:

```rust
/// Get user information
///
/// Query and return detailed user information by user ID
#[get("/users/{id}")]
#[u_tag("User Management")]
#[u_response(status = 200, description = "Success", body = User)]  // ← Explicitly label response body
#[u_response(status = 404, description = "User not found")]
async fn get_user(
    #[path]
    #[desc("User ID")] id: u32
) -> AppResult<Json<User>> {
    // Miko infers: path parameter "id"
    // Miko does NOT infer: response body (requires #[u_response])
}
```

**Optional: Overriding doc comments with macros**:

```rust
/// This comment will be overridden by the macros below
#[get("/users/{id}")]
#[u_summary("Query User")]  // ← Overrides the first line of doc comments
#[u_description("Get user information via ID")]  // ← Overrides subsequent lines
#[u_response(status = 200, body = User)]
async fn get_user(#[path] id: u32) -> Json<User> {
    // Final OpenAPI: summary = "Query User", description = "Get user information via ID"
}
```

### 4. Generate OpenAPI Document

If `utoipa` + `auto` are enabled, use `AutoPaths` to collect macro routes (only routes declared with `#[get]`, `#[post]`, `#[route]`, etc.):

```rust
use miko::OpenApi;
use miko::openapi::AutoPaths;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Blog API",
        version = "1.0.0",
        description = "A simple blog API"
    ),
    modifiers(&AutoPaths)
)]
struct ApiDoc;
```

If you want to maintain `paths(...)` manually, you can still use:

```rust
use miko::OpenApi;
use miko::macros::*;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Blog API",
        version = "1.0.0",
        description = "A simple blog API"
    ),
    servers(
        (url = "http://localhost:8080", description = "Local server")
    ),
    tags(
        (name = "User Management", description = "User related APIs"),
        (name = "Post Management", description = "Post related APIs")
    )
)]
struct ApiDoc;

#[route("/openapi.json", method = "get")]
async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}
```

### 5. Integrate Scalar UI

```rust
use utoipa_scalar::{Scalar, Servable};

#[route("/scalar", method = "get")]
async fn scalar_ui() -> impl IntoResponse {
    Scalar::new("/openapi.json").into_response()
}

#[miko]
async fn main() {
    println!("📚 Scalar UI: http://localhost:8080/scalar");
    println!("📄 OpenAPI JSON: http://localhost:8080/openapi.json");
}
```

## Documentation Annotations

### Basic Annotations

```rust
/// API Endpoint Description
///
/// More detailed explanations can be written here, supporting Markdown format
#[get("/users")]
#[u_tag("User Management")]
#[u_response(status = 200, description = "Success", body = Vec<User>)]
async fn list_users() -> Json<Vec<User>> {
    // ...
}
```

### Parameter Documentation

Use `#[desc]` to add descriptions to parameters:

```rust
#[get("/users/{id}")]
async fn get_user(
    #[path]
    #[desc("Unique identifier of the user")] id: u32,
    #[query]
    #[desc("Whether to include detailed information")] include_details: Option<bool>,
) -> AppResult<Json<User>> {
    // ...
}
```

### Response Documentation

Define multiple possible response status codes:

```rust
#[post("/users")]
#[u_tag("User Management")]
#[u_response(status = 201, description = "Created successfully", body = User)]
#[u_response(status = 400, description = "Invalid request parameters")]
#[u_response(status = 409, description = "User already exists")]
async fn create_user(
    Json(data): Json<CreateUser>
) -> AppResult<(StatusCode, Json<User>)> {
    // ...
}
```

## Complete Example

```rust
use miko::*;
use miko::macros::*;
use utoipa_scalar::{Scalar, Servable};

// ========== Schemas ==========

#[derive(Serialize, Deserialize, ToSchema)]
struct User {
    #[schema(example = 1)]
    id: u32,

    #[schema(example = "Alice")]
    name: String,

    #[schema(example = "alice@example.com")]
    email: String,
}

#[derive(Deserialize, ToSchema)]
struct CreateUser {
    #[schema(example = "Bob", min_length = 3)]
    name: String,

    #[schema(example = "bob@example.com")]
    email: String,
}

#[derive(Serialize, ToSchema)]
struct ErrorResponse {
    error: String,
    message: String,
}

// ========== Handlers ==========

/// Get all users
///
/// Returns a list of all users in the system
#[get("/users")]
#[u_tag("User Management")]
#[u_response(status = 200, description = "Successfully returned user list", body = Vec<User>)]
async fn list_users() -> Json<Vec<User>> {
    Json(vec![
        User {
            id: 1,
            name: "Alice".into(),
            email: "alice@example.com".into(),
        }
    ])
}

/// Get a single user
///
/// Query user information by user ID
#[get("/users/{id}")]
#[u_tag("User Management")]
#[u_response(status = 200, description = "Success", body = User)]
#[u_response(status = 404, description = "User not found", body = ErrorResponse)]
async fn get_user(
    #[path]
    #[desc("User ID")] id: u32
) -> AppResult<Json<User>> {
    Ok(Json(User {
        id,
        name: format!("User {}", id),
        email: format!("user{}@example.com", id),
    }))
}

/// Create a user
///
/// Creates a new user
#[post("/users")]
#[u_tag("User Management")]
#[u_response(status = 201, description = "Created successfully", body = User)]
#[u_response(status = 400, description = "Invalid request parameters", body = ErrorResponse)]
#[u_response(status = 409, description = "User already exists", body = ErrorResponse)]
async fn create_user(
    Json(data): Json<CreateUser>
) -> (StatusCode, Json<User>) {
    (
        StatusCode::CREATED,
        Json(User {
            id: 1,
            name: data.name,
            email: data.email,
        })
    )
}

/// Update a user
#[put("/users/{id}")]
#[u_tag("User Management")]
#[u_response(status = 200, description = "Updated successfully", body = User)]
#[u_response(status = 404, description = "User not found")]
async fn update_user(
    #[path] id: u32,
    Json(data): Json<CreateUser>,
) -> Json<User> {
    Json(User {
        id,
        name: data.name,
        email: data.email,
    })
}

/// Delete a user
#[delete("/users/{id}")]
#[u_tag("User Management")]
#[u_response(status = 204, description = "Deleted successfully")]
#[u_response(status = 404, description = "User not found")]
async fn delete_user(#[path] id: u32) -> StatusCode {
    StatusCode::NO_CONTENT
}

// ========== OpenAPI ==========

#[derive(OpenApi)]
#[openapi(
    info(
        title = "User API",
        version = "1.0.0",
        description = "User Management API Documentation",
        contact(
            name = "API Support",
            email = "support@example.com"
        )
    ),
    servers(
        (url = "http://localhost:8080", description = "Development Environment"),
        (url = "https://api.example.com", description = "Production Environment")
    ),
    tags(
        (name = "User Management", description = "User related CRUD operations")
    )
)]
struct ApiDoc;

#[route("/openapi.json", method = "get")]
async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

#[route("/scalar", method = "get")]
async fn scalar_ui() -> impl IntoResponse {
    Scalar::new("/openapi.json").into_response()
}

#[miko]
async fn main() {
    println!("🚀 Server running on http://localhost:8080");
    println!("📚 Scalar UI:    http://localhost:8080/scalar");
    println!("📄 OpenAPI JSON: http://localhost:8080/openapi.json");
}
```

## utoipa Documentation

Miko's OpenAPI integration is based on the [utoipa](https://docs.rs/utoipa/) library. For more advanced usage, please
refer to:

- **Schema Definition**: [utoipa ToSchema](https://docs.rs/utoipa/latest/utoipa/derive.ToSchema.html)
- **OpenAPI Configuration**: [utoipa OpenApi](https://docs.rs/utoipa/latest/utoipa/derive.OpenApi.html)
- **Full Documentation**: [utoipa official docs](https://docs.rs/utoipa/)

## Next Steps

- ✅ Learn [Data Validation](data_validation.md) to improve API quality.
- 🔍 Understand usage of [Request Extractors](request_extractors.md).
- 📖 Review [Routing System](routing_system.md) to define routes.
