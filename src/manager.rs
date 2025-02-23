use async_std::channel;
use async_std::channel::Sender;
use gnome::prelude::{GnomeId, SwarmID, SwarmName};
use std::collections::HashMap;
// use std::sync::mpsc::{channel, Sender};

use crate::ToAppData;

pub struct ApplicationManager {
    pub gnome_id: GnomeId,
    pub app_data_store: HashMap<SwarmID, Sender<ToAppData>>,
    pub active_app_data: (SwarmID, Sender<ToAppData>),
    gnome_to_swarm: HashMap<GnomeId, (SwarmID, bool)>,
}

impl ApplicationManager {
    pub fn new(gnome_id: GnomeId) -> ApplicationManager {
        let (active_app_data, _recv) = channel::bounded(32);
        ApplicationManager {
            gnome_id,
            app_data_store: HashMap::new(),
            active_app_data: (SwarmID(0), active_app_data),
            gnome_to_swarm: HashMap::new(),
        }
    }
    pub fn update_app_data_founder(&mut self, s_id: SwarmID, f_id: GnomeId) {
        self.gnome_to_swarm.insert(f_id, (s_id, false));
        if let Some((swarm_id, synced)) = self.gnome_to_swarm.remove(&GnomeId::any()) {
            if swarm_id != s_id {
                self.gnome_to_swarm
                    .insert(GnomeId::any(), (swarm_id, synced));
            } else {
                // eprintln!("Removed generic gnome to swarm mapping from AppMgr");
            }
        }
    }
    pub fn set_active(&mut self, g_id: &GnomeId) -> Result<SwarmID, ()> {
        eprintln!(
            "Known gnomes: {:?}, searching for: {:?}",
            self.gnome_to_swarm.keys(),
            g_id
        );
        // for key in self.gnome_to_swarm.keys() {
        //     if let Some(value) = self.gnome_to_swarm.get(&key) {
        //         eprintln!("K: {:?} - V: {:?}", key, value);
        //     }
        // }
        if let Some((s_id, _synced)) = self.gnome_to_swarm.get(g_id) {
            if let Some(sender) = self.app_data_store.get(s_id) {
                self.active_app_data = (*s_id, sender.clone());
                eprintln!("Swarm {} is now active", s_id);
                Ok(*s_id)
            } else {
                Err(())
            }
        } else {
            Err(())
        }
    }
    pub fn set_synced(&mut self, s_id: SwarmID) {
        for val in self.gnome_to_swarm.values_mut() {
            if val.0 == s_id {
                val.1 = true;
                break;
            }
        }
    }
    pub fn add_app_data(&mut self, s_name: SwarmName, s_id: SwarmID, sender: Sender<ToAppData>) {
        if self.app_data_store.is_empty() {
            self.active_app_data = (s_id, sender.clone());
        }
        self.gnome_to_swarm.insert(s_name.founder, (s_id, false));
        self.app_data_store.insert(s_id, sender);
    }
    pub fn get_mapping(&self) -> HashMap<GnomeId, SwarmID> {
        let mut mapping = HashMap::new();
        for (g_id, value) in self.gnome_to_swarm.iter() {
            if value.1 {
                mapping.insert(*g_id, value.0);
            }
        }
        mapping
    }
    pub fn remove_gnome_mapping(&mut self, g_id: &GnomeId) -> Option<(SwarmID, bool)> {
        self.gnome_to_swarm.remove(g_id)
    }
}
