use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::metrics::Counter;
use opentelemetry::metrics::Meter;
use pin_project::pin_project;
use pin_project::pinned_drop;
use tower::BoxError;
use tower::Service;

use spawn_scope::scope::Scope;
use spawn_scope::scope::ScopedSpawn;

#[derive(Clone, Debug)]
struct SpawnScopeServiceMetrics {
    early_wake: Counter<u64>,
}

#[derive(Clone, Debug)]
pub struct SpawnScopeService<S> {
    inner: S,
    meter: Meter,
    instruments: SpawnScopeServiceMetrics,
}

impl<S, Req> Service<Req> for SpawnScopeService<S>
where
    S: Service<Req, Error = BoxError>,
    Req: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ScopeFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        ScopeFuture::new(self.inner.call(req))
    }
}

impl<S> SpawnScopeService<S> {
    pub fn new(inner: S) -> Self {
        let meter = global::meter("rate_limit_service");
        let instruments = SpawnScopeServiceMetrics {
            early_wake: meter.u64_counter("early_wake").build(),
        };

        Self {
            inner,
            meter: global::meter("spawn_scope_service"),
            instruments,
        }
    }
}

/// A ScopeFuture. Useful for integrating Scope with [tower](https://docs.rs/tower/latest/tower/), [axum](https://docs.rs/axum/latest/axum), etc..
#[pin_project(PinnedDrop)]
pub struct ScopeFuture<F> {
    #[pin]
    inner: F,
    scope: Scope,
}

impl<F> ScopeFuture<F> {
    pub fn new(inner: F) -> Self {
        Self {
            inner,
            scope: Scope::new(),
        }
    }

    pub fn scope_ref(&self) -> &Scope {
        &self.scope
    }
}

impl<F: Future> Future for ScopeFuture<F> {
    type Output = F::Output;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

#[pinned_drop]
impl<F> PinnedDrop for ScopeFuture<F> {
    fn drop(self: Pin<&mut Self>) {
        self.project().scope.cancel();
    }
}

#[cfg(test)]
mod tests {
    use axum::http::Request;
    use bytes::Bytes;
    use http_body_util::Empty;
    use tower::ServiceExt;
    use tower_test::mock::{self, Handle};

    use super::*;
    use crate::layer::SpawnScopeLayer; // No longer needed if ServiceBuilder is bypassed

    #[tokio::test]
    async fn test_cancellation_on_drop() {
        type TestReq = Request<Empty<Bytes>>;
        type TestRes = ();

        // 1. Setup the mock service and create SpawnScopeService directly
        let (inner_mock_service, mock_handle) = mock::spawn::<TestReq, TestRes>();
        let mut service = SpawnScopeService::new(inner_mock_service);

        // 2. Send a request and get the ScopeFuture
        let req = Request::new(Empty::new()); // Request needs a body, Empty::new() is suitable
        let fut = service.call(req);

        // 3. Mock service receives the request, but we don't need to extract scope from it
        let (_req, _send_response) = mock_handle.next_request().await.unwrap();
        let scope = fut.scope_ref();

        // 4. Spawn a "background task" in the scope that lasts forever
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        scope.spawn(async move {
            let _guard = tx; // Drops when this task is cancelled
            tokio::time::sleep(std::time::Duration::from_secs(100)).await;
        });

        // 5. Simulate a client timeout/disconnect by dropping the response future
        drop(fut);

        // 6. Verify the background task was actually killed
        // Since the task was cancelled, the sender 'tx' is dropped, so 'rx' returns None.
        tokio::select! {
            _ = rx.recv() => panic!("Task should have been cancelled!"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                // Task successfully reaped
            }
        }
    }
}
