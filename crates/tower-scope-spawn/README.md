# tower-scope-spawn

`tower-scope-spawn` is a Tower layer that integrates request-scoped task management into your services. It leverages the `scope-spawn` crate to provide a mechanism for spawning background tasks that are automatically cancelled when the associated request's processing is complete or the connection is dropped.

This is particularly useful for:

-   **Request-specific cleanup:** Ensuring resources tied to a specific request are released when the request lifecycle ends.
-   **Structured concurrency:** Managing background tasks with clear ownership and cancellation semantics.
-   **Avoiding resource leaks:** Preventing long-running tasks from outliving the request that initiated them.

## How it works

The `SpawnScopeLayer` wraps your service, injecting a `scope_spawn::scope::Scope` into each incoming request. This scope can then be used within your service's logic to spawn `tokio` tasks. The key feature is that when the `tower::Service::call` future for a request completes or is dropped (e.g., due to a client disconnect or timeout), the associated `Scope` is automatically cancelled, and all tasks spawned within that scope are gracefully terminated.

## Example

To run this example, navigate to the `crates/tower-scope-spawn` directory and execute:

```bash
cargo run --example simple_service
```

Then, you can send an HTTP request to `http://127.0.0.1:3000` using `curl` or your browser:

```bash
curl http://127.0.0.1:3000
```

You will observe the background task messages in your terminal. If you cancel the `curl` command mid-way (e.g., by pressing `Ctrl+C` before it finishes), you will see that the background task is immediately cancelled.

```rust
use std::convert::Infallible;
use std::time::Duration;
use bytes::Bytes;
use http_body_util::Empty;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use tokio::net::TcpListener;
use tokio::time::sleep;
use tower::{Service, ServiceBuilder};
use tower_scope_spawn::layer::SpawnScopeLayer;
use tower_scope_spawn::service::WithScope;

// A simple service that processes a request and spawns a background task.
async fn my_service(req: WithScope<Request<hyper::body::Incoming>>) -> Result<Response<Empty<Bytes>>, Infallible> {
    let scope = req.scope;
    let _original_request = req.request;

    println!("Service received request. Spawning background task...");

    scope.spawn(async move {
        // This task will be cancelled if the request scope is dropped
        for i in 1..=5 {
            println!("Background task working... step {}", i);
            sleep(Duration::from_millis(500)).await;
        }
        println!("Background task finished normally.");
    });

    println!("Service sending response immediately.");
    Ok(Response::new(Empty::new()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = "127.0.0.1:3000";
    let listener = TcpListener::bind(addr).await?;
    println!("Listening on http://{}", addr);

    // Build our service with the SpawnScopeLayer
    let service = ServiceBuilder::new()
        .layer(SpawnScopeLayer::new())
        .service(service_fn(my_service));

    loop {
        let (stream, _) = listener.accept().await?;
        let service = service.clone(); // Clone the service for each connection
        tokio::task::spawn(async move {
            if let Err(err) = http1::Builder::new()
                .serve_connection(stream, service)
                .await
            {
                eprintln!("Error serving connection: {}", err);
            }
        });
    }
}
```
