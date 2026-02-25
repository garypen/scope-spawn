//!
//! A Scoped Service
//!

use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use pin_project::pin_project;
use pin_project::pinned_drop;
use tower::Service;

use scope_spawn::scope::Scope;

/// Request wrapper
#[derive(Debug)]
pub struct WithScope<Req> {
    /// The original Request wrapped in this scope
    pub request: Req,
    /// The Scope of the wrapped request
    pub scope: Scope,
}

/// A spawn scope service.
#[derive(Clone, Debug)]
pub struct ScopeSpawnService<S> {
    inner: S,
}

impl<S, Req> Service<Req> for ScopeSpawnService<S>
where
    S: Service<WithScope<Req>>, // Inner service expects WithScope<Req>
    Req: Send + 'static,
    S::Error: 'static, // Ensure S::Error is static
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ScopeFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let scope = Scope::new();
        // The scope clone is passed to the inner request AND kept by ScopeFuture
        let inner_req_with_scope = WithScope {
            request: req,
            scope: scope.clone(),
        };
        let inner_future = self.inner.call(inner_req_with_scope);
        ScopeFuture::new(inner_future, scope) // ScopeFuture retains its own clone for cancellation
    }
}

impl<S> ScopeSpawnService<S> {
    /// Create a new ScopeSpawnService
    pub fn new(inner: S) -> Self {
        Self { inner }
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
    /// Create a new ScopeFuture
    pub fn new(inner: F, scope: Scope) -> Self {
        Self { inner, scope }
    }

    /// Borrow the scope
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
}

impl<F: Future> Future for ScopeFuture<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
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
    use std::time::Duration;

    use axum::http::Request;
    use bytes::Bytes;
    use http_body_util::Empty;
    use tokio::sync::oneshot::channel;
    use tokio::time::sleep;

    use super::*;

    type TestReq = Request<Empty<Bytes>>;
    type TestRes = ();

    #[tokio::test]
    async fn test_cancellation_on_drop() {
        // Setup the mock service, which now expects WithScope<TestReq>
        let (mut mock_service, mut mock_handle) = tower_test::mock::spawn_with(
            |svc: tower_test::mock::Mock<WithScope<TestReq>, TestRes>| ScopeSpawnService::new(svc),
        );

        // We only expect one call
        mock_handle.allow(1);

        // Send a request and get the ScopeFuture
        let req = Request::new(Empty::new()); // Original request type
        tokio_test::assert_ready_ok!(mock_service.poll_ready());
        let fut = mock_service.call(req);

        // Mock service receives the request as WithScope<TestReq>
        let (with_scope_req, _send_response) = mock_handle.next_request().await.unwrap();
        let _inner_req = with_scope_req.request; // The original request
        let _inner_service_scope = with_scope_req.scope; // The scope passed to the inner service

        // The scope from ScopeFuture, which is responsible for cancellation upon fut drop
        let scope_from_fut = fut.scope();

        // Spawn a "background task" in the scope that lasts forever
        let (tx, rx) = channel::<()>();
        scope_from_fut.spawn(async move {
            let _guard = tx; // Drops when this task is cancelled
            sleep(Duration::from_millis(200)).await;
        });

        // Simulate a client timeout/disconnect by dropping the response future
        drop(fut);

        // Verify the background task was actually killed
        // The receiver will get an error when the sender is dropped.
        tokio::select! {
            resp = rx => assert!(resp.is_err()),
            _ = sleep(Duration::from_millis(100)) => {
                panic!("Task should have been cancelled!");
            }
        }
    }

    #[tokio::test]
    #[should_panic]
    async fn test_no_cancellation_on_no_drop() {
        // Setup the mock service, which now expects WithScope<TestReq>
        let (mut mock_service, mut mock_handle) = tower_test::mock::spawn_with(
            |svc: tower_test::mock::Mock<WithScope<TestReq>, TestRes>| ScopeSpawnService::new(svc),
        );

        // We only expect one call
        mock_handle.allow(1);

        // Send a request and get the ScopeFuture
        let req = Request::new(Empty::new()); // Original request type
        tokio_test::assert_ready_ok!(mock_service.poll_ready());
        let fut = mock_service.call(req);

        // Mock service receives the request as WithScope<TestReq>
        let (with_scope_req, _send_response) = mock_handle.next_request().await.unwrap();
        let _inner_req = with_scope_req.request; // The original request
        let _inner_service_scope = with_scope_req.scope; // The scope passed to the inner service

        // The scope from ScopeFuture, which is responsible for cancellation upon fut drop
        let scope_from_fut = fut.scope();

        // Spawn a "background task" in the scope that lasts forever
        let (tx, rx) = channel::<()>();
        scope_from_fut.spawn(async move {
            sleep(Duration::from_millis(200)).await;
            // We won't get here because our tokio::select is too impatient
            let _ = tx.send(());
        });

        // Don't simulate a client timeout/disconnect by dropping the response future
        // (i.e., don't drop 'fut')

        // Verify the background task was not killed
        // The receiver will get an error when the sender is dropped.
        tokio::select! {
            resp = rx => assert!(resp.is_err()),
            _ = sleep(Duration::from_millis(100)) => {
                panic!("Task was not cancelled!");
            }
        }
    }

    #[tokio::test]
    async fn test_cancellation_on_completion() {
        let (mut mock_service, mut mock_handle) = tower_test::mock::spawn_with(
            |svc: tower_test::mock::Mock<WithScope<TestReq>, TestRes>| ScopeSpawnService::new(svc),
        );

        mock_handle.allow(1);

        let req = Request::new(Empty::new());
        tokio_test::assert_ready_ok!(mock_service.poll_ready());
        let fut = mock_service.call(req);

        let (with_scope_req, send_response) = mock_handle.next_request().await.unwrap();
        let scope_from_service = with_scope_req.scope;

        let (tx, rx) = channel::<()>();
        scope_from_service.spawn(async move {
            let _guard = tx;
            sleep(Duration::from_millis(200)).await;
        });

        // Complete the request
        send_response.send_response(());

        // The future should resolve
        let _res = fut.await;

        // After the future is dropped (which it is here), the task should be cancelled
        tokio::select! {
            resp = rx => assert!(resp.is_err()),
            _ = sleep(Duration::from_millis(100)) => {
                panic!("Task should have been cancelled after completion!");
            }
        }
    }
}
