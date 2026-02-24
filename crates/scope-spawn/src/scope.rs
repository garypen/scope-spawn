//!
//! Implementation of Scope
//!

use tokio::task::JoinHandle;
use tokio_util::future::FutureExt;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// A Scope for spawning Tokio Tasks.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    token: CancellationToken,
    tracker: TaskTracker,
}

impl Scope {
    /// Create a new scoped task spawner
    ///
    /// Tasks may be cancelled manually, using `cancel()`, or automatically
    /// when the Scope is dropped.
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            tracker: TaskTracker::new(),
        }
    }

    /// Cancel all spawned tasks.
    ///
    /// Note: Spawned tasks will be cancelled when the scope is dropped.
    /// However, if you wish to cancel them sooner, you can.
    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// Spawn a task on a tokio runtime.
    ///
    /// ```
    /// use scope_spawn::scope::Scope;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let scope = Scope::new();
    ///     scope.spawn(async {
    ///         println!("Hello from a spawned task!");
    ///     });
    ///     // scope is dropped here, and spawned tasks are cancelled.
    /// }
    /// ```
    pub fn spawn<F, R>(&self, future: F) -> JoinHandle<Option<F::Output>>
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        let token = self.token.clone();
        self.tracker
            .spawn(async move { future.with_cancellation_token_owned(token).await })
    }

    /// Spawn a task on a tokio runtime with associated post execution tasks.
    ///
    /// If the task is cancelled, the provided cleanup function will execute.
    /// If the task completes, the provided complete function will execute.
    ///
    /// ```
    /// use scope_spawn::scope::Scope;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let scope = Scope::new();
    ///     scope.spawn_with_hooks(async {
    ///         println!("Hello from a spawned task!");
    ///     }, || { println!("called if cancelled");
    ///     }, || { println!("called if completed"); });
    ///     // scope is dropped here, and spawned tasks are cancelled.
    /// }
    /// ```
    pub fn spawn_with_hooks<F, C, D, R>(
        &self,
        future: F,
        on_completion: C,
        on_cancellation: D,
    ) -> JoinHandle<Option<F::Output>>
    where
        F: Future<Output = R> + Send + 'static,
        C: FnOnce() + Send + 'static,
        D: FnOnce() + Send + 'static,
        R: Send + 'static,
    {
        let token = self.token.clone();
        self.tracker.spawn(async move {
            match future.with_cancellation_token_owned(token).await {
                Some(r) => {
                    on_completion();
                    Some(r)
                }
                None => {
                    on_cancellation();
                    None
                }
            }
        })
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::http::StatusCode;
    use axum::routing::get;
    use tokio::sync::oneshot;
    use tokio::time;
    use tokio_util::sync::CancellationToken;
    use tower::Service;

    use super::*;

    #[tokio::test]
    async fn it_returns_stuff() {
        let scope = Scope::new();
        let (tx, rx) = oneshot::channel::<()>();

        let jh = scope.spawn(async move {
            // This task will complete when cancelled.
            // The oneshot sender will be dropped.
            let _tx = tx;
            panic!("to panic is human");
            pending::<()>().await;
            45
        });

        // INTERESTING. IF WE CANCEL, we get NONE, if we don't cancel
        // we get
        // thread 'scope::tests::it_returns_stuff' (6677631) panicked at crates/scope-spawn/src/scope.rs:170:48:
        // first jh: JoinError::Panic(Id(3), "to panic is human", ...)
        // Hmm, actually it's racy. If we panic before we cancel we get panic.
        // Putting in a sleep here guarantees we get panic even if we try to drop because panic
        // happens before the drop

        time::sleep(Duration::from_millis(50)).await;
        drop(scope);
        println!("first await: {:?}", jh.await.expect_err("first jh"));

        // The receiver will get an error when the sender is dropped.
        assert!(rx.await.is_err());
    }

    #[tokio::test]
    async fn it_works() {
        let scope = Scope::new();
        let (tx, rx) = oneshot::channel::<()>();

        scope.spawn(async move {
            // This task will complete when cancelled.
            // The oneshot sender will be dropped.
            let _tx = tx;
            pending::<()>().await;
        });

        drop(scope);

        // The receiver will get an error when the sender is dropped.
        assert!(rx.await.is_err());
    }

    #[tokio::test]
    async fn test_scope_cancellation_with_tokio() {
        let scope = Scope::new();
        let started = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));

        let s_clone = started.clone();
        let c_clone = cancelled.clone();

        scope.spawn_with_hooks(
            async move {
                s_clone.store(true, Ordering::SeqCst);
                // This task will complete when cancelled.
                pending::<()>().await;
            },
            || (),
            move || {
                c_clone.store(true, Ordering::SeqCst);
            },
        );

        // Give the spawned task a moment to start
        time::sleep(Duration::from_millis(50)).await;
        assert!(started.load(Ordering::SeqCst));

        drop(scope);

        // Give the cancellation a moment to propagate
        time::sleep(Duration::from_millis(50)).await;
        assert!(cancelled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_scope_with_tower() {
        let token = CancellationToken::new();

        let handler = {
            let token = token.clone();
            move || async move {
                let scope = Scope::new();
                scope.spawn(async move {
                    pending::<()>().await;
                });
                token.cancelled().await;
            }
        };

        let mut service = Router::new().route("/", get(handler)).into_service();

        let request = Request::new(Body::empty());

        let response_future = service.call(request);

        let response_handle = tokio::spawn(response_future);

        // Give the handler time to start
        time::sleep(Duration::from_millis(50)).await;

        token.cancel();

        // The handler should complete, dropping the scope, cancelling the task.
        let response = response_handle.await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_spawn_with_hooks_completion() {
        let scope = Scope::new();
        let completed = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));

        let comp_clone = completed.clone();
        let canc_clone = cancelled.clone();

        scope.spawn_with_hooks(
            async move {
                // Task completes normally
            },
            move || {
                comp_clone.store(true, Ordering::SeqCst);
            },
            move || {
                canc_clone.store(true, Ordering::SeqCst);
            },
        );

        // Give the spawned task a moment to complete
        time::sleep(Duration::from_millis(50)).await;
        assert!(completed.load(Ordering::SeqCst));
        assert!(!cancelled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_multiple_tasks() {
        let scope = Scope::new();
        let count = Arc::new(AtomicUsize::new(0));

        for _ in 0..10 {
            let c = count.clone();
            scope.spawn_with_hooks(
                async move {
                    pending::<()>().await;
                },
                || (),
                move || {
                    c.fetch_add(1, Ordering::SeqCst);
                },
            );
        }

        // Give tasks time to start (though they just pend)
        time::sleep(Duration::from_millis(50)).await;
        assert_eq!(count.load(Ordering::SeqCst), 0);

        drop(scope);

        // Give cancellation time to propagate
        time::sleep(Duration::from_millis(50)).await;
        assert_eq!(count.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn test_scope_manual_cancel() {
        let scope = Scope::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let c_clone = cancelled.clone();

        scope.spawn_with_hooks(
            async move {
                pending::<()>().await;
            },
            || (),
            move || {
                c_clone.store(true, Ordering::SeqCst);
            },
        );

        scope.cancel();

        // Give cancellation time to propagate
        time::sleep(Duration::from_millis(50)).await;
        assert!(cancelled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_scope_clone_shares_lifecycle() {
        let scope = Scope::new();
        let scope_clone = scope.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let c_clone = cancelled.clone();

        scope_clone.spawn_with_hooks(
            async move {
                pending::<()>().await;
            },
            || (),
            move || {
                c_clone.store(true, Ordering::SeqCst);
            },
        );

        // Dropping the original scope should cancel tasks in the clone
        drop(scope);

        // Give cancellation time to propagate
        time::sleep(Duration::from_millis(50)).await;
        assert!(cancelled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_spawn_after_cancel() {
        let scope = Scope::new();
        scope.cancel();

        let cancelled = Arc::new(AtomicBool::new(false));
        let c_clone = cancelled.clone();

        scope.spawn_with_hooks(
            async move {
                pending::<()>().await;
            },
            || (),
            move || {
                c_clone.store(true, Ordering::SeqCst);
            },
        );

        // It should be cancelled immediately
        time::sleep(Duration::from_millis(50)).await;
        assert!(cancelled.load(Ordering::SeqCst));
    }
}
