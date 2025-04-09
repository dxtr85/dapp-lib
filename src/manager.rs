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
    name_to_id: HashMap<SwarmName, (SwarmID, bool)>,
}

impl ApplicationManager {
    pub fn new(gnome_id: GnomeId) -> ApplicationManager {
        let (active_app_data, _recv) = channel::bounded(32);
        ApplicationManager {
            gnome_id,
            app_data_store: HashMap::new(),
            active_app_data: (SwarmID(0), active_app_data),
            name_to_id: HashMap::new(),
        }
    }
    pub fn update_app_data_founder(&mut self, s_id: SwarmID, s_name: SwarmName) {
        self.name_to_id.insert(s_name, (s_id, false));
        let empty = SwarmName {
            founder: GnomeId::any(),
            name: "/".to_string(),
        };
        if let Some((swarm_id, synced)) = self.name_to_id.remove(&empty) {
            if swarm_id != s_id {
                self.name_to_id.insert(empty, (swarm_id, synced));
            } else {
                eprintln!("Removed generic gnome to swarm mapping from AppMgr");
            }
        }
    }
    pub fn number_of_connected_swarms(&self) -> u8 {
        self.name_to_id.len() as u8
    }
    pub fn set_active(&mut self, s_name: &SwarmName) -> Result<SwarmID, ()> {
        eprintln!(
            "Known gnomes: {:?}, searching for: {:?}",
            self.name_to_id.keys(),
            s_name
        );
        // for key in self.gnome_to_swarm.keys() {
        //     if let Some(value) = self.gnome_to_swarm.get(&key) {
        //         eprintln!("K: {:?} - V: {:?}", key, value);
        //     }
        // }
        if let Some((s_id, _synced)) = self.name_to_id.get(s_name) {
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
        for val in self.name_to_id.values_mut() {
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
        self.name_to_id.insert(s_name, (s_id, false));
        self.app_data_store.insert(s_id, sender);
    }
    pub fn get_mapping(&self) -> HashMap<SwarmName, SwarmID> {
        let mut mapping = HashMap::new();
        for (s_name, value) in self.name_to_id.iter() {
            if value.1 {
                mapping.insert(s_name.clone(), value.0);
            }
        }
        mapping
    }
    pub fn get_swarm_founder(&self, swarm_id: &SwarmID) -> Option<GnomeId> {
        for (s_name, (s_id, _synced)) in self.name_to_id.iter() {
            if swarm_id == s_id {
                return Some(s_name.founder);
            }
        }
        None
    }
    pub fn remove_name_mapping(&mut self, s_name: &SwarmName) -> Option<(SwarmID, bool)> {
        self.name_to_id.remove(s_name)
    }
}
