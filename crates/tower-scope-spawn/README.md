# tower-scope-spawn

`tower-scope-spawn` is a Tower layer for request-scoped task management. It uses the `scope-spawn` crate to spawn background tasks that are automatically cancelled when the request is fully handled or the client disconnects.

This is useful for structured concurrency, preventing resource leaks by ensuring that tasks do not outlive the request that spawned them.

## How it works

The `SpawnScopeLayer` wraps your service. For each incoming request, it:
1. Creates a new `scope_spawn::scope::Scope`.
2. Wraps the `Request` in a `WithScope` struct, which includes the new scope.
3. Passes the `WithScope<Request>` to your inner service.

When the `tower::Service::call` future for the request is dropped (e.g., client disconnect), the associated `Scope` is cancelled, automatically terminating all tasks spawned within it.

## Example

The example below shows a `tower::Service` that spawns a background task. The task's lifecycle is tied to the request.

To run this example:
```bash
cargo run --example simple_service
```

Then, test the two scenarios in a separate terminal:

1.  **Normal Completion**: The background task finishes because the client waits.
    ```bash
    curl http://127.0.0.1:3000
    ```

2.  **Cancellation**: The background task is cancelled because the client disconnects early.
    ```bash
    curl http://127.0.0.1:3000
    # Press Ctrl+C immediately
    ```

```rust
//! An example of how to use the `SpawnScopeLayer`.
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::BodyExt;
use http_body_util::Empty;
use hyper::server::conn::http1;
use hyper::Request;
use hyper::Response;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio::time::sleep;
use tower::Service;
use tower::ServiceBuilder;

use tower_scope_spawn::layer::SpawnScopeLayer;
use tower_scope_spawn::service::WithScope;

// A simple tower::Service that processes a request and spawns a background task.
#[derive(Clone)]
struct MyTowerService;

impl Service<WithScope<Request<hyper::body::Incoming>>> for MyTowerService {
    type Response = Response<Empty<Bytes>>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: WithScope<Request<hyper::body::Incoming>>) -> Self::Future {
        let scope = req.scope;
        let _original_request = req.request;

        Box::pin(async move {
            println!("[Handler] Received request. Spawning background task.");

            // Here, we use spawn_with_hooks to clearly see the outcome.
            scope.spawn_with_hooks(
                async move {
                    // This task will be cancelled if the request is dropped.
                    for i in 1..=5 {
                        println!("[Background] Working... step {}", i);
                        sleep(Duration::from_secs(1)).await;
                    }
                },
                || println!("[Background] Task finished normally."),
                || println!("[Background] Task was cancelled."),
            );

            // The handler returns a response immediately, without waiting for the
            // background task to complete.
            println!("[Handler] Sending response.");
            Ok(Response::new(Empty::new()))
        })
    }
}
```
