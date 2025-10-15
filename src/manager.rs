use crate::content::ContentID;
use crate::content::DataType;
use crate::prelude::AppType;
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
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::time::Duration;
// use std::time::Duration;

use crate::ToAppData;

#[derive(Clone, Copy)]
pub struct SwarmState {
    pub s_id: SwarmID,
    pub app_type: Option<AppType>,
    pub is_synced: bool,
    pub is_busy: bool,
}
impl SwarmState {
    pub fn new(s_id: SwarmID, app_type: Option<AppType>, is_synced: bool, is_busy: bool) -> Self {
        SwarmState {
            s_id,
            app_type,
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
impl Display for SwapProcess {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Cooldown => write!(f, "Cooldown"),
            Self::WaitingForGnomeMgr => write!(f, "WaitingForGnomeMgr"),
            Self::Joining(sn) => write!(f, "Joining({}])", sn),
            Self::Leaving(sn) => write!(f, "Leaving({})", sn),
        }
    }
}

struct ReadState {
    // pending_read: (SwarmID, ContentID, bool),
    // volatile: Option<(SwarmID, ContentID, DataType)>,
    permenent: HashMap<SwarmID, HashSet<(ContentID, DataType)>>,
}
impl ReadState {
    pub fn new() -> Self {
        ReadState {
            // pending_read: (SwarmID(255), u16::MAX, false),
            // volatile: None,
            permenent: HashMap::new(),
        }
    }
    // pub fn add_pending(&mut self, s_id: SwarmID, c_id: ContentID, volatile: bool) {
    //     self.pending_read = (s_id, c_id, volatile);
    // }
    // pub fn update_pending(
    //     &mut self,
    //     s_id: SwarmID,
    //     c_id: ContentID,
    //     d_type: DataType,
    // ) -> Option<(SwarmID, ContentID, DataType)> {
    //     let (ps_id, pc_id, is_volatile) = self.pending_read;
    //     if ps_id == s_id && pc_id == c_id {
    //         self.add(is_volatile, s_id, c_id, d_type)
    //     } else {
    //         eprintln!("TODO: Extend add/update pending reads in dapp-lib/manager");
    //         None
    //     }
    // }
    pub fn add(
        &mut self,
        // is_volatile: bool,
        s_id: SwarmID,
        c_id: ContentID,
        d_type: DataType,
    ) {
        // ) -> Option<(SwarmID, ContentID, DataType)> {
        // if is_volatile {
        //     let to_return = if let Some((es_id, ec_id, ec_dt)) = self.volatile.take() {
        //         Some((es_id, ec_id, ec_dt))
        //     } else {
        //         None
        //     };
        //     self.volatile = Some((s_id, c_id, d_type));
        //     to_return
        // } else {
        if let Some(h_set) = self.permenent.get_mut(&s_id) {
            h_set.insert((c_id, d_type));
        } else {
            let mut new_hset = HashSet::new();
            new_hset.insert((c_id, d_type));
            self.permenent.insert(s_id, new_hset);
        }
        // None
        // }
    }

    pub fn remove(&mut self, s_id: SwarmID, c_id: ContentID, d_type: DataType) {
        // if is_volatile {
        // if let Some((vs_id, vc_id, vc_dt)) = self.volatile.take() {
        //     if vs_id == s_id {
        //         if vc_id == c_id {
        //             // All good
        //         } else {
        //             eprintln!("Volatile ReadRequest remove fail: {}!={}", c_id, vc_id);
        //             self.volatile = Some((vs_id, vc_id, vc_dt));
        //         }
        //     } else {
        //         eprintln!("Volatile ReadRequest remove fail: {}!={}", s_id, vs_id);
        //         self.volatile = Some((vs_id, vc_id, vc_dt));
        //     }
        // }
        // } else {
        if let Some(h_set) = self.permenent.get_mut(&s_id) {
            h_set.remove(&(c_id, d_type));
        }
        // }
    }

    pub fn remove_all(&mut self, s_id: SwarmID) {
        // if let Some((vs_id, vc_id, vc_dt)) = self.volatile.take() {
        //     if vs_id != s_id {
        //         self.volatile = Some((vs_id, vc_id, vc_dt));
        //     }
        // }
        self.permenent.remove(&s_id);
    }
    pub fn read_list(&self) -> Vec<(SwarmID, Vec<(ContentID, DataType)>)> {
        let mut res_list = Vec::with_capacity(self.permenent.len() + 1);
        // if let Some((vs_id, vc_id, vc_dt)) = &self.volatile {
        //     res_list.push((*vs_id, vec![(*vc_id, *vc_dt)]));
        // }
        for (s_id, c_ids) in self.permenent.iter() {
            if !c_ids.is_empty() {
                let mut cids_vec = Vec::with_capacity(c_ids.len());
                for c_id in c_ids.iter() {
                    cids_vec.push(*c_id);
                }
                res_list.push((*s_id, cids_vec));
            }
        }
        res_list
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
    read_state: ReadState,
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
            read_state: ReadState::new(),
            to_gnome_mgr,
            to_user,
            to_app_mgr,
        }
    }

    pub fn get_swarm_state(&self, s_name: &SwarmName) -> Option<SwarmState> {
        self.name_to_id.get(s_name).copied()
    }
    // pub fn add_pending_read(
    //     &mut self,
    //     s_id: SwarmID,
    //     c_id: ContentID,
    //     // d_type: DataType,
    //     is_volatile: bool,
    // ) {
    //     eprintln!(
    //         "{} add_pending {}(is_volatile: {})",
    //         s_id, c_id, is_volatile
    //     );
    //     self.read_state.add_pending(s_id, c_id, is_volatile)
    // }

    // pub fn update_pending_read(
    //     &mut self,
    //     s_id: SwarmID,
    //     c_id: ContentID,
    //     d_type: DataType,
    //     // is_volatile: bool,
    // ) -> Option<(SwarmID, u16, DataType)> {
    //     eprintln!("{} update_pending_read {}", s_id, c_id,);
    //     self.read_state.update_pending(s_id, c_id, d_type)
    // }

    pub fn add_read(
        &mut self,
        s_id: SwarmID,
        c_id: ContentID,
        d_type: DataType,
        // is_volatile: bool,
    ) {
        // ) -> Option<(SwarmID, u16, DataType)> {
        eprintln!("{} update_pending_read {}", s_id, c_id,);
        self.read_state.add(s_id, c_id, d_type)
    }

    pub fn remove_read(&mut self, s_id: SwarmID, c_id: ContentID, d_type: DataType) {
        self.read_state.remove(s_id, c_id, d_type);
    }
    pub fn read_list(&self) -> Vec<(SwarmID, Vec<(ContentID, DataType)>)> {
        self.read_state.read_list()
    }

    pub fn update_my_name(&mut self, my_name: SwarmName) {
        self.my_name = my_name.clone();
        self.swap_state.to_join.push(my_name);
    }
    pub async fn update_app_data_founder(
        &mut self,
        s_id: SwarmID,
        app_type: Option<AppType>,
        s_name: SwarmName,
    ) {
        eprintln!("update_app_data_founder: {} ({:?})", s_name, app_type);
        if s_name.founder.is_any() && app_type.is_some() {
            let new_name = self.get_name(s_id).unwrap();
            if let Some(state) = self.name_to_id.get_mut(&new_name) {
                eprintln!("{} new AppType: {:?}", s_id, app_type);
                state.app_type = app_type;
            }
            return;
        }
        let s_state = if let Some(mut e_state) = self.name_to_id.remove(&s_name) {
            e_state.s_id = s_id;
            e_state.app_type = app_type;
            e_state
        } else {
            SwarmState::new(s_id, app_type, false, true)
        };
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
                let sender = self.app_data_store.get(&s_id).unwrap();
                let _ = sender.send(ToAppData::MyName(s_name)).await;
                eprintln!("known names: {:?}", self.name_to_id.keys());
            }
        }
    }
    pub fn swarm_joined(&mut self, s_id: SwarmID, s_name: SwarmName) {
        eprintln!("{s_id} SwarmJoined: {}", s_name);
        let prev_swap_state = std::mem::replace(&mut self.swap_state.process, SwapProcess::Idle);
        // if !s_name.founder.is_any() {
        //     self.update_app_data_founder(s_id, s_name.clone());
        // }
        match prev_swap_state {
            SwapProcess::Joining(js_name) => {
                if s_name != js_name && self.name_to_id.len() != 1 {
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
        // for name in self.name_to_id.keys() {
        //     eprintln!("I know: {:?}", name);
        // }
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
                eprintln!("{} is now synced", s_id);
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
        self.update_swap_state_after_leave(None, false).await;
    }
    pub async fn swarm_synced(&mut self, s_id: SwarmID) -> bool {
        let mut own_swarm_started_and_activated = false;
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
                if let Some(s_state) = self.get_swarm_state(&s_name) {
                    if s_state.app_type.is_none() {
                        if let Some(sender) = self.app_data_store.get(&s_id) {
                            let _ = sender
                                .send(ToAppData::ReadPagesRange(crate::Requestor::App, 0, 0, 0))
                                .await;
                        }
                    }
                }
                if s_name == self.my_name {
                    if let Ok(s_id) = self.set_active(&s_name) {
                        own_swarm_started_and_activated = true;
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
        self.update_swap_state_after_leave(None, false).await;
        // }
        own_swarm_started_and_activated
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
        self.read_state.remove_all(s_id);
        let prev_swap_state = std::mem::replace(&mut self.swap_state.process, SwapProcess::Idle);
        eprintln!("PState: {}    {} {}", prev_swap_state, s_id, s_name);
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
                eprintln!("JS: Restoring prev state: {}", other);
                self.swap_state.process = other;
            }
        }
        eprintln!(
            "PPState: {}    {} {}",
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
            self.update_swap_state_after_leave(Some(s_name), false)
                .await;
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
    pub async fn update_swap_state_after_leave(&mut self, left_id: Option<SwarmName>, reset: bool) {
        eprintln!(
            "update_swap_state_after_leave {:?}, {}",
            left_id, self.swap_state.process
        );
        // If some other procedure is running we quit
        if !self.swap_state.process.is_idle() {
            if reset {
                eprintln!("Proces reset from: {}", self.swap_state.process);
                self.swap_state.process = SwapProcess::Idle;
            } else {
                eprintln!("Proces not idle: {}", self.swap_state.process);
                return;
            }
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
    pub fn add_app_data(
        &mut self,
        s_name: SwarmName,
        s_id: SwarmID,
        app_type: Option<AppType>,
        sender: Sender<ToAppData>,
    ) {
        eprintln!("add_app_data {}, {:?}", s_name, app_type);
        if self.app_data_store.is_empty() {
            eprintln!("add_app_data empty");
            self.active_app_data = (s_id, sender.clone());
        }
        // let app_type = if let Some(a_type) = app_type {
        //     a_type
        // } else {
        //     AppType::Other(0)
        // };
        self.name_to_id
            .insert(s_name, SwarmState::new(s_id, app_type, false, true));
        for val in self.name_to_id.values() {
            eprintln!("n2id {} {:?}", val.s_id, val.app_type);
        }
        self.app_data_store.insert(s_id, sender);
    }
    pub fn get_mapping(&self) -> HashMap<SwarmName, (SwarmID, Option<AppType>)> {
        eprintln!("In get_mapping");
        let mut mapping = HashMap::new();
        for (s_name, s_state) in self.name_to_id.iter() {
            if s_state.is_synced {
                eprintln!("get_mapping incl: {} {:?}", s_name, s_state.app_type);
                mapping.insert(s_name.clone(), (s_state.s_id, s_state.app_type));
            } else {
                eprintln!("get_mapping excl: {} {:?}", s_name, s_state.app_type);
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
        if let Some(s) = &s_name {
            eprintln!("Selected: {}", s);
        }
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
