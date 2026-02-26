//!
//! Implementation of Scope
//!

use tokio::task::JoinHandle;
use tokio_util::future::FutureExt;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// A Scope for spawning Tokio Tasks.
///
/// When the Scope is dropped, any tasks which are still executing are cancelled.
/// If you want to cancel all tasks sooner, you can use [Scope::cancel()].
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

    /// Cancel all the spawned, and still executing, tasks.
    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// Spawn a task within the scope.
    ///
    /// This function is the primary way to introduce concurrency within a `Scope`.
    /// The spawned task will be cancelled automatically when the `Scope` is dropped
    /// or when `Scope::cancel()` is called.
    ///
    /// # When to use `spawn`
    ///
    /// Use `spawn` when you need to react to the outcome of the spawned task.
    /// It returns a `JoinHandle<Option<Output>>`, allowing you to `.await` the
    /// result. The `Option` will be:
    ///
    /// - `Some(value)` if the future completes successfully.
    /// - `None` if the future is cancelled before completion.
    ///
    /// If you only need to run a side-effect on completion or cancellation (like
    /// decrementing a counter), consider using [Scope::spawn_with_hooks] for a more
    /// direct API.
    ///
    /// ```
    /// use scope_spawn::scope::Scope;
    /// use std::time::Duration;
    /// use tokio::time::sleep;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let scope = Scope::new();
    ///
    ///     let handle = scope.spawn(async {
    ///         // Simulate some work
    ///         sleep(Duration::from_millis(10)).await;
    ///         "Hello from a spawned task!"
    ///     });
    ///
    ///     let result = handle.await.unwrap();
    ///     assert!(result.is_some());
    ///     assert_eq!(result.unwrap(), "Hello from a spawned task!");
    ///
    ///     // scope is dropped here, and any remaining tasks are cancelled.
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

    /// Spawn a "fire-and-forget" task with completion and cancellation hooks.
    ///
    /// This function is useful when you need to execute a side-effect based on the
    /// task's outcome, but do not need to handle its return value directly.
    ///
    /// - The `on_completion` closure runs if the task finishes successfully.
    /// - The `on_cancellation` closure runs if the task is cancelled.
    ///
    /// # When to use `spawn_with_hooks`
    ///
    /// This method is ideal for managing resources, such as semaphores or counters,
    /// that are tied to the lifecycle of a task. For example, you might decrement
    /// an "in-flight requests" counter in both hooks to ensure it's always accurate,
    /// regardless of how the task terminates.
    ///
    /// If you need to `await` the task's result, use [Scope::spawn] instead.
    ///
    /// ```
    /// use scope_spawn::scope::Scope;
    /// use std::sync::Arc;
    /// use std::sync::atomic::AtomicUsize;
    /// use std::sync::atomic::Ordering;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let scope = Scope::new();
    ///     let completed_count = Arc::new(AtomicUsize::new(0));
    ///
    ///     let count_clone = completed_count.clone();
    ///     scope.spawn_with_hooks(
    ///         async { /* ... */ },
    ///         move || { count_clone.fetch_add(1, Ordering::SeqCst); },
    ///         || { /* handle cancellation */ }
    ///     );
    ///
    ///     // Give the task time to complete.
    ///     tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    ///
    ///     assert_eq!(completed_count.load(Ordering::SeqCst), 1);
    ///     // scope is dropped here, and spawned tasks are cancelled.
    /// }
    /// ```
    pub fn spawn_with_hooks<F, C, D, R>(&self, future: F, on_completion: C, on_cancellation: D)
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
        });
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
    async fn it_correctly_processes_spawned_panic() {
        let scope = Scope::new();
        let (tx, rx) = oneshot::channel::<()>();

        let jh = scope.spawn(async move {
            // This task will complete when cancelled.
            // The oneshot sender will be dropped.
            let _tx = tx;
            panic!("to panic is human");
        });

        // Putting in a sleep here guarantees we get panic even if we try to drop because panic
        // happens before the drop
        time::sleep(Duration::from_millis(50)).await;

        drop(scope);

        let result = jh.await; // We expect an error because we panicked before we dropped

        assert!(result.is_err());
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
