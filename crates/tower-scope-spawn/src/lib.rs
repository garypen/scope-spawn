//!
//! Provide task management to tower services.
//!
//! A Simple Example
//!
//! ```rust
//! //! An example of how to use the `ScopeSpawnLayer`.
//! use std::convert::Infallible;
//! use std::future::Future;
//! use std::pin::Pin;
//! use std::task::Context;
//! use std::task::Poll;
//! use std::time::Duration;
//!
//! use bytes::Bytes;
//! use http_body_util::BodyExt;
//! use http_body_util::Empty;
//! use hyper::server::conn::http1;
//! use hyper::Request;
//! use hyper::Response;
//! use hyper_util::rt::TokioIo;
//! use hyper_util::service::TowerToHyperService;
//! use tokio::net::TcpListener;
//! use tokio::time::sleep;
//! use tower::Service;
//! use tower::ServiceBuilder;
//!
//! use tower_scope_spawn::layer::ScopeSpawnLayer;
//! use tower_scope_spawn::service::WithScope;
//!
//! // A simple tower::Service that processes a request and spawns a background task.
//! #[derive(Clone)]
//! struct MyTowerService;
//!
//! impl Service<WithScope<Request<hyper::body::Incoming>>> for MyTowerService {
//!     type Response = Response<Empty<Bytes>>;
//!     type Error = Infallible;
//!     type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;
//!
//!     fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//!         Poll::Ready(Ok(()))
//!     }
//!
//!     fn call(&mut self, req: WithScope<Request<hyper::body::Incoming>>) -> Self::Future {
//!         let scope = req.scope;
//!         let _original_request = req.request;
//!
//!         Box::pin(async move {
//!             println!("[Handler] Received request. Spawning background task.");
//!
//!             // Here, we use spawn_with_hooks to clearly see the outcome.
//!             scope.spawn_with_hooks(
//!                 async move {
//!                     // This task will be cancelled if the request is dropped.
//!                     for i in 1..=5 {
//!                         println!("[Background] Working... step {} of 5", i);
//!                         sleep(Duration::from_millis(500)).await;
//!                     }
//!                 },
//!                 || println!("[Background] Task finished normally."),
//!                 || println!("[Background] Task was cancelled."),
//!             );
//!
//!             // The handler waits for 3 seconds before sending a response.
//!             // If the user presses Ctrl+C, then they'll seek the task is cancelled.
//!             sleep(Duration::from_secs(3)).await;
//!             println!("[Handler] Sending response.");
//!             Ok(Response::new(Empty::new()))
//!         })
//!     }
//! }
//!
#![warn(
    missing_docs,
    missing_debug_implementations,
    missing_copy_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    unused_qualifications
)]

pub mod layer;
pub mod service;
