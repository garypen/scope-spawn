use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
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

use tower_scope_spawn::layer::SpawnScopeLayer;
use tower_scope_spawn::service::WithScope;

// A simple service that processes a request and spawns a background task.
// This struct implements the `Service` trait, expecting a `WithScope` request.
#[derive(Clone)]
struct MyHyperService;

impl Service<WithScope<Request<hyper::body::Incoming>>> for MyHyperService {
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
            println!("Service received request. Spawning background task...");

            scope.spawn(async move {
                // This task will be cancelled if the request scope is dropped
                for i in 1..=5 {
                    println!("Background task working... step {}", i);
                    sleep(Duration::from_millis(500)).await;
                }
                println!("Background task finished normally.");
            });

            // We'll sleep for 2 seconds, so we will see 4 Background
            // notifications.
            sleep(Duration::from_millis(2000)).await;
            println!("Service sending response immediately.");
            Ok(Response::new(Empty::new()))
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = "127.0.0.1:3000";
    let listener = TcpListener::bind(addr).await?;
    println!("Listening on http://{}", addr);

    // Build our service with the SpawnScopeLayer
    let tower_service = ServiceBuilder::new()
        .layer(SpawnScopeLayer::new())
        .service(MyHyperService); // This is a tower::Service

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let service = TowerToHyperService::new(tower_service.clone()); // Convert to hyper::service::Service
        tokio::task::spawn(async move {
            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                eprintln!("Error serving connection: {}", err);
            }
        });
    }
}
