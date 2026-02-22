use tower::Layer;

use crate::service::SpawnScopeService;

/// Applies Spawn Scope to requests.
#[derive(Clone, Debug, Default)]
pub struct SpawnScopeLayer {}

impl SpawnScopeLayer {
    /// Create a SpawnScopeLayer
    pub fn new() -> Self {
        SpawnScopeLayer {}
    }
}

impl<S> Layer<S> for SpawnScopeLayer {
    type Service = SpawnScopeService<S>;

    fn layer(&self, service: S) -> Self::Service {
        SpawnScopeService::new(service)
    }
}
