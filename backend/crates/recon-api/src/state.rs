use recon_store::Store;

// used by routes in a later task
#[allow(dead_code)]
#[derive(Clone)]
pub struct AppState {
    pub store: Store,
}
