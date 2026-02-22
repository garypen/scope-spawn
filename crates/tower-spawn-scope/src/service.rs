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

#[derive(Clone, Debug)]
struct SpawnScopeServiceMetrics {
    tasks: Counter<u64>,
}

#[derive(Clone, Debug)]
// We have to keep the meter hanging around...
#[allow(dead_code)]
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
        self.instruments
            .tasks
            .add(1, &[KeyValue::new("tasks", "printed")]);
        let scope = Scope::new();
        ScopeFuture::new(self.inner.call(req), scope)
    }
}

impl<S> SpawnScopeService<S> {
    pub fn new(inner: S) -> Self {
        let meter = global::meter("spawn_scope_service");
        let instruments = SpawnScopeServiceMetrics {
            tasks: meter.u64_counter("tasks").build(),
        };

        Self {
            inner,
            meter,
            instruments,
        }
    }
}

/// A ScopeFuture. Useful for integrating Scope with [tower](https://docs.rs/tower/latest/tower/), [axum](https://docs.rs/axum/latest/axum), etc..
#[pin_project(PinnedDrop)]
#[derive(Clone, Debug)]
pub struct ScopeFuture<F> {
    #[pin]
    inner: F,
    scope: Scope,
}

impl<F> ScopeFuture<F> {
    pub fn new(inner: F, scope: Scope) -> Self {
        Self { inner, scope }
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

    use super::*;
    use spawn_scope::scope::ScopedSpawn;

    #[tokio::test]
    async fn test_cancellation_on_drop() {
        type TestReq = Request<Empty<Bytes>>;
        type TestRes = ();

        // 1. Setup the mock service
        let (mut mock_service, mut mock_handle) =
            tower_test::mock::spawn_with(|svc: tower_test::mock::Mock<TestReq, TestRes>| {
                SpawnScopeService::new(svc)
            });

        // We only expect one call
        mock_handle.allow(1);

        // 2. Send a request and get the ScopeFuture
        let req = Request::new(Empty::new()); // Request needs a body, Empty::new() is suitable
        tokio_test::assert_ready_ok!(mock_service.poll_ready());
        let fut = mock_service.call(req);

        // 3. Mock service receives the request, but we don't need to extract scope from it
        let (_req, _send_response) = mock_handle.next_request().await.unwrap();
        let scope = fut.scope_ref();

        // 4. Spawn a "background task" in the scope that lasts forever
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        scope.spawn(async move {
            let _guard = tx; // Drops when this task is cancelled
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        });

        // 5. Simulate a client timeout/disconnect by dropping the response future
        drop(fut);

        // 6. Verify the background task was actually killed
        // The receiver will get an error when the sender is dropped.
        // assert!(rx.await.is_err());
        tokio::select! {
            resp = rx => assert!(resp.is_err()),
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                panic!("Task should have been cancelled!");
            }
        }
    }

    #[tokio::test]
    #[should_panic]
    async fn test_no_cancellation_on_no_drop() {
        type TestReq = Request<Empty<Bytes>>;
        type TestRes = ();

        // 1. Setup the mock service
        let (mut mock_service, mut mock_handle) =
            tower_test::mock::spawn_with(|svc: tower_test::mock::Mock<TestReq, TestRes>| {
                SpawnScopeService::new(svc)
            });

        // We only expect one call
        mock_handle.allow(1);

        // 2. Send a request and get the ScopeFuture
        let req = Request::new(Empty::new()); // Request needs a body, Empty::new() is suitable
        tokio_test::assert_ready_ok!(mock_service.poll_ready());
        let fut = mock_service.call(req);

        // 3. Mock service receives the request, but we don't need to extract scope from it
        let (_req, _send_response) = mock_handle.next_request().await.unwrap();
        let scope = fut.scope_ref();

        // 4. Spawn a "background task" in the scope that lasts forever
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        scope.spawn(async move {
            let _guard = tx; // Drops when this task is cancelled
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        });

        // 5. Don't simulate a client timeout/disconnect by dropping the response future

        // 6. Verify the background task was actually killed
        // The receiver will get an error when the sender is dropped.
        // assert!(rx.await.is_err());
        tokio::select! {
            resp = rx => assert!(resp.is_err()),
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                panic!("Task should have been cancelled!");
            }
        }
    }
}
