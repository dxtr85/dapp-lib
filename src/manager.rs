use async_std::channel;
use async_std::channel::Sender;
use gnome::prelude::{GnomeId, SwarmID, SwarmName};
use std::collections::HashMap;
// use std::sync::mpsc::{channel, Sender};

use crate::ToAppData;

pub struct ApplicationManager {
    pub gnome_id: GnomeId,
    pub app_data_store: HashMap<SwarmID, Sender<ToAppData>>,
    pub active_app_data: Sender<ToAppData>,
    gnome_to_swarm: HashMap<GnomeId, SwarmID>,
}

impl ApplicationManager {
    pub fn new(gnome_id: GnomeId) -> ApplicationManager {
        let (active_app_data, _recv) = channel::bounded(32);
        ApplicationManager {
            gnome_id,
            app_data_store: HashMap::new(),
            active_app_data,
            gnome_to_swarm: HashMap::new(),
        }
    }
    pub fn update_app_data_founder(&mut self, s_id: SwarmID, f_id: GnomeId) {
        self.gnome_to_swarm.insert(f_id, s_id);
        if let Some(swarm_id) = self.gnome_to_swarm.remove(&GnomeId::any()) {
            if swarm_id != s_id {
                self.gnome_to_swarm.insert(GnomeId::any(), swarm_id);
            } else {
                // eprintln!("Removed generic gnome to swarm mapping from AppMgr");
            }
        }
    }
    pub fn set_active(&mut self, g_id: &GnomeId) -> Result<SwarmID, ()> {
        // eprintln!(
        //     "Known gnomes: {:?}, searching for: {:?}",
        //     self.gnome_to_swarm.keys(),
        //     g_id
        // );
        // for key in self.gnome_to_swarm.keys() {
        //     if let Some(value) = self.gnome_to_swarm.get(&key) {
        //         eprintln!("K: {:?} - V: {:?}", key, value);
        //     }
        // }
        if let Some(s_id) = self.gnome_to_swarm.get(g_id) {
            if let Some(sender) = self.app_data_store.get(s_id) {
                self.active_app_data = sender.clone();
                eprintln!("Swarm {:?} is now active", s_id);
                Ok(*s_id)
            } else {
                Err(())
            }
        } else {
            Err(())
        }
    }
    pub fn add_app_data(&mut self, s_name: SwarmName, s_id: SwarmID, sender: Sender<ToAppData>) {
        if self.app_data_store.is_empty() {
            self.active_app_data = sender.clone();
        }
        self.gnome_to_swarm.insert(s_name.founder, s_id);
        self.app_data_store.insert(s_id, sender);
    }
}
