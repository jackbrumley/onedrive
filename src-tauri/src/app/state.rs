use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct DeviceAuthPending {
    pub profile_id: String,
    pub device_code: String,
    pub interval_secs: u64,
}

pub struct AppState {
    pub profiles_lock: Arc<Mutex<()>>,
    pub auth_pending: Arc<Mutex<HashMap<String, DeviceAuthPending>>>,
    pub sync_worker_stops: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>>,
}

pub fn create_app_state() -> AppState {
    AppState {
        profiles_lock: Arc::new(Mutex::new(())),
        auth_pending: Arc::new(Mutex::new(HashMap::new())),
        sync_worker_stops: Arc::new(Mutex::new(HashMap::new())),
    }
}
