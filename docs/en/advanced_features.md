# Advanced Features

This document introduces the advanced features of the Miko framework, including automatic route registration, static
file services, file uploads, and the Trace ID tracking system.

## Automatic Route Registration

> **Requires `auto` feature**

Using the `#[miko]` macro can automatically collect and register all routes, eliminating the need for manual addition:

### Basic Usage

```rust
use miko::*;
use miko::macros::*;

#[get("/")]
async fn index() -> &'static str {
    "Hello, World!"
}

#[get("/users")]
async fn list_users() -> Json<Vec<String>> {
    Json(vec!["Alice".into(), "Bob".into()])
}

#[post("/users")]
async fn create_user(Json(data): Json<serde_json::Value>) -> StatusCode {
    StatusCode::CREATED
}

// Automatically register all routes
#[miko]
async fn main() {
    println!("🚀 Server running on http://localhost:8080");
}
```

### What the `#[miko]` macro does

The `#[miko]` macro expands to:

```rust
#[tokio::main]
async fn main() {
    // 1. Load configuration files (config.toml + config.{dev/prod}.toml)
    let config = miko::app::ApplicationConfig::load();

    // 2. Collect all routes marked with macros like #[get], #[post]
    let router = miko::auto::collect_routes();

    // 3. Create and run the application with its own dependency container
    miko::app::Application::new(config, router)
        .run()
        .await
        .unwrap();
}
```

### Manual Control

If you need more control, you can choose not to use the `#[miko]` macro:

```rust
use miko::*;
use miko::macros::*;

#[get("/")]
async fn index() -> &'static str {
    "Hello"
}

#[tokio::main]
async fn main() {
    // Custom configuration
    let mut config = ApplicationConfig::default();
    config.port = 9000;

    // Manually collect routes
    let router = miko::auto::collect_routes();

    // Add extra middleware
    let router = router.layer(/* ... */);

    Application::new(config, router).run().await.unwrap();
}
```

## Static File Service

> **Requires `ext` feature**

Miko provides static file service capabilities, supporting directory mapping and SPA (Single Page Application) fallback.

### Basic Usage

```rust
use miko::*;
use miko::macros::*;
use miko::ext::static_svc::StaticSvc;

#[miko]
async fn main() {
    // Mount a static file directory
    router.nest_service("/static", StaticSvc::builder("public").build());

    println!("📁 Static files at http://localhost:8080/static/");
}
```

Access examples:

- `/static/index.html` → `public/index.html`
- `/static/css/style.css` → `public/css/style.css`
- `/static/images/logo.png` → `public/images/logo.png`

### SPA Mode

For Single Page Applications like Vue or React, enable SPA fallback:

```rust
use miko::ext::static_svc::StaticSvc;

#[miko]
async fn main() {
    // All unmatched routes return index.html
    router.nest_service(
        "/",
        StaticSvc::builder("dist")
            .spa_fallback(true)  // Enable SPA fallback
            .build()
    );
}
```

With this configuration:

- `/` → `dist/index.html`
- `/about` → `dist/index.html` (handled by frontend routing)
- `/static/app.js` → `dist/static/app.js`

### Full Configuration

```rust
use miko::ext::static_svc::StaticSvc;

#[get("/api/users")]
async fn api_users() -> Json<Vec<String>> {
    Json(vec!["Alice".into()])
}

#[miko]
async fn main() {
    // API routes have higher priority
    // (Routes defined before static services)

    // Static file service
    router.nest_service(
        "/assets",
        StaticSvc::builder("public/assets").build()
    );

    // SPA Application (placed last as a catch-all)
    router.nest_service(
        "/",
        StaticSvc::builder("public")
            .spa_fallback(true)
            .build()
    );

    println!("🌐 SPA at http://localhost:8080/");
    println!("📦 API at http://localhost:8080/api/");
}
```

### Security

`StaticSvc` automatically prevents path traversal attacks:

```rust
// ❌ These requests will be blocked
// /static/../../../etc/passwd
// /static/..%2F..%2Fetc%2Fpasswd

// ✅ Only files within the specified directory can be accessed
// /static/style.css
// /static/images/logo.png
```

## File Upload

> **Requires `ext` feature**

Miko provides a convenient file upload service.

### Using the Uploader Service

```rust
use miko::*;
use miko::macros::*;
use miko::ext::uploader::{Uploader, DiskStorage, DiskStorageConfig};

#[miko]
async fn main() {
    // Mount a single file upload service
    router.service(
        "/upload",
        Uploader::single(DiskStorage::new(
            "uploads",                                    // Save directory
            DiskStorageConfig::default().max_size(50 * 1024 * 1024)  // 50MB
        ))
    );

    println!("📤 Upload endpoint: http://localhost:8080/upload");
}
```

### `DiskStorageConfig` Configuration

```rust
use miko::ext::uploader::{DiskStorage, DiskStorageConfig};

let storage = DiskStorage::new(
"uploads",
DiskStorageConfig::default ()
.max_size(10 * 1024 * 1024)  // Max 10MB
.allowed_extensions(vec!["jpg".into(), "png".into(), "pdf".into()])
.allowed_mime_types(vec!["image/jpeg".into(), "image/png".into()])
.filename_mapper( | original_name| {
// Custom filename generation
format ! ("{}_{}", chrono::Utc::now().timestamp(), original_name)
})
);
```

### Using `MultipartResult`

A more flexible way is using the `MultipartResult` extractor:

```rust
use miko::{*, macros::*, extractor::multipart::MultipartResult};

#[post("/upload")]
async fn upload(mut multipart: MultipartResult) -> AppResult<Json<serde_json::Value>> {
    let mut uploaded = vec![];

    // Note: iterating by value to take ownership if read_and_drop_file is needed
    for (field_name, files) in multipart.files {
        for file in files {
            // Validate file type
            if let Some(mime) = &file.content_type {
                if !mime.type_().as_str().starts_with("image/") {
                    return Err(AppError::BadRequest(
                        format!("{} is not an image", file.filename)
                    ));
                }
            }

            // Validate file size
            const MAX_SIZE: usize = 5 * 1024 * 1024;  // 5MB
            if file.size > MAX_SIZE {
                return Err(AppError::BadRequest(
                    format!("{} exceeds 5MB", file.filename)
                ));
            }

            // Save file
            let dest = format!("uploads/{}", file.filename);
            file.linker.transfer_to(&dest).await?;

            uploaded.push(serde_json::json!({
                "field": field_name,
                "filename": file.filename,
                "size": file.size,
                "path": dest,
            }));
        }
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "files": uploaded,
    })))
}
```

### Image Upload Example

```rust
use miko::{*, macros::*, extractor::multipart::MultipartResult};
use image::ImageFormat;

#[post("/upload-image")]
async fn upload_image(mut multipart: MultipartResult) -> AppResult<Json<serde_json::Value>> {
    // Use remove to take ownership of files, as read_and_drop_file consumes them
    if let Some(files) = multipart.files.remove("image") {
        for file in files {
            // Validate MIME type
            if let Some(mime) = &file.content_type {
                if mime.type_().as_str() != "image" {
                    return Err(AppError::BadRequest("Not an image file".into()));
                }
            }

            // Read image and validate
            let bytes = file.linker.read_and_drop_file().await?;
            let img = image::load_from_memory(&bytes)
                .map_err(|e| AppError::BadRequest(format!("Invalid image: {}", e)))?;

            // Generate thumbnail
            let thumbnail = img.resize(200, 200, image::imageops::FilterType::Lanczos3);

            // Save original and thumbnail
            let filename = format!("{}_{}", chrono::Utc::now().timestamp(), file.filename);
            let thumb_filename = format!("thumb_{}", filename);

            tokio::fs::write(format!("uploads/{}", filename), bytes).await?;
            thumbnail.save_with_format(
                format!("uploads/{}", thumb_filename),
                ImageFormat::Jpeg
            )?;

            return Ok(Json(serde_json::json!({
                "success": true,
                "original": filename,
                "thumbnail": thumb_filename,
            })));
        }
    }

    Err(AppError::BadRequest("No image uploaded".into()))
}
```

### Frontend Example

HTML Form:

```html

<form action="/upload" method="POST" enctype="multipart/form-data">
  <input type="file" name="file" accept="image/*" required>
  <button type="submit">Upload</button>
</form>
```

JavaScript Fetch:

```javascript
async function uploadFile(file) {
  const formData = new FormData();
  formData.append('file', file);

  const response = await fetch('/upload', {
    method: 'POST',
    body: formData
  });

  const result = await response.json();
  console.log('Uploaded:', result);
}
```

## Trace ID Tracking

Miko provides an automatic Trace ID system for tracking and correlating requests.

### Automatic Trace ID

All error responses will automatically include a `trace_id` field:

```rust
use miko::*;
use miko::macros::*;

#[get("/error")]
async fn error_handler() -> AppResult<String> {
    Err(AppError::NotFound("Resource not found".into()))
}

// Example Response:
// {
//   "status": 404,
//   "error": "NOT_FOUND",
//   "message": "Resource not found",
//   "trace_id": "550e8400-e29b-41d4-a716-446655440000",
//   "timestamp": "2024-01-01T12:00:00Z"
// }
```

### Trace ID Sources

The framework retrieves the Trace ID according to the following priority:

1. Request header `x-trace-id`
2. Request header `x-request-id`
3. Automatically generated UUID

```bash
# Using custom Trace ID
curl -H "x-trace-id: my-custom-trace-123" http://localhost:8080/api

# Automatically generating Trace ID
curl http://localhost:8080/api
```

### Manually Using Trace ID

You can manually get and set the Trace ID in your code:

```rust
use miko::error::{get_trace_id, set_trace_id};

#[get("/api/data")]
async fn get_data() -> AppResult<String> {
    // Get the Trace ID of the current request
    if let Some(trace_id) = get_trace_id() {
        println!("Processing request: {}", trace_id);

        // Record to log system
        tracing::info!(trace_id = %trace_id, "Fetching data");
    }

    // Business logic
    Ok("Data".to_string())
}
```

### Using Trace ID in Middleware

```rust
use miko::error::{get_trace_id, set_trace_id};
use tower::{Layer, Service};

// Custom middleware to record Trace ID
#[derive(Clone)]
struct TraceLayer;

impl<S> Layer<S> for TraceLayer {
    type Service = TraceMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TraceMiddleware { inner }
    }
}

#[derive(Clone)]
struct TraceMiddleware<S> {
    inner: S,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for TraceMiddleware<S>
where
    S: Service<Request<ReqBody>, Response=Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        // Get or generate Trace ID from request header
        let trace_id = request
            .headers()
            .get("x-trace-id")
            .or_else(|| request.headers().get("x-request-id"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Set to thread-local
        set_trace_id(Some(trace_id.clone()));

        tracing::info!(
            method = %request.method(),
            uri = %request.uri(),
            trace_id = %trace_id,
            "Incoming request"
        );

        self.inner.call(request)
    }
}

// Usage
#[miko]
async fn main() {
    router.layer(TraceLayer);
}
```

### API Documentation

**Trace ID related functions**:

```rust
// Get the current request's Trace ID
pub fn get_trace_id() -> Option<String>

// Set the current request's Trace ID
pub fn set_trace_id(trace_id: Option<String>)

// Clear the current request's Trace ID
pub fn clear_trace_id()
```

## Graceful Shutdown

The Miko framework has a built-in production-grade graceful shutdown mechanism, ensuring that active requests are not
forcibly terminated when the service stops.

### How it Works

Once the application starts, `Application::run` automatically listens for system termination signals (`SIGTERM`/`SIGINT`
on Linux/macOS, `Ctrl+C` on Windows).

When a signal is received:

1. **Stop Accepting New Connections**: The server immediately stops `accept`-ing new TCP connections.
2. **Notify Existing Connections**: A shutdown signal is sent to all connections currently processing requests.
    * For HTTP/1.1, a `Connection: close` header is added to the response.
    * For HTTP/2, a `GOAWAY` frame is sent.
3. **Wait for Request Completion**: The server waits for all active requests to finish processing.
4. **Hard Timeout**: If requests are still incomplete after a default **30-second** period, the server will force a
   shutdown to prevent the process from hanging.

### Example Code

You can write a slow request endpoint to test this feature:

```rust
use miko::*;
use miko::macros::*;
use std::time::Duration;

#[get("/slow")]
async fn slow_handler() -> &'static str {
    // Simulate a time-consuming task
    tokio::time::sleep(Duration::from_secs(5)).await;
    "Task Finished!"
}

#[tokio::main]
async fn main() {
    let router = Router::new().get("/slow", slow_handler);

    println!("Press Ctrl+C to stop the server; active requests will finish processing...");
    Application::new_(router).run().await.unwrap();
}
```

### Verification Method

1. Start the server.

2. Access the `/slow` endpoint.

3. Immediately press `Ctrl+C` in the terminal.

4. You will notice that the server does not exit immediately; it waits for the `/slow` request to return a result before
   shutting down gracefully.

## Panic Handling

> **Requires `catch_panic` feature**



Miko can capture `panic`s that occur in handler functions, preventing the service from crashing and returning a 500 JSON
response instead.

### How to Enable

#### 1. Via Macro Attribute (Recommended)

Add the `catch` attribute to the `#[miko]` macro:

```rust

#[miko(catch)]

async fn main() {

   // Global panic catching is enabled

}

```

#### 2. Manual Enable

If you are not using the macro, you can manually mount the middleware to the Router:

```rust

let mut router = Router::new();

router.with_catch_panic();

```

### Response Format

Once a panic is captured, the client will receive a response like this:

```json

{
   "status": 500,
   "error": "INTERNAL_SERVER_ERROR",
   "message": "Panic occurred: [panic message]",
   "trace_id": "...",
   "timestamp": ...
}

```
