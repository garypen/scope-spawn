# spawn-scope Monorepo

This monorepo contains two related crates for managing asynchronous tasks within a defined scope, particularly useful in the context of `tokio` and `tower`-based services.

## Crates

### [`spawn-scope`](./crates/spawn-scope)

`spawn-scope` is a small utility library that provides a flexible mechanism for spawning asynchronous tasks within a `Scope`. It uses `tokio_util::sync::CancellationToken` and `tokio_util::task::TaskTracker` to enable structured concurrency.

Key features:
-   **Scoped Task Spawning:** Spawn tasks that are logically grouped together.
-   **Automatic Cancellation:** Tasks spawned within a `Scope` are automatically cancelled when the `Scope` is dropped or explicitly cancelled.
-   **Cleanup on Cancellation:** Supports `spawn_with_cleanup` to execute a function when a task is cancelled.

This crate forms the foundation for managing task lifecycles, ensuring that background operations do not outlive their intended context.

### [`tower-spawn-scope`](./crates/tower-spawn-scope)

`tower-spawn-scope` builds upon `spawn-scope` by providing a `tower::Layer` (`SpawnScopeLayer`) and an associated `SpawnScopeService` that integrates request-scoped task management into `tower` services.

Key features:
-   **Request-Scoped Tasks:** Automatically associates a `spawn-scope::Scope` with each incoming service request via the `SpawnScopeService`.
-   **Service Integration:** Seamlessly apply structured concurrency to any `tower`-compatible service.
-   **Lifecycle Management:** Tasks spawned within the request's scope are automatically cancelled when the `tower::Service::call` future for that request completes or is dropped (e.g., due to a client timeout or disconnect).

This crate is ideal for web services and other applications built with `tower` (and frameworks like `axum` that use `tower`) where background work needs to be tightly coupled to the lifecycle of an individual request. For example, long-polling tasks, data processing jobs, or resource cleanups can be managed efficiently and safely.
