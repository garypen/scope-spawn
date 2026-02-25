//! An example of how to use the `ScopeSpawnLayer`.
//!
//! To run this example, you can use `curl`:
//!
//! 1. In one terminal, run the example: `cargo run --example simple_service`
//!
//! 2. In a second terminal, send a request:
//!    - To see normal completion: `curl http://127.0.0.1:3000`
//!      (The server will log the background task finishing)
//!
//!    - To see cancellation: `curl http://127.0.0.1:3000` and then press `Ctrl+C`
//!      before 3 seconds have passed.
//!      (The server will log that the background task was cancelled)

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Empty;
use hyper::Request;
use hyper::Response;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio::time::sleep;
use tower::Service;
use tower::ServiceBuilder;

use tower_scope_spawn::layer::ScopeSpawnLayer;
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
                        println!("[Background] Working... step {} of 5", i);
                        sleep(Duration::from_millis(500)).await;
                    }
                },
                || println!("[Background] Task finished normally."),
                || println!("[Background] Task was cancelled."),
            );

            // The handler waits for 3 seconds before sending a response.
            // If the user presses Ctrl+C, then they'll seek the task is cancelled.
            sleep(Duration::from_secs(3)).await;
            println!("[Handler] Sending response.");
            Ok(Response::new(Empty::new()))
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = "127.0.0.1:3000";
    let listener = TcpListener::bind(addr).await?;
    println!("Listening on http://{}", addr);
    println!("Try running `curl {}` in another terminal.", addr);

    // Build our service with the ScopeSpawnLayer.
    // This layer wraps our service and provides the `Scope` to it.
    let tower_service = ServiceBuilder::new()
        .layer(ScopeSpawnLayer::new())
        .service(MyTowerService);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let service = TowerToHyperService::new(tower_service.clone());
        tokio::task::spawn(async move {
            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                // This error often happens when the client disconnects, which is expected.
                eprintln!("Error serving connection: {}", err);
            }
        });
    }
}
