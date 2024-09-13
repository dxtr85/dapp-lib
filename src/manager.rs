use async_std::channel;
use async_std::channel::Sender;
use gnome::prelude::SwarmID;
use std::collections::HashMap;
// use std::sync::mpsc::{channel, Sender};

use crate::ToAppData;

pub struct ApplicationManager {
    pub app_data_store: HashMap<SwarmID, Sender<ToAppData>>,
    pub active_app_data: Sender<ToAppData>,
}

impl ApplicationManager {
    pub fn new() -> ApplicationManager {
        let (active_app_data, _recv) = channel::bounded(32);
        ApplicationManager {
            app_data_store: HashMap::new(),
            active_app_data,
        }
    }
    pub fn add_app_data(&mut self, s_id: SwarmID, sender: Sender<ToAppData>) {
        if self.app_data_store.is_empty() {
            self.active_app_data = sender.clone();
        }
        self.app_data_store.insert(s_id, sender);
    }
}
