use async_std::channel;
use async_std::channel::Sender;
use gnome::prelude::{GnomeId, SwarmID};
use std::collections::HashMap;
// use std::sync::mpsc::{channel, Sender};

use crate::ToAppData;

pub struct ApplicationManager {
    pub gnome_id: GnomeId,
    pub app_data_store: HashMap<SwarmID, Sender<ToAppData>>,
    pub active_app_data: Sender<ToAppData>,
}

impl ApplicationManager {
    pub fn new(gnome_id: GnomeId) -> ApplicationManager {
        let (active_app_data, _recv) = channel::bounded(32);
        ApplicationManager {
            gnome_id,
            app_data_store: HashMap::new(),
            active_app_data,
        }
    }
    pub fn set_active(&mut self, s_id: &SwarmID) -> Result<(), ()> {
        if let Some(sender) = self.app_data_store.get(s_id) {
            self.active_app_data = sender.clone();
            Ok(())
        } else {
            Err(())
        }
    }
    pub fn add_app_data(&mut self, s_id: SwarmID, sender: Sender<ToAppData>) {
        if self.app_data_store.is_empty() {
            self.active_app_data = sender.clone();
        }
        self.app_data_store.insert(s_id, sender);
    }
}
