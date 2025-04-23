use async_std::channel;
use async_std::channel::Sender;
use gnome::prelude::{GnomeId, SwarmID, SwarmName};
use std::collections::HashMap;
// use std::sync::mpsc::{channel, Sender};

use crate::ToAppData;

pub struct SwarmState {
    pub s_id: SwarmID,
    pub is_synced: bool,
    pub is_busy: bool,
}
impl SwarmState {
    pub fn new(s_id: SwarmID, is_synced: bool, is_busy: bool) -> Self {
        SwarmState {
            s_id,
            is_synced,
            is_busy,
        }
    }
}
pub struct ApplicationManager {
    pub gnome_id: GnomeId,
    pub app_data_store: HashMap<SwarmID, Sender<ToAppData>>,
    pub active_app_data: (SwarmID, Sender<ToAppData>),
    name_to_id: HashMap<SwarmName, SwarmState>,
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
        let s_state = SwarmState::new(s_id, false, false);
        self.name_to_id.insert(s_name, s_state);
        let empty = SwarmName {
            founder: GnomeId::any(),
            name: "/".to_string(),
        };
        if let Some(s_state) = self.name_to_id.remove(&empty) {
            if s_state.s_id != s_id {
                self.name_to_id.insert(empty, s_state);
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
        if let Some(s_state) = self.name_to_id.get(s_name) {
            let s_id = s_state.s_id;
            if let Some(sender) = self.app_data_store.get(&s_id) {
                self.active_app_data = (s_id, sender.clone());
                eprintln!("Swarm {} is now active", s_id);
                Ok(s_id)
            } else {
                Err(())
            }
        } else {
            Err(())
        }
    }
    pub fn set_synced(&mut self, s_id: SwarmID) -> bool {
        for s_state in self.name_to_id.values_mut() {
            if s_state.s_id == s_id {
                s_state.is_synced = true;
                return s_state.is_synced && !s_state.is_busy && self.active_app_data.0 != s_id;
            }
        }
        false
    }
    pub fn set_busy(&mut self, s_id: SwarmID, is_busy: bool) -> bool {
        for s_state in self.name_to_id.values_mut() {
            if s_state.s_id == s_id {
                s_state.is_busy = is_busy;
                return s_state.is_synced && !s_state.is_busy && self.active_app_data.0 != s_id;
            }
        }
        false
    }

    pub fn ready_to_be_swapped(&self, keep_id: SwarmID) -> Option<SwarmID> {
        for s_state in self.name_to_id.values() {
            if s_state.is_synced
                && !s_state.is_busy
                && s_state.s_id.0 != keep_id.0
                && self.active_app_data.0 != s_state.s_id
            {
                return Some(s_state.s_id);
            }
        }
        None
    }
    pub fn add_app_data(&mut self, s_name: SwarmName, s_id: SwarmID, sender: Sender<ToAppData>) {
        if self.app_data_store.is_empty() {
            self.active_app_data = (s_id, sender.clone());
        }
        self.name_to_id
            .insert(s_name, SwarmState::new(s_id, false, false));
        self.app_data_store.insert(s_id, sender);
    }
    pub fn get_mapping(&self) -> HashMap<SwarmName, SwarmID> {
        let mut mapping = HashMap::new();
        for (s_name, s_state) in self.name_to_id.iter() {
            if s_state.is_synced {
                mapping.insert(s_name.clone(), s_state.s_id);
            }
        }
        mapping
    }
    pub fn get_swarm_founder(&self, swarm_id: &SwarmID) -> Option<GnomeId> {
        for (s_name, s_state) in self.name_to_id.iter() {
            if swarm_id == &s_state.s_id {
                return Some(s_name.founder);
            }
        }
        None
    }
    pub fn remove_name_mapping(&mut self, s_name: &SwarmName) -> Option<SwarmState> {
        self.name_to_id.remove(s_name)
    }
}
