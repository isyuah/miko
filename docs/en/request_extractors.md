# Request Extractors

Extractors are the core mechanism in the Miko framework for processing request parameters by extracting data from HTTP
requests.

## Extractor Types

Miko provides two types of extractors:

### FromRequestParts

Extracts data from parts of the request, **does not consume the request body**. You can use multiple `FromRequestParts`
extractors in a single handler:

- `Path<T>` - Path parameters
- `Query<T>` - Query parameters
- `State<T>` - Global state
- `HeaderMap` - Request headers
- `Method` - HTTP method
- `Uri` - Request URI

### FromRequest

Extracts data from the full request, **may consume the request body**. Only one `FromRequest` extractor is allowed per
handler:

- `Json<T>` - JSON request body
- `Form<T>` - Form data
- `Multipart` `MultipartResult` - File upload
- `ValidatedJson<T>` - Validated JSON (requires `validation` feature, using `garde`)

## Json - JSON Request Body

Deserialize JSON data from the request body:

```rust
use miko::{*, extractor::Json};
use miko::macros::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct CreateUser {
    name: String,
    email: String,
    age: u8,
}

#[derive(Serialize)]
struct User {
    id: u32,
    name: String,
    email: String,
}

#[post("/users")]
async fn create_user(Json(data): Json<CreateUser>) -> Json<User> {
    Json(User {
        id: 1,
        name: data.name,
        email: data.email,
    })
}
```

**Automatic Error Handling**: If JSON parsing fails, a 400 Bad Request error is automatically returned.

## Query - Query Parameters

Extract parameters from the URL query string:

### Basic Usage

```rust
use miko::{*, extractor::Query};
use miko::macros::*;
use serde::Deserialize;

// Using a struct
#[derive(Deserialize)]
struct Pagination {
    page: Option<u32>,
    per_page: Option<u32>,
}

#[get("/users")]
async fn list_users(Query(pagination): Query<Pagination>) -> String {
    let page = pagination.page.unwrap_or(1);
    let per_page = pagination.per_page.unwrap_or(20);
    format!("Page: {}, Per page: {}", page, per_page)
}
```

Access example:

- `/users?page=2&per_page=50`

### Get Raw Query String

If you need access to the entire query string without parsing it into a specific type, you can use `RawQuery`:

```rust
use miko::*;
use miko::macros::*;
use hyper::Uri;

#[get("/search")]
async fn search(uri: Uri) -> String {
    let query = uri.query().unwrap_or("");
    format!("Raw query: {}", query)
}
```

Or create a custom extractor to get a parsed HashMap:

```rust
use std::collections::HashMap;
use miko::extractor::from_request::FromRequestParts;
use hyper::http::request::Parts;
use std::sync::Arc;

/// Raw query parameter Map
pub struct QueryMap(pub HashMap<String, String>);

impl<S> FromRequestParts<S> for QueryMap {
    fn from_request_parts(
        req: &mut Parts,
        _state: Arc<S>
    ) -> miko::extractor::from_request::FRPFut<Self> {
        let query = req.uri.query().unwrap_or("");
        Box::pin(async move {
            let mut map = HashMap::new();
            for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
                map.insert(key.into_owned(), value.into_owned());
            }
            Ok(QueryMap(map))
        })
    }
}

// Usage
#[get("/search")]
async fn search(QueryMap(params): QueryMap) -> String {
    format!("Params: {:?}", params)
}
```

> **Note**: `Query<T>` does not support `HashMap<String, String>` type because it conflicts with the `Deserialize`
> trait. Please use the custom `QueryMap` extractor described above.

## Path - Path Parameters

Extract parameters from the URL path (extracted in order, variable names cannot be verified):

### Single Parameter

```rust
use miko::{*, extractor::Path};
use miko::macros::*;

#[get("/users/{id}")]
async fn get_user(Path(id): Path<u32>) -> String {
    format!("User ID: {}", id)
}
```

### Multiple Parameters

```rust
#[get("/users/{user_id}/posts/{post_id}")]
async fn get_post(
    Path((user_id, post_id)): Path<(u32, u32)>
) -> String {
    format!("User: {}, Post: {}", user_id, post_id)
}
```

### Using `#[path]` Annotation (Available when using macros)

```rust
#[get("/users/{id}")]
async fn get_user(#[path] id: u32) -> String {
    format!("User ID: {}", id)
}

#[get("/users/{user_id}/posts/{post_id}")]
async fn get_user_post(
    #[path] user_id: u32,
    #[path] post_id: u32,
) -> String {
    format!("User: {}, Post: {}", user_id, post_id)
}
```

**Type Safety**: Path supports any type that implements `FromStr`. If conversion fails, a 400 error is returned.

## Form - Form Data

Extract `application/x-www-form-urlencoded` form data:

```rust
use miko::{*, extractor::Form};
use miko::macros::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

#[post("/login")]
async fn login(Form(form): Form<LoginForm>) -> String {
    format!("Login: {}", form.username)
}
```

HTML form example:

```html

<form method="POST" action="/login">
  <input name="username" type="text">
  <input name="password" type="password">
  <button type="submit">Login</button>
</form>
```

## State - Global State

Extract global state set via `Router::with_state` (Remember to set it beforehand, mount it to a single route function
when routing):

```rust
use miko::{*, extractor::State};
use miko::macros::*;
use std::sync::Arc;

struct AppState {
    db: Database,
    cache: Cache,
}

#[get("/users")]
async fn list_users(State(state): State<AppState>) -> String {
    // Use state.db, state.cache
    format!("Users from DB: {}", state.db.count())
}

#[tokio::main]
async fn main() {
    let state = AppState {
        db: Database::new(),
        cache: Cache::new(),
    };

    let router = Router::new()
        .with_state(state)
        .get("/users", list_users);

    let config = ApplicationConfig::default();
    Application::new(config, router).run().await.unwrap();
}
```

State is wrapped in `Arc<T>` and can be safely shared across multiple handlers.

> **⚠️ Important**: `#[dep]` and `State` are basically incompatible.
>
> - When using `#[miko]` macro, routes are automatically registered, and the State type for all routes is `()`.
> - If you need to use both dependency injection and custom State, you need to:
    >

1. Enable `auto` feature

> 2. **Do not use** `#[miko]` macro
>   3. Manually register routes and set State
>
> Example:
> ```rust
> // ❌ Cannot mix - #[miko] macro forces State to ()
> #[miko]
> async fn main() {
>     // Routes auto-registered, State type is ()
> }
>
> // ✅ Can mix - Manual route registration
> #[tokio::main]
> async fn main() {
>     let state = AppState { /* ... */ };
>     let router = Router::new()
>         .with_state(state)
>         .get("/users", list_users);  // Can use #[dep] and State
>
>     Application::new_(router).run().await.unwrap();
> }
> ```
>
> **Recommendation**: Prioritize `#[dep]` dependency injection and avoid mixing with State.

## Request Headers

Extract HTTP request headers:

```rust
use hyper::HeaderMap;

#[get("/headers")]
async fn check_headers(headers: HeaderMap) -> String {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("none");

    format!("Authorization: {}", auth)
}
```

## Using `#[dep]` Dependency Injection

> **Requires `auto` feature**

Use dependency injection instead of State:

```rust
use miko::*;
use miko::macros::*;
use std::sync::Arc;

#[component]
impl Database {
    async fn new() -> Self {
        Self { /* Init */ }
    }

    pub fn get_user(&self, id: u32) -> User {
        // ...
    }
}

#[get("/users/{id}")]
async fn get_user(
    #[path] id: u32,
    #[dep] db: Arc<Database>,
) -> Json<User> {
    Json(db.get_user(id))
}
```

See [Dependency Injection](dependency_injection.md) for details.

## Using `#[config]` Configuration Injection

Inject values from configuration file:

```rust
#[get("/info")]
async fn info(
    #[config("app.name")] app_name: String,
    #[config("app.version")] version: String,
) -> String {
    format!("{} v{}", app_name, version)
}
```

Configuration file `config.toml`:

```toml
[app]
name = "My App"
version = "1.0.0"
```

See [Configuration Management](configuration_management.md) for details.

## Multipart / MultipartResult - File Upload

Handle `multipart/form-data` requests for file uploads. Miko provides two ways:

### MultipartResult - Automatic Parsing (Recommended)

Automatically parses all fields and files, files are saved to temporary files:

```rust
use miko::{*, extractor::multipart::MultipartResult};
use miko::macros::*;

#[post("/upload")]
async fn upload(multipart: MultipartResult) -> AppResult<String> {
    // Access normal form fields
    if let Some(titles) = multipart.fields.get("title") {
        println!("Title: {}", titles.first().unwrap());
    }

    // Access uploaded files
    if let Some(files) = multipart.files.get("file") {
        for file in files {
            println!("Uploaded: {} ({} bytes)", file.filename, file.size);
            println!("Content-Type: {:?}", file.content_type);

            // Copy to target location
            file.linker.transfer_to(format!("uploads/{}", file.filename)).await?;
        }
    }

    Ok(format!("Uploaded {} files", multipart.files.len()))
}
```

**MultipartResult Structure**:

```rust
pub struct MultipartResult {
    pub fields: HashMap<String, Vec<String>>,  // Normal fields
    pub files: HashMap<String, Vec<MultipartFile>>,  // File fields
}

pub struct MultipartFile {
    pub filename: String,           // Original filename
    pub size: usize,                // File size (bytes)
    pub content_type: Option<Mime>, // MIME type
    pub linker: MultipartFileDiskLinker,  // Disk linker
}
```

**MultipartFileDiskLinker Methods**:

```rust
// Copy to specified path
pub async fn transfer_to(&self, path: impl Into<PathBuf>) -> Result<u64, std::io::Error>

// Read entire file as string
pub async fn read_to_string(&mut self) -> Result<String, std::io::Error>

// Read all bytes and close file
pub async fn read_and_drop_file(mut self) -> Result<Bytes, std::io::Error>

// Get file metadata
pub async fn metadata(&self) -> std::io::Result<Metadata>
```

### Complete Example

```rust
use miko::{*, extractor::multipart::MultipartResult};
use miko::macros::*;

#[post("/upload")]
async fn upload_files(multipart: MultipartResult) -> AppResult<Json<serde_json::Value>> {
    let mut uploaded = vec![];

    // Process each file
    for (field_name, files) in &multipart.files {
        for file in files {
            // Generate safe filename
            let safe_name = format!("{}_{}",
                                    chrono::Utc::now().timestamp(),
                                    file.filename
            );

            // Save to uploads directory
            let dest = format!("uploads/{}", safe_name);
            file.linker.transfer_to(&dest).await?;

            uploaded.push(serde_json::json!({
                "field": field_name,
                "original_name": file.filename,
                "saved_as": safe_name,
                "size": file.size,
                "content_type": file.content_type.as_ref().map(|m| m.to_string()),
            }));
        }
    }

    Ok(Json(serde_json::json!({
        "message": "Upload successful",
        "files": uploaded,
    })))
}
```

### Multipart - Manual Parsing

For streaming processing or custom parsing logic, use raw `Multipart`:

```rust
use miko::{*, extractor::multipart::Multipart};
use miko::macros::*;

#[post("/upload-stream")]
async fn upload_stream(mut multipart: Multipart) -> AppResult<String> {
    let mut count = 0;

    while let Some(field) = multipart.0.next_field().await? {
        let name = field.name().unwrap_or("unknown");

        if let Some(filename) = field.file_name() {
            let data = field.bytes().await?;
            println!("File: {}, Size: {}", filename, data.len());

            // Custom save logic
            tokio::fs::write(format!("uploads/{}", filename), data).await?;
            count += 1;
        } else {
            // Normal field
            let value = field.text().await?;
            println!("Field {}: {}", name, value);
        }
    }

    Ok(format!("Uploaded {} files", count))
}
```

### Frontend Example

HTML Form:

```html

<form method="POST" action="/upload" enctype="multipart/form-data">
  <input name="title" type="text" placeholder="Title">
  <input name="file" type="file" multiple>
  <button type="submit">Upload</button>
</form>
```

JavaScript Fetch API:

```javascript
const formData = new FormData();
formData.append('title', 'My Upload');
formData.append('file', fileInput.files[0]);

fetch('/upload', {
  method: 'POST',
  body: formData
});
```

### Multiple File Upload Example

```rust
#[post("/upload-multiple")]
async fn upload_multiple(multipart: MultipartResult) -> AppResult<String> {
    let mut total_size = 0;
    let mut file_count = 0;

    // Iterate all file fields
    for (field_name, files) in &multipart.files {
        println!("Processing field: {}", field_name);

        for (index, file) in files.iter().enumerate() {
            total_size += file.size;
            file_count += 1;

            // Generate unique name for each file
            let dest = format!("uploads/{}_{}.{}",
                               field_name,
                               index,
                               file.filename.split('.').last().unwrap_or("bin")
            );

            file.linker.transfer_to(dest).await?;
        }
    }

    Ok(format!(
        "Uploaded {} files, total size: {} bytes",
        file_count,
        total_size
    ))
}
```

### Read File Content

```rust
#[post("/process-csv")]
async fn process_csv(mut multipart: MultipartResult) -> AppResult<Json<Vec<String>>> {
    if let Some(files) = multipart.files.get_mut("csv") {
        if let Some(file) = files.first_mut() {
            // Read as string
            let content = file.linker.read_to_string().await?;

            // Parse CSV
            let lines: Vec<String> = content
                .lines()
                .map(|s| s.to_string())
                .collect();

            return Ok(Json(lines));
        }
    }

    Err(AppError::BadRequest("No CSV file uploaded".into()))
}
```

### Validate File Type and Size

```rust
#[post("/upload-image")]
async fn upload_image(mut multipart: MultipartResult) -> AppResult<String> {
    if let Some(files) = multipart.files.remove("image") { // Use remove to take ownership for read_and_drop_file
        for file in files {
            // Validate MIME type
            if let Some(mime) = &file.content_type {
                if !mime.type_().as_str().starts_with("image/") {
                    return Err(AppError::BadRequest(
                        format!("File {} is not an image", file.filename)
                    ));
                }
            }

            // Validate file size (max 5MB)
            const MAX_SIZE: usize = 5 * 1024 * 1024;
            if file.size > MAX_SIZE {
                return Err(AppError::BadRequest(
                    format!("File {} exceeds 5MB limit", file.filename)
                ));
            }

            // Save file
            file.linker.transfer_to(format!("images/{}", file.filename)).await?;
        }
    }

    Ok("Images uploaded successfully".to_string())
}
```

See [Advanced Features - File Upload](advanced_features.md#file-upload) for details.

## ValidatedJson - Validated JSON

> **Requires `validation` feature**

Automatically validate JSON data:

```rust
use miko::{*, extractor::ValidatedJson};
use miko::macros::*;
use serde::Deserialize;
use garde::Validate;

#[derive(Deserialize, Validate)]
struct CreateUser {
    #[garde(length(min = 3, max = 50))]
    name: String,

    #[garde(contains("@"))]
    email: String,

    #[garde(range(min = 18, max = 120))]
    age: u8,
}

#[post("/users")]
async fn create_user(
    ValidatedJson(data): ValidatedJson<CreateUser>
) -> Json<User> {
    // Data validated
    Json(User {
        id: 1,
        name: data.name,
        email: data.email,
    })
}
```

If validation fails, a 400 response with detailed error information is automatically returned.

See [Data Validation](data_validation.md) for details.

## Combining Multiple Extractors

A handler can use multiple extractors:

```rust
use miko::{*, extractor::{Json, Path, Query}};
use miko::macros::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct UpdateData {
    name: String,
}

#[derive(Deserialize)]
struct Options {
    notify: Option<bool>,
}

#[put("/users/{id}")]
async fn update_user(
    #[path] id: u32,                          // FromRequestParts
    Query(options): Query<Options>,            // FromRequestParts
    headers: HeaderMap,                        // FromRequestParts
    Json(data): Json<UpdateData>,              // FromRequest
) -> AppResult<Json<User>> {
    // Use all extracted data
    Ok(Json(user))
}
```

**Important Rules**:

- You can have multiple `FromRequestParts` extractors
- You can have only one `FromRequest` extractor (consumes request body)

## Custom Extractor

Implement `FromRequest` or `FromRequestParts` trait:

```rust
use miko::extractor::from_request::FromRequestParts;
use miko::handler::Req;
use hyper::http::request::Parts;
use std::sync::Arc;

struct AuthUser {
    id: u32,
    name: String,
}

impl<S> FromRequestParts<S> for AuthUser {
    fn from_request_parts(
        parts: &mut Parts,
        _state: Arc<S>
    ) -> miko::extractor::from_request::FRPFut<Self> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        Box::pin(async move {
            if token.is_empty() {
                return Err(AppError::Unauthorized("Missing token".into()));
            }

            // Validate token and get user info
            Ok(AuthUser {
                id: 1,
                name: "User".into(),
            })
        })
    }
}

// Use custom extractor
#[get("/profile")]
async fn profile(user: AuthUser) -> String {
    format!("Hello, {}!", user.name)
}
```

## Error Handling

When an extractor fails, an error response is automatically returned:

| Extractor          | Failure Scenario       | Error Code | Error Message        |
|--------------------|------------------------|------------|----------------------|
| `Json<T>`          | JSON parse failed      | 400        | JsonParseError       |
| `Query<T>`         | URL decode failed      | 400        | UrlEncodedParseError |
| `Path<T>`          | Type conversion failed | 400        | BadRequest           |
| `Form<T>`          | Form parse failed      | 400        | UrlEncodedParseError |
| `ValidatedJson<T>` | Validation failed      | 400        | ValidationError      |

All errors are converted to a unified JSON format. See [Error Handling](error_handling.md) for details.

## Best Practices

### 1. Use Parameter Annotations

For simple scenarios, using annotations like `#[path]`, `#[query]` is more concise:

```rust
// ✅ Concise
#[get("/users/{id}")]
async fn get_user(#[path] id: u32) {}

// ✅ Acceptable
#[get("/users/{id}")]
async fn get_user(Path(id): Path<u32>) {}
```

### 2. Use Option Wisely

Use `Option` for query parameters and optional fields:

```rust
#[derive(Deserialize)]
struct Filters {
    status: Option<String>,
    category: Option<String>,
    page: Option<u32>,
}
```

### 3. Combine with Dependency Injection

Combine multiple extraction methods:

```rust
#[get("/users/{id}")]
async fn get_user(
    #[path] id: u32,
    #[dep] db: Arc<Database>,
    #[config("feature.cache")] use_cache: bool,
    user: AuthUser,  // Custom extractor
) -> AppResult<Json<User>> {
    // ...
}
```

### 4. Get Raw Query Parameters

If you need dynamic query parameters instead of a fixed struct, use custom `QueryMap`:

```rust
// ✅ Recommended - Use struct
#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    page: Option<u32>,
}

#[get("/search")]
async fn search(Query(query): Query<SearchQuery>) {}

// ✅ Dynamic parameters - Use custom QueryMap
#[get("/search")]
async fn search_dynamic(QueryMap(params): QueryMap) {
    // params is HashMap<String, String>
}

// ❌ Not supported - HashMap conflicts with Deserialize
#[get("/search")]
async fn search(Query(params): Query<HashMap<String, String>>) {}
```

## Complete Example

```rust
use miko::{*, extractor::{Json, Path, Query}};
use miko::macros::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct CreatePost {
    title: String,
    content: String,
}

#[derive(Deserialize)]
struct ListQuery {
    page: Option<u32>,
    per_page: Option<u32>,
    tag: Option<String>,
}

#[derive(Serialize)]
struct Post {
    id: u32,
    title: String,
    content: String,
}

// List - Query params
#[get("/posts")]
async fn list_posts(Query(query): Query<ListQuery>) -> Json<Vec<Post>> {
    let page = query.page.unwrap_or(1);
    let per_page = query.per_page.unwrap_or(20);

    // Query DB
    Json(vec![])
}

// Get single - Path param
#[get("/posts/{id}")]
async fn get_post(#[path] id: u32) -> AppResult<Json<Post>> {
    // Query DB
    Ok(Json(Post {
        id,
        title: "Example".into(),
        content: "Content".into(),
    }))
}

// Create - JSON body
#[post("/posts")]
async fn create_post(Json(data): Json<CreatePost>) -> Json<Post> {
    Json(Post {
        id: 1,
        title: data.title,
        content: data.content,
    })
}

// Combine multiple extractors
#[put("/posts/{id}")]
async fn update_post(
    #[path] id: u32,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
    Json(data): Json<CreatePost>,
) -> AppResult<Json<Post>> {
    // Check permission
    let auth = headers.get("authorization");

    // Update data
    Ok(Json(Post {
        id,
        title: data.title,
        content: data.content,
    }))
}

#[miko]
async fn main() {
    println!("🚀 Server running");
}
```

## Next Steps

- 📤 Learn various ways of [Response Handling](response_handling.md)
- ⚠️ Understand [Error Handling](error_handling.md) mechanism
- ✅ Use [Data Validation](data_validation.md) to validate input
- 💉 Explore [Dependency Injection](dependency_injection.md) features
