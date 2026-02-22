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
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub trait ScopedSpawn {
    /// Spawn a task on a tokio runtime.
    ///
    /// ```
    /// use spawn_scope::scope::Scope;
    /// use spawn_scope::scope::ScopedSpawn;
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
    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static;

    /// Spawn a task on a tokio runtime with associated post execution tasks.
    ///
    /// If the task is cancelled, the provided cleanup function will execute.
    /// If the task completes, the provided complete function will execute.
    ///
    /// ```
    /// use spawn_scope::scope::Scope;
    /// use spawn_scope::scope::ScopedSpawn;
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
    fn spawn_with_hooks<F, C, D>(&self, future: F, on_completion: C, on_cancellation: D)
    where
        F: Future<Output = ()> + Send + 'static,
        C: FnOnce() + Send + 'static,
        D: FnOnce() + Send + 'static;
}

impl ScopedSpawn for Scope {
    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let token = self.token.clone();
        let task = async move {
            tokio::select! {
                _ = token.cancelled() => {
                    // Task cancelled
                }
                _ = future => {
                    // Task completed
                }
            }
        };
        self.tracker.spawn(task);
    }

    fn spawn_with_hooks<F, C, D>(&self, future: F, on_completion: C, on_cancellation: D)
    where
        F: Future<Output = ()> + Send + 'static,
        C: FnOnce() + Send + 'static,
        D: FnOnce() + Send + 'static,
    {
        let token = self.token.clone();
        let task = async move {
            tokio::select! {
                _ = token.cancelled() => {
                    // Task cancelled
                    on_cancellation();
                }
                _ = future => {
                    // Task completed
                    on_completion();
                }
            }
        };
        self.tracker.spawn(task);
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
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
}
