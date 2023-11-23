use std::sync::{Arc, RwLock};

use crate::util::jwt::Auth;

#[derive(Debug, Clone, Default)]
pub struct AppState {
    pub auth: Auth,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            auth: Default::default(),
        }
    }
    pub fn change(&mut self, new: &str) {
        // self.version = new.to_string();
    }
}

pub type SharedState = Arc<RwLock<AppState>>;
