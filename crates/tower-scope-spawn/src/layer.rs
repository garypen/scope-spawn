//!
//! A Scoped Layer
//!

use tower::Layer;

use crate::service::ScopeSpawnService;

/// Applies Spawn Scope to requests.
#[derive(Copy, Clone, Debug, Default)]
pub struct ScopeSpawnLayer {}

impl ScopeSpawnLayer {
    /// Create a ScopeSpawnLayer
    pub fn new() -> Self {
        ScopeSpawnLayer {}
    }
}

impl<S> Layer<S> for ScopeSpawnLayer {
    type Service = ScopeSpawnService<S>;

    fn layer(&self, service: S) -> Self::Service {
        ScopeSpawnService::new(service)
    }
}

#[cfg(test)]
mod tests {
    use axum::http::Request;
    use bytes::Bytes;
    use http_body_util::Empty;
    use tower_test::mock;

    use super::*;
    use crate::service::ScopeFuture;

    // Not Need with Layer tests, but left as documentation
    // type TestReq = Request<Empty<Bytes>>;
    type TestRes = ();

    #[tokio::test]
    async fn test_cancellation_on_drop() {
        // Setup the mock service, which now expects WithScope<TestReq>
        let (mut mock_service, mut mock_handle) = mock::spawn_layer(ScopeSpawnLayer::new());

        // We only expect one call
        mock_handle.allow(1);

        // Send a request and get the ScopeFuture
        let req = Request::new(Empty::<Bytes>::new()); // Original request type
        tokio_test::assert_ready_ok!(mock_service.poll_ready());
        let fut: ScopeFuture<mock::future::ResponseFuture<TestRes>> = mock_service.call(req);

        // Mock service receives the request as WithScope<TestReq>
        let (with_scope_req, _send_response) = mock_handle.next_request().await.unwrap();
        let _inner_req = with_scope_req.request; // The original request
        let _inner_service_scope = with_scope_req.scope; // The scope passed to the inner service

        // The scope from ScopeFuture, which is responsible for cancellation upon fut drop
        let scope_from_fut = fut.scope();

        // Spawn a "background task" in the scope that lasts forever
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        scope_from_fut.spawn(async move {
            let _guard = tx; // Drops when this task is cancelled
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        });

        // Simulate a client timeout/disconnect by dropping the response future
        drop(fut);

        // Verify the background task was actually killed
        // The receiver will get an error when the sender is dropped.
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
        // Setup the mock service, which now expects WithScope<TestReq>
        let (mut mock_service, mut mock_handle) = mock::spawn_layer(ScopeSpawnLayer::new());

        // We only expect one call
        mock_handle.allow(1);

        // Send a request and get the ScopeFuture
        let req = Request::new(Empty::<Bytes>::new()); // Original request type
        tokio_test::assert_ready_ok!(mock_service.poll_ready());
        let fut: ScopeFuture<mock::future::ResponseFuture<TestRes>> = mock_service.call(req);

        // Mock service receives the request as WithScope<TestReq>
        let (with_scope_req, _send_response) = mock_handle.next_request().await.unwrap();
        let _inner_req = with_scope_req.request; // The original request
        let _inner_service_scope = with_scope_req.scope; // The scope passed to the inner service

        // The scope from ScopeFuture, which is responsible for cancellation upon fut drop
        let scope_from_fut = fut.scope();

        // Spawn a "background task" in the scope that lasts forever
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        scope_from_fut.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            // We won't get here because our tokio::select is too impatient
            let _ = tx.send(());
        });

        // Don't simulate a client timeout/disconnect by dropping the response future
        // (i.e., don't drop 'fut')

        // Verify the background task was not killed
        // The receiver will get an error when the sender is dropped.
        tokio::select! {
            resp = rx => assert!(resp.is_err()),
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                panic!("Task was not cancelled!");
            }
        }
    }
}
