use crate::ToApp;
use crate::ToAppMgr;
use crate::{start_a_timer, TimeoutType};
use async_std::channel;
use async_std::channel::Sender;
use async_std::task::spawn;
use gnome::prelude::ToGnomeManager;
use gnome::prelude::{GnomeId, SwarmID, SwarmName};
use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;
// use std::time::Duration;

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

//TODO: we need to manage various messages coming in
// from GnomeManager and also from Application.
// We should also make sure that our own Swarm has started.
// There should be a list of untouchable SwarmID that can not
// be used as a swap replacement.
// Only if there is explicit SwarmID given from Application
// are we allowed to use it as a swap replacement even if
// it is on an untouchables list.
// There can only be one active process at a time.
// We can not be Joining and Leaving simultaneously.
// But we could be joining multiple swarms at a time.
// Or we could be leaving multiple swarms at a time.
#[derive(Debug)]
pub enum SwapProcess {
    Idle,
    Cooldown,
    WaitingForGnomeMgr,
    Joining(SwarmName),
    Leaving(SwarmID),
}
impl SwapProcess {
    pub fn is_idle(&self) -> bool {
        matches!(self, SwapProcess::Idle)
    }
    pub fn is_waiting(&self) -> bool {
        matches!(self, SwapProcess::WaitingForGnomeMgr)
    }
}

struct SwapState {
    max_swarms: usize,
    running_swarms: HashSet<SwarmID>,
    process: SwapProcess,
    to_join: Vec<SwarmName>,
    //TODO: more attributes
}
impl SwapState {
    pub fn any_swap_slot_available(&self) -> bool {
        // eprintln!(
        //     "is_overloaded running: {} > {} max",
        //     running_swarms, self.max_swarms
        // );
        self.running_swarms.len() < self.max_swarms
    }
    pub fn quit(&mut self) {
        eprintln!("JS: Quit, curr SwapProcess: {:?}", self.process);
        self.process = SwapProcess::Idle;
    }
    fn swarm_to_join(&mut self) -> Option<SwarmName> {
        //TODO: AppMgr should hold a list of names that Application layer wants to get
        // connected
        //
        // We need to make sure our own swarm is running first
        // if !self.name_to_id.contains_key(&self.my_name) {
        //     Some((self.my_name.clone(), None))
        // } else {
        //     None
        // }
        self.to_join.pop()
    }
}

// TODO: we should keep a state describing SwarmSwap operation.
// TODO: we should process messages that can change SwarmSwap state inside AppMgr.
// TODO: we should send out necessary messages from within AppMgr.
pub struct ApplicationManager {
    pub my_name: SwarmName,
    pub app_data_store: HashMap<SwarmID, Sender<ToAppData>>,
    pub active_app_data: (SwarmID, Sender<ToAppData>),
    name_to_id: HashMap<SwarmName, SwarmState>,
    swap_state: SwapState,
    to_gnome_mgr: Sender<ToGnomeManager>,
    to_user: Sender<ToApp>,
    to_app_mgr: Sender<ToAppMgr>,
}

impl ApplicationManager {
    pub fn new(
        gnome_id: GnomeId,
        max_swarms: u8,
        (to_gnome_mgr, to_user, to_app_mgr): (
            Sender<ToGnomeManager>,
            Sender<ToApp>,
            Sender<ToAppMgr>,
        ),
    ) -> ApplicationManager {
        let (active_app_data, _recv) = channel::bounded(32);
        let my_name = SwarmName {
            founder: gnome_id,
            name: "/".to_string(),
        };
        ApplicationManager {
            my_name,
            app_data_store: HashMap::new(),
            active_app_data: (SwarmID(0), active_app_data),
            name_to_id: HashMap::new(),
            swap_state: SwapState {
                max_swarms: max_swarms as usize,
                running_swarms: HashSet::new(),
                process: SwapProcess::Idle,
                to_join: vec![],
            },
            to_gnome_mgr,
            to_user,
            to_app_mgr,
        }
    }
    pub fn update_my_name(&mut self, my_name: SwarmName) {
        self.my_name = my_name.clone();
        self.swap_state.to_join.push(my_name);
    }
    pub fn update_app_data_founder(&mut self, s_id: SwarmID, s_name: SwarmName) {
        let s_state = SwarmState::new(s_id, false, true);
        self.name_to_id.insert(s_name.clone(), s_state);
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
    pub fn swarm_joined(&mut self, s_name: SwarmName) {
        let prev_swap_state = std::mem::replace(&mut self.swap_state.process, SwapProcess::Idle);
        match prev_swap_state {
            SwapProcess::Joining(js_name) => {
                if s_name != js_name {
                    eprintln!("JS: Joined {}, expected {}", s_name, js_name);
                    self.swap_state.process = SwapProcess::Joining(js_name);
                } else {
                    eprintln!("JS: Reseting SwapProcess");
                }
            }
            other => {
                eprintln!("JS: Restoring to {:?}", other);
                self.swap_state.process = other;
            }
        }
    }
    pub fn has_swarm_id(&self, s_id: SwarmID) -> bool {
        for s_state in self.name_to_id.values() {
            if s_state.s_id == s_id {
                return true;
            }
        }
        false
    }
    pub fn get_name(&self, s_id: SwarmID) -> Option<SwarmName> {
        for (s_name, s_state) in self.name_to_id.iter() {
            if s_state.s_id == s_id {
                return Some(s_name.clone());
            }
        }
        None
    }
    pub fn get_swarm_id(&self, s_name: &SwarmName) -> Option<SwarmID> {
        if let Some(s_state) = self.name_to_id.get(s_name) {
            Some(s_state.s_id)
        } else {
            None
        }
    }
    pub fn number_of_connected_swarms(&self) -> u8 {
        self.name_to_id.len() as u8
    }
    pub fn set_active(&mut self, s_name: &SwarmName) -> Result<SwarmID, ()> {
        // eprintln!(
        //     "Known gnomes: {:?}, searching for: {:?}",
        //     self.name_to_id.keys(),
        //     s_name
        // );
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

    pub async fn swarm_busy(&mut self, s_id: SwarmID, is_busy: bool) {
        let can_be_swapped = self.set_busy(s_id, is_busy);
        eprintln!(
            "In ApplicationManager::swarm_busy({}) swappable: {}",
            s_id, can_be_swapped
        );
        // self.update_swap_state(s_id, can_be_swapped).await;
        self.update_swap_state_after_leave(None).await;
    }
    pub async fn swarm_synced(&mut self, s_id: SwarmID) {
        self.swap_state.running_swarms.insert(s_id);
        let can_be_swapped = self.set_synced(s_id);
        eprintln!(
            "In ApplicationManager::swarm_synced({}) swappable: {}",
            s_id, can_be_swapped
        );

        // We activate a swarm if none is active
        if self.active_founder_any() {
            if let Some(s_name) = self.get_name(s_id) {
                eprintln!("His name: {} my_name: {}", s_name, self.my_name);
                if s_name == self.my_name {
                    if let Ok(s_id) = self.set_active(&s_name) {
                        let _ = self.to_user.send(ToApp::ActiveSwarm(s_name, s_id)).await;
                    }
                }
            }
        }
        // self.update_swap_state(s_id, can_be_swapped).await;
        // if self.swap_state.is_overloaded() {
        //     self.swap_state.process = SwapProcess::Cooldown;
        //     spawn(start_a_timer(
        //         self.to_app_mgr.clone(),
        //         TimeoutType::Cooldown,
        //         Duration::from_millis(512),
        //     ));
        // } else {
        self.update_swap_state_after_leave(None).await;
        // }
    }
    pub fn cooldown_over(&mut self) {
        let prev_swap_state = std::mem::replace(&mut self.swap_state.process, SwapProcess::Idle);

        match prev_swap_state {
            SwapProcess::Cooldown => {
                eprintln!("JS: Swap cooldown is over");
            }
            other => {
                eprintln!("JS: Cooldown over when in state: {:?}", other);
                self.swap_state.process = other;
            }
        }
    }
    pub async fn swarm_disconnected(
        &mut self,
        s_id: SwarmID,
        s_name: SwarmName,
        has_neighbors: bool,
    ) {
        // 1 update process value from Leaving to Idle
        self.swap_state.running_swarms.remove(&s_id);
        let prev_swap_state = std::mem::replace(&mut self.swap_state.process, SwapProcess::Idle);
        eprintln!("PState: {:?}    {} {}", prev_swap_state, s_id, s_name);
        match prev_swap_state {
            SwapProcess::Leaving(leave_id) => {
                if leave_id != s_id {
                    eprintln!("JS: Left {}, expected: {}", s_id, leave_id);
                    self.swap_state.process = SwapProcess::Leaving(leave_id);
                } else if has_neighbors {
                    if self.swap_state.any_swap_slot_available() {
                        eprintln!("> We should start a cooldown period before");
                        eprintln!("> joining another swarm in order for");
                        eprintln!("> Networking to settle down");
                        eprintln!("JS: Start Cooldown");
                        self.swap_state.process = SwapProcess::Cooldown;
                        //TODO: gnome manager should take care of this instead,
                        spawn(start_a_timer(
                            self.to_app_mgr.clone(),
                            TimeoutType::Cooldown,
                            Duration::from_millis(1024),
                        ));
                        // and when it triggers reset swap_state.process to Idle.
                    }
                    eprintln!("Reseting SwapProcess after Leaving");
                }
            }
            SwapProcess::Joining(join_name) => {
                if join_name != s_name {
                    eprintln!("JS: Joining {} disconnected: {}", join_name, s_name);
                    self.swap_state.process = SwapProcess::Joining(join_name);
                } else {
                    eprintln!("JS: Reseting SwapProcess after Joining");
                }
            }
            other => {
                eprintln!("JS: Restoring prev state: {:?}", other);
                self.swap_state.process = other;
            }
        }
        eprintln!(
            "PPState: {:?}    {} {}",
            self.swap_state.process, s_id, s_name
        );
        //2 remove name mapping
        self.remove_name_mapping(&s_name);
        // previously it was get not remove
        if let Some(app_data) = self.app_data_store.remove(&s_id) {
            let _ = app_data.send(ToAppData::Terminate).await;
        }
        //3 if swarm is on untouchables list, add it on top of
        // list of swarms to join from app
        if self.is_untouchable(s_id, &s_name) {
            eprintln!("Adding {} to waitlist", s_name);
            spawn(start_a_timer(
                self.to_app_mgr.clone(),
                TimeoutType::AddToWaitList(s_name.clone()),
                Duration::from_millis(1024),
            ));
            // self.swap_state.to_join.push(s_name.clone());
        }

        // 4 notify user if necessary
        // eprintln!("maybe notify user?");
        // 5 update_swap_state
        if has_neighbors {
            eprintln!("mgr has_neighbors");
            self.update_swap_state_after_leave(Some(s_name)).await;
        }
    }

    pub fn add_swarm_to_wait_list(&mut self, s_name: SwarmName) {
        self.swap_state.to_join.push(s_name);
    }
    pub fn reset_swap_state(&mut self) {
        self.swap_state = SwapState {
            max_swarms: self.swap_state.max_swarms,
            running_swarms: HashSet::new(),
            process: SwapProcess::Idle,
            to_join: vec![],
        };
    }
    pub async fn update_swap_state_after_leave(&mut self, left_id: Option<SwarmName>) {
        eprintln!("update_swap_state_after_leave {:?}", left_id);
        // If some other procedure is running we quit
        if !self.swap_state.process.is_idle() {
            eprintln!("Proces not idle: {:?}", self.swap_state.process);
            return;
        }
        // if self.swap_state.is_overloaded(self.name_to_id.len()) {
        if !self.swap_state.any_swap_slot_available() {
            if let Some((_name, leave_id)) = self.ready_to_be_swapped() {
                eprintln!("JS: 3 Set to Leaving({})", leave_id);
                // self.swap_state.running_swarms.remove(&leave_id);
                self.swap_state.process = SwapProcess::Leaving(leave_id);
                let _ = self
                    .to_gnome_mgr
                    .send(ToGnomeManager::LeaveSwarm(leave_id))
                    .await;
            }
        } else {
            // if let Some((swarm_name, neighbor_ids)) = self.swarm_to_join() {
            let mut is_joining = false;
            while let Some(swarm_name) = self.swap_state.swarm_to_join() {
                if self.name_to_id.contains_key(&swarm_name) {
                    continue;
                }
                is_joining = true;
                eprintln!("JS: Swarm to join: {}", swarm_name);
                self.swap_state.process = SwapProcess::Joining(swarm_name.clone());
                let _ = self
                    .to_gnome_mgr
                    .send(ToGnomeManager::JoinSwarm(swarm_name, None))
                    .await;
                break;
            }
            if !is_joining {
                eprintln!("JS: Waiting for GMgr to join a random swarm");
                self.swap_state.process = SwapProcess::WaitingForGnomeMgr;
                let _ = self
                    .to_gnome_mgr
                    .send(ToGnomeManager::JoinRandom(left_id))
                    .await;
            }
        }
    }
    // async fn update_swap_state(&mut self, s_id: SwarmID, can_be_swapped: bool) {
    //     eprintln!(
    //         "update_swap_state {} can be swapped {}",
    //         s_id, can_be_swapped
    //     );
    //     // If some other procedure is running we quit
    //     if !self.swap_state.process.is_idle() {
    //         return;
    //     }

    //     if self.swap_state.is_overloaded() {
    //         // if self.swap_state.is_overloaded(self.name_to_id.len()) {
    //         if can_be_swapped {
    //             eprintln!("1 Set to Leaving({})", s_id);
    //             self.swap_state.process = SwapProcess::Leaving(s_id);
    //             // self.swap_state.running_swarms.remove(&s_id);
    //             let _ = self
    //                 .to_gnome_mgr
    //                 .send(ToGnomeManager::LeaveSwarm(s_id))
    //                 .await;
    //         } else {
    //             if let Some((_name, leave_id)) = self.ready_to_be_swapped() {
    //                 eprintln!("2 Set to Leaving({})", leave_id);
    //                 self.swap_state.process = SwapProcess::Leaving(leave_id);
    //                 // self.swap_state.running_swarms.remove(&leave_id);
    //                 let _ = self
    //                     .to_gnome_mgr
    //                     .send(ToGnomeManager::LeaveSwarm(leave_id))
    //                     .await;
    //             }
    //         }
    //     } else {
    //         if let Some(founder) = self.get_swarm_founder(&s_id) {
    //             // If we don't know what swarm we have just synced,
    //             // we can not join a new swarm
    //             // This is to prevent joining the same swarm multiple times
    //             if founder.is_any() {
    //                 eprintln!("Not starting new swarm - synced founder is any");
    //             } else {
    //                 // if let Some((swarm_name, neighbor_ids)) = self.swarm_to_join() {
    //                 if let Some(swarm_name) = self.swap_state.swarm_to_join() {
    //                     eprintln!("Swarm to join: {}", swarm_name);
    //                     self.swap_state.process = SwapProcess::Joining(swarm_name.clone());
    //                     let _ = self
    //                         .to_gnome_mgr
    //                         .send(ToGnomeManager::JoinSwarm(swarm_name, None))
    //                         .await;
    //                 } else {
    //                     eprintln!("Waiting for GMgr to join a random swarm");
    //                     self.swap_state.process = SwapProcess::WaitingForGnomeMgr;
    //                     let _ = self
    //                         .to_gnome_mgr
    //                         .send(ToGnomeManager::JoinRandom(None))
    //                         .await;
    //                 }
    //             }
    //         }
    //     }
    // }

    pub fn ready_to_be_swapped(&self) -> Option<(SwarmName, SwarmID)> {
        // pub fn ready_to_be_swapped(&self, keep_id: SwarmID) -> Option<(SwarmName, SwarmID)> {
        for (s_name, s_state) in &self.name_to_id {
            if s_state.is_synced
                && !s_state.is_busy
                // && s_state.s_id.0 != keep_id.0
                && self.active_app_data.0 != s_state.s_id
            {
                return Some((s_name.clone(), s_state.s_id));
            }
        }
        None
    }
    pub fn add_app_data(&mut self, s_name: SwarmName, s_id: SwarmID, sender: Sender<ToAppData>) {
        eprintln!("add_app_data {}", s_name);
        if self.app_data_store.is_empty() {
            eprintln!("add_app_data empty");
            self.active_app_data = (s_id, sender.clone());
        }
        self.name_to_id
            .insert(s_name, SwarmState::new(s_id, false, true));
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

    pub fn quit(&mut self) {
        self.swap_state.quit();
    }
    pub fn selected_swarm(&mut self, s_name: Option<SwarmName>) {
        if !self.swap_state.process.is_waiting() {
            eprintln!(
                "JS: Got info from GMgr when in state {:?}",
                self.swap_state.process
            );
            return;
        }
        self.swap_state.process = if let Some(s_name) = s_name {
            eprintln!("JS: Selected, joining {}", s_name);
            SwapProcess::Joining(s_name)
        } else {
            eprintln!("JS: Selected, Idle");
            SwapProcess::Idle
        };
    }
    // fn swarm_to_join(&mut self) -> Option<SwarmName> {
    //     //TODO: AppMgr should hold a list of names that Application layer wants to get
    //     // connected
    //     //
    //     // We need to make sure our own swarm is running first
    //     // if !self.name_to_id.contains_key(&self.my_name) {
    //     //     Some((self.my_name.clone(), None))
    //     // } else {
    //     //     None
    //     // }
    //     self.swap_state.swarm_to_join()
    // }
    fn active_founder_any(&self) -> bool {
        if let Some(name) = self.get_name(self.active_app_data.0) {
            eprintln!("Active name: {}", name);
            name.founder.is_any()
        } else {
            true
        }
    }
    fn is_untouchable(&self, s_id: SwarmID, s_name: &SwarmName) -> bool {
        //TODO: expand this fn
        self.active_app_data.0 == s_id || s_name == &self.my_name
    }
}
