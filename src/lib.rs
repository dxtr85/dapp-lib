use crate::content::data_to_link;
use crate::prelude::SyncRequirements;
use crate::prelude::TransformInfo;
use crate::sync_message::serialize_requests;
use std::net::IpAddr;
// use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
mod app_type;
mod config;
mod content;
mod data;
mod datastore;
mod error;
mod manager;
mod message;
mod registry;
mod storage;
mod sync_message;
use app_type::AppType;
use async_std::fs::create_dir_all;
// use async_std::net::ToSocketAddrs;
use std::collections::HashMap;
use std::collections::HashSet;
use storage::load_content_from_disk;
use storage::store_data_on_disk;
// use storage::write_datastore_to_disk;
use sync_message::deserialize_requests;
use sync_message::SyncRequest;
use sync_message::SyncResponse;

use crate::content::double_hash;
use async_std::task::sleep;
use async_std::task::spawn;
pub use config::Configuration;
use content::ContentTree;
use content::DataType;
use content::{Content, ContentID};
pub use data::Data;
use datastore::Datastore;
use error::AppError;
use gnome::prelude::*;
pub use manager::ApplicationManager;
use message::{SyncMessage, SyncMessageType};
use registry::ChangeRegistry;
use storage::read_datastore_from_disk;
// TODO: probably better to use async channels for this lib where possible
use std::sync::mpsc::channel;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;

use async_std::channel as achannel;
use async_std::channel::Receiver as AReceiver;
use async_std::channel::Sender as ASender;

pub mod prelude {
    pub use crate::app_type::AppType;
    pub use crate::content::{
        double_hash, Content, ContentID, ContentTree, DataType, TransformInfo,
    };
    pub use crate::data::Data;
    pub use crate::error::AppError;
    pub use crate::initialize;
    pub use crate::message::SyncMessage;
    pub use crate::message::SyncMessageType;
    pub use crate::message::SyncRequirements;
    pub use crate::storage::load_content_from_disk;
    pub use crate::storage::read_datastore_from_disk;
    pub use crate::ApplicationData;
    pub use crate::ApplicationManager;
    pub use crate::Configuration;
    pub use crate::ToApp;
    pub use gnome::prelude::CastData;
    pub use gnome::prelude::GnomeId;
    pub use gnome::prelude::Nat;
    pub use gnome::prelude::NetworkSettings;
    pub use gnome::prelude::PortAllocationRule;
    pub use gnome::prelude::SwarmID;
    pub use gnome::prelude::SyncData;
    pub use gnome::prelude::ToGnome;
}

pub enum ToApp {
    ActiveSwarm(GnomeId, SwarmID), //TODO: also send Vec<Data> from CID=0(Manifest)
    Neighbors(SwarmID, Vec<GnomeId>),
    NewContent(SwarmID, ContentID, DataType, Data),
    ContentChanged(SwarmID, ContentID, DataType, Option<Data>),
    ReadSuccess(SwarmID, ContentID, DataType, Vec<Data>),
    ReadError(SwarmID, ContentID, AppError),
    GetCIDsForTags(SwarmID, GnomeId, Vec<u8>, Vec<(ContentID, Data)>),
    MyPublicIPs(Vec<(IpAddr, u16, Nat, (PortAllocationRule, i8))>),
    GnomeToSwarmMapping(HashMap<GnomeId, SwarmID>),
    Disconnected,
}

pub enum ToAppMgr {
    GetCIDsForTags(SwarmID, GnomeId, Vec<u8>, Vec<(ContentID, Data)>),
    CIDsForTag(SwarmID, GnomeId, u8, ContentID, Data),
    ContentLoadedFromDisk(SwarmID, ContentID, DataType, Data),
    ContentRequestedFromNeighbor(SwarmID, ContentID, DataType),
    ReadData(SwarmID, ContentID),
    ReadSuccess(SwarmID, ContentID, DataType, Vec<Data>),
    ReadError(SwarmID, ContentID, AppError),
    UploadData,
    SetActiveApp(GnomeId),
    StartUnicast,
    StartBroadcast,
    EndBroadcast,
    UnsubscribeBroadcast,
    ListNeighbors,
    NeighborsListing(SwarmID, Vec<GnomeId>),
    ChangeContent(SwarmID, ContentID, DataType, Vec<Data>),
    AppendContent(SwarmID, DataType, Data),
    AppendData(SwarmID, ContentID, Data),
    RemoveData(SwarmID, ContentID, u16),
    UpdateData(SwarmID, ContentID, u16, Data),
    ContentAdded(SwarmID, ContentID, DataType, Data),
    ContentChanged(SwarmID, ContentID, DataType, Option<Data>),
    TransformLinkRequest(Box<SyncData>),
    ProvideGnomeToSwarmMapping,
    Quit,
}

#[derive(Debug)]
pub enum ToAppData {
    Response(GnomeToApp),
    ReadData(ContentID),
    SendFirstPage(GnomeId, ContentID, Data),
    UploadData,
    StartUnicast,
    StartBroadcast,
    EndBroadcast,
    UnsubscribeBroadcast,
    ListNeighbors,
    SwarmReady(SwarmName),
    BCastData(CastID, CastData),
    BCastOrigin(CastID, Sender<CastData>),
    ChangeContent(ContentID, DataType, Vec<Data>),
    UpdateData(ContentID, u16, Data),
    AppendContent(DataType, Data),
    AppendData(ContentID, Data),
    RemoveData(ContentID, u16),
    AppendShelledDatas(ContentID, Data, Vec<Data>),
    CustomRequest(u8, GnomeId, CastData),
    CustomResponse(u8, GnomeId, CastData),
    TransformLinkRequest(SyncData),
    TransformLink(SyncData),
    Terminate,
}
// TODO: We need to define a way where we can update multiple Data of a given ContentID at once
// I.e. in a Catalog swarm we have defined a Tag, and now Manifest that was 1 Data with single
// byte in it becomes 4 Datas filled with bytes
// Procedure to write will be:
// 1. Update first Data
// 2. Extend given ContentID with Data shells containing only root hash of Data that is yet to
//    come. In case some of those Data is filled with all zeros, then we can send 0 as a hash
//    for that Data, in order not to send all zeros across the net.
// 3. For any Data initialized in previous point that is not all zeros send it in
//    a ChangeContent message (or it could be sent via broadcast since Datastore hash
//    will stay the samy. It was changed after point 2 was evaluated.)
// For some cases where all extended Data have their hashes equal to zero,
// point 3 can be skipped.

struct BigChunk(u8, u16);
impl BigChunk {
    fn next(&mut self) -> Option<Data> {
        if self.1 == 0 {
            return None;
        }
        let value = ((self.0 as u32 * self.1 as u32) % 255) as u8;
        self.1 -= 1;
        let data = Data::new(vec![value; 1024]).unwrap();
        Some(data)
    }
}

// fn parse_sync(sync_data: SyncData) -> AppMessage {}

pub fn initialize(
    to_user_send: Sender<ToApp>,
    to_app_mgr_send: Sender<ToAppMgr>,
    to_app_mgr_recv: Receiver<ToAppMgr>,
    config_dir: PathBuf,
    mut neighbors: Vec<(GnomeId, NetworkSettings)>,
) -> GnomeId {
    // TODO: neighbors contain also our own public IP, get rid of it!
    eprintln!("Storage neighbors: {:?}", neighbors);
    let config = Configuration::new(config_dir.clone());
    if let Some(ns) = config.neighbors {
        for n in ns {
            // if !neighbors.contains(&n) {
            eprintln!("Pushing config.neighbor");
            neighbors.push((GnomeId::any(), n));
            // }
        }
    }
    eprintln!("Storage root: {:?}", config.storage);
    // TODO: here we need to load neighbors from storage and provide it to
    // init function
    // let neighbors =
    // let (gmgr_send, gmgr_recv, my_id) = init(config_dir, config.neighbors);
    let (gmgr_send, gmgr_recv, my_id) = init(config_dir, Some(neighbors));
    spawn(serve_gnome_manager(
        config.storage.clone(),
        gmgr_send,
        gmgr_recv,
        to_user_send,
        to_app_mgr_send,
        to_app_mgr_recv,
    ));
    my_id
}

async fn serve_gnome_manager(
    storage: PathBuf,
    to_gnome_mgr: Sender<ToGnomeManager>,
    from_gnome_mgr: Receiver<FromGnomeManager>,
    to_user: Sender<ToApp>,
    to_app_mgr: Sender<ToAppMgr>,
    to_app_mgr_recv: Receiver<ToAppMgr>,
) {
    // TODO: AppMgr should hold state
    eprintln!("Storage root, gmgr: {:?}", storage);
    let message = from_gnome_mgr
        .recv()
        .expect("First message sent from gnome mgr has to be MyID");
    let mut my_id = if let FromGnomeManager::MyID(gnome_id) = message {
        gnome_id
    } else {
        GnomeId(u64::MAX)
    };
    // eprintln!("My-ID: {}", my_id);
    let mut app_mgr = ApplicationManager::new(my_id);

    let sleep_time = Duration::from_millis(128);
    let mut own_swarm_started = false;
    'outer: loop {
        sleep(sleep_time).await;
        while let Ok(message) = from_gnome_mgr.try_recv() {
            match message {
                FromGnomeManager::MyID(m_id) => my_id = m_id,
                FromGnomeManager::SwarmFounderDetermined(swarm_id, f_id) => {
                    // eprintln!("SwarmFounderDetermined (is it me?: {})", f_id == my_id);
                    app_mgr.update_app_data_founder(swarm_id, f_id);
                    //TODO: distinguish between founder and my_id, if not equal
                    // request gnome manager to join another swarm where f_id == my_id
                    if f_id != my_id {
                        if !own_swarm_started {
                            own_swarm_started = true;
                            eprintln!("Starting a new swarm, where I am Founder…");
                            let _ = to_gnome_mgr.send(ToGnomeManager::JoinSwarm(
                                SwarmName::new(my_id, "/".to_string()).unwrap(),
                            ));
                        } else {
                            // eprintln!("TODO: Need to notify neighbor about my swarm");
                            app_mgr.set_active(&my_id);
                        }
                    } else {
                        own_swarm_started = true;
                    }
                }
                FromGnomeManager::MyPublicIPs(ip_list) => {
                    // eprintln!("dapp_lib got list of my public IP");
                    // for (ip, port, nat, (rule, val)) in &ip_list {
                    //     eprintln!(
                    //         "dapp_lib Pub IP: {}:{} ({:?}, {:?}:{})",
                    //         ip, port, nat, rule, val
                    //     );
                    // }
                    let _ = to_user.send(ToApp::MyPublicIPs(ip_list));
                }
                FromGnomeManager::NewSwarmAvailable(swarm_name) => {
                    eprintln!("NewSwarm available, joining: {}", swarm_name);
                    if !own_swarm_started && swarm_name.founder == my_id {
                        eprintln!("Oh, seems like my swarm is already there, I'll just join it");
                        own_swarm_started = true;
                    }
                    let _res = to_gnome_mgr.send(ToGnomeManager::JoinSwarm(swarm_name));
                    // eprintln!("Join sent: {:?}", _res);
                }
                FromGnomeManager::SwarmJoined(s_id, s_name, to_swarm, from_swarm) => {
                    eprintln!("{:?} joined {}", s_id, s_name);
                    // TODO: we need to identify to which application should we assign
                    //       given swarm
                    // TODO: we need a bi-directional communication between
                    // AppMgr and each AppData
                    // TODO: we need a bi-directional comm between AppMgr and user App
                    let (to_app_data_send, to_app_data_recv) = achannel::bounded(32);
                    app_mgr.add_app_data(s_name, s_id, to_app_data_send.clone());
                    // TODO: build an algorithm that decides how many pages should be provisioned
                    let max_pages_in_memory = 32768;
                    // eprintln!("spawning new serve_app_data");
                    let app_data = ApplicationData::new(AppType::Catalog);
                    spawn(serve_app_data(
                        storage.clone(),
                        max_pages_in_memory,
                        s_id,
                        app_data,
                        to_app_data_send.clone(),
                        to_app_data_recv,
                        to_swarm,
                        to_app_mgr.clone(),
                    ));
                    spawn(serve_swarm(
                        Duration::from_millis(64),
                        from_swarm,
                        to_app_data_send.clone(),
                    ));
                }
                FromGnomeManager::Disconnected => {
                    // eprintln!("AppMgr received Disconnected");
                    let _ = to_user.send(ToApp::Disconnected);
                    break 'outer;
                }
            }
        }
        while let Ok(message) = to_app_mgr_recv.try_recv() {
            match message {
                ToAppMgr::UploadData => {
                    let _ = app_mgr.active_app_data.send(ToAppData::UploadData).await;
                }
                ToAppMgr::ReadData(s_id, c_id) => {
                    if let Some(to_app_data) = app_mgr.app_data_store.get(&s_id) {
                        let _ = to_app_data.send(ToAppData::ReadData(c_id)).await;
                    }
                }
                ToAppMgr::GetCIDsForTags(swarm_id, n_id, tags, all_first_pages) => {
                    eprintln!("We are requesting tags {:?} for {:?} swarm", tags, swarm_id);
                    let _ =
                        to_user.send(ToApp::GetCIDsForTags(swarm_id, n_id, tags, all_first_pages));
                }
                ToAppMgr::CIDsForTag(s_id, n_id, _tag, cid, data) => {
                    // eprintln!("We are supposed to send those back to select swarm and neighbor");

                    if let Some(to_app_data) = app_mgr.app_data_store.get(&s_id) {
                        let _ = to_app_data
                            .send(ToAppData::SendFirstPage(n_id, cid, data))
                            .await;
                    }
                }
                ToAppMgr::ReadSuccess(s_id, c_id, dtype, data_vec) => {
                    let _ = to_user.send(ToApp::ReadSuccess(s_id, c_id, dtype, data_vec));
                }
                ToAppMgr::ReadError(s_id, c_id, error) => {
                    let _ = to_user.send(ToApp::ReadError(s_id, c_id, error));
                }
                ToAppMgr::TransformLinkRequest(boxed_s_data) => {
                    let _ = app_mgr
                        .active_app_data
                        .send(ToAppData::TransformLinkRequest(*boxed_s_data))
                        .await;
                }
                ToAppMgr::SetActiveApp(gnome_id) => {
                    if let Ok(s_id) = app_mgr.set_active(&gnome_id) {
                        let _ = to_user.send(ToApp::ActiveSwarm(gnome_id, s_id));
                    } else {
                        eprintln!("Unable to find swarm for {}…", gnome_id);
                    }
                }
                ToAppMgr::StartUnicast => {
                    let _ = app_mgr.active_app_data.send(ToAppData::StartUnicast).await;
                }
                ToAppMgr::StartBroadcast => {
                    let _ = app_mgr
                        .active_app_data
                        .send(ToAppData::StartBroadcast)
                        .await;
                }
                ToAppMgr::EndBroadcast => {
                    eprintln!("ToAppMgr::EndBroadcast");
                    let _ = app_mgr.active_app_data.send(ToAppData::EndBroadcast).await;
                }
                ToAppMgr::UnsubscribeBroadcast => {
                    eprintln!("ToAppMgr::UnsubscribeBroadcast");
                    let _ = app_mgr
                        .active_app_data
                        .send(ToAppData::UnsubscribeBroadcast)
                        .await;
                }
                ToAppMgr::ListNeighbors => {
                    let _ = app_mgr.active_app_data.send(ToAppData::ListNeighbors).await;
                }
                ToAppMgr::NeighborsListing(s_id, neighbors) => {
                    let _ = to_user.send(ToApp::Neighbors(s_id, neighbors));
                }
                ToAppMgr::ChangeContent(s_id, c_id, d_type, data_vec) => {
                    // TODO: make sure we are sending this to correct swarm!
                    // eprintln!("app mgr received CC request, sending to app data");
                    // for data in &data_vec {
                    //     eprintln!("h:{}, d: {:?}", data.get_hash(), data);
                    // }
                    let _ = app_mgr
                        .active_app_data
                        .send(ToAppData::ChangeContent(c_id, d_type, data_vec))
                        .await;
                }
                ToAppMgr::AppendContent(_s_id, d_type, data) => {
                    let _ = app_mgr
                        .active_app_data
                        .send(ToAppData::AppendContent(d_type, data))
                        .await;
                }
                ToAppMgr::AppendData(s_id, c_id, data) => {
                    if let Some(sender) = app_mgr.app_data_store.get(&s_id) {
                        let _ = sender.send(ToAppData::AppendData(c_id, data)).await;
                    }
                }
                ToAppMgr::RemoveData(s_id, c_id, data_id) => {
                    if let Some(sender) = app_mgr.app_data_store.get(&s_id) {
                        let _ = sender.send(ToAppData::RemoveData(c_id, data_id)).await;
                    }
                }
                ToAppMgr::UpdateData(s_id, c_id, d_id, data) => {
                    if let Some(sender) = app_mgr.app_data_store.get(&s_id) {
                        let _ = sender.send(ToAppData::UpdateData(c_id, d_id, data)).await;
                    }
                }
                // ToAppMgr::AppendShelledDatas(_s_id, c_id, data) => {
                //     //TODO
                //     // let mut data_hashes = vec![];
                //     // let bytes = data.bytes();
                //     // for chunk in bytes.chunks_exact(8) {
                //     //     data_hashes.push(u64::from_be_bytes(chunk.try_into().unwrap()))
                //     // }
                //     let _ = app_mgr
                //         .active_app_data
                //         .send(ToAppData::AppendShelledDatas(c_id, hash_data, data))
                //         .await;
                // }
                ToAppMgr::ContentAdded(s_id, c_id, d_type, main_page) => {
                    // eprintln!("GENERATE ToApp::NewContent({:?},{:?})", c_id, d_type);
                    let _ = to_user.send(ToApp::NewContent(s_id, c_id, d_type, main_page));
                }
                ToAppMgr::ContentLoadedFromDisk(s_id, c_id, d_type, main_page) => {
                    // eprintln!("GENERATE ToApp::NewContent({:?},{:?})", c_id, d_type);
                    if c_id > 0 {
                        let _ = to_user.send(ToApp::NewContent(s_id, c_id, d_type, main_page));
                    } else {
                        // eprintln!("Not informing user about Manifest in {:?}", s_id);
                        if let Some(to_app_data) = app_mgr.app_data_store.get(&s_id) {
                            let _ = to_app_data.send(ToAppData::ReadData(c_id)).await;
                        }
                    }
                }
                ToAppMgr::ContentRequestedFromNeighbor(s_id, c_id, d_type) => {
                    if c_id > 0 {
                        let _ = to_user.send(ToApp::NewContent(s_id, c_id, d_type, Data::empty(0)));
                    } else {
                        // eprintln!("Not informing user about Manifest in {:?}", s_id);
                        // if let Some(to_app_data) = app_mgr.app_data_store.get(&s_id) {
                        //     let _ = to_app_data.send(ToAppData::ReadData(c_id)).await;
                        // }
                    }
                }
                ToAppMgr::ContentChanged(s_id, c_id, d_type, mpo) => {
                    eprintln!("ToApp::ContentChanged({:?})", c_id,);
                    let _ = to_user.send(ToApp::ContentChanged(s_id, c_id, d_type, mpo));
                }
                ToAppMgr::ProvideGnomeToSwarmMapping => {
                    let _ = to_user.send(ToApp::GnomeToSwarmMapping(app_mgr.get_mapping()));
                }
                ToAppMgr::Quit => {
                    // eprintln!("AppMgr received Quit");
                    let _ = to_gnome_mgr.send(ToGnomeManager::Disconnect);
                    for app_data in app_mgr.app_data_store.values() {
                        // let _ = app_mgr.active_app_data.send(ToAppData::Terminate).await;
                        let _ = app_data.send(ToAppData::Terminate).await;
                    }
                    break;
                }
            }
        }
    }
    eprintln!("Done serving AppMgr");
}

async fn serve_app_data(
    storage: PathBuf,
    max_pages_in_memory: usize,
    swarm_id: SwarmID,
    mut app_data: ApplicationData,
    app_data_send: ASender<ToAppData>,
    app_data_recv: AReceiver<ToAppData>,
    to_gnome_sender: Sender<ToGnome>,
    to_app_mgr_send: Sender<ToAppMgr>,
) {
    let mut s_storage = PathBuf::new();
    // TODO: use this variable to track how many pages are being stored in memory
    let mut used_memory_pages = 0;
    // TODO: build logic to decide whether or not we should store this Swarm's data on disk
    //       and if so, which parts of it (maybe all?)
    //       This should be merged with application logic
    let mut store_on_disk = false;
    eprintln!("Storage root app data: {:?}", storage);
    eprintln!(
        "{:?} Memory: {}/{} Pages (not implemented)",
        swarm_id, used_memory_pages, max_pages_in_memory
    );
    let mut b_cast_origin: Option<(CastID, Sender<CastData>)> = None;
    let mut link_with_transform_info: Option<ContentID> = None;
    let mut b_req_sent = false;
    // let mut next_val = 0;
    let mut datastore_sync: Option<(u16, HashMap<u16, Vec<(DataType, u64)>>)> =
        Some((0, HashMap::new()));
    let sleep_time = Duration::from_millis(32);
    while let Ok(resp) = app_data_recv.recv().await {
        match resp {
            // TODO: We should always assume to be out of sync
            ToAppData::SwarmReady(s_name) => {
                s_storage = storage.join(s_name.to_path());
                let dsync_store = s_storage.join("datastore.sync");
                if dsync_store.exists() {
                    app_data = read_datastore_from_disk(
                        dsync_store.clone(),
                        // app_data_send.clone(),
                    )
                    .await
                } else {
                    eprintln!("{:?} does not exist", dsync_store);
                    //     ApplicationData::new(AppType::Catalog)
                };
                // TODO: spawn a task to load datastore from disk into memory for
                //       comparison with Swarm's datastore
                if s_storage.exists() {
                    store_on_disk = true;
                    eprintln!("Swarm storage at: {:?}", s_storage);
                    // TODO: now we need to compare what we have on disk with Swarm
                } else {
                    store_on_disk = true;
                    // TODO: decide if we should create a dir (possibly after syncing)
                    eprintln!("Creating Swarm storage at: {:?}", s_storage);
                    let _ = create_dir_all(s_storage.clone()).await;
                }
                //TODO: implement logic for disk storage verification against swarm
                //
                // If we have some data on disk we send only Datastore request
                // once we receive it, we compare received value against what we store
                // on disk.
                // if they match, we read data from disk,
                // if they differ, we find out what is the difference and send
                // requests to neighbors for missing data,
                // all other data that matches is being read from disk
                //
                // And if we do not have data on disk we follow the same route as if
                // we had some data on disk, but it was all invalid, so maximum difference.
                let sync_requests: Vec<SyncRequest> = vec![
                    SyncRequest::Datastore,
                    // SyncRequest::AllFirstPages(Some(vec![0])),
                    // SyncRequest::Hashes(0, vec![]),
                    // SyncRequest::Hashes(1, vec![]),
                    // SyncRequest::Hashes(2, vec![]),
                    // SyncRequest::Hashes(3, vec![]),
                    // SyncRequest::AllPages(vec![0, 1, 2, 3]),
                ];
                // let sync_requests: Vec<u8> = vec![
                //     0, // We want all root hashes from Datastore
                //     1, // We want all first pages of every Content in Datastore
                //     2, 0, 0, // We want all pages of specified ContentID (here CID(0))
                // ];
                let _ = to_gnome_sender.send(ToGnome::AskData(
                    GnomeId::any(),
                    NeighborRequest::Custom(
                        0,
                        CastData::new(serialize_requests(sync_requests)).unwrap(),
                    ),
                ));
                // } else {
                //     eprintln!("App synced");
                //     // eprintln!("Set DStore to None");
                //     datastore_sync = None;
                // }
            }
            ToAppData::StartUnicast => {
                let res =
                    to_gnome_sender.send(ToGnome::StartUnicast(GnomeId(15561580566906229863)));
                eprintln!("UnicastReq: {:?}", res);
            }
            ToAppData::StartBroadcast => {
                let res = to_gnome_sender.send(ToGnome::StartBroadcast);
                b_req_sent = res.is_ok();
            }
            ToAppData::UnsubscribeBroadcast => {
                // TODO: get this from app's logic
                let c_id = CastID(0);
                let res = to_gnome_sender.send(ToGnome::UnsubscribeBroadcast(c_id));
                b_req_sent = res.is_ok();
            }
            ToAppData::EndBroadcast => {
                eprintln!("ToAppData::EndBroadcast");
                if let Some((c_id, _sender)) = b_cast_origin {
                    // eprintln!("Some");
                    let res = to_gnome_sender.send(ToGnome::EndBroadcast(c_id));
                    b_req_sent = res.is_ok();
                    b_cast_origin = None;
                } else {
                    // eprintln!("None");
                }
            }
            ToAppData::AppendShelledDatas(c_id, hash_data, data_vec) => {
                // eprintln!("evaluating AppendShelledDatas");
                if let Ok((_d_type, pre_hash)) = app_data.content_root_hash(c_id) {
                    //TODO: here we need to calculate root hash
                    let mut c = app_data.shell(c_id).unwrap();
                    let mut next_data_idx = c.len();
                    // for h in c.data_hashes() {
                    //     eprintln!("Original hashes: {:?},len: {}", h.to_be_bytes(), c.len());
                    // }
                    let bytes = hash_data.clone().bytes();
                    for hash_bytes in bytes.chunks_exact(8) {
                        // eprintln!("Pushing hash: {:?}", hash_bytes);
                        let _ = c.push_data(Data::empty(u64::from_be_bytes(
                            hash_bytes.try_into().unwrap(),
                        )));
                        for h in c.data_hashes() {
                            // eprintln!("New hashes: {:?}", h.to_be_bytes());
                        }
                        // eprintln!("Len: {}", c.len());
                    }
                    let post_hash = c.hash();
                    // eprintln!("Post hash calculated: {:?}", post_hash.to_be_bytes());
                    let reqs = SyncRequirements {
                        pre: vec![(c_id, pre_hash)],
                        post: vec![(c_id, post_hash)],
                    };
                    let msg = SyncMessage::new(
                        SyncMessageType::AppendShelledDatas(c_id),
                        reqs,
                        hash_data,
                    );
                    let parts = msg.into_parts();
                    for part in parts {
                        let _ = to_gnome_sender.send(ToGnome::AddData(part));
                    }
                    let reqs = SyncRequirements {
                        pre: vec![(c_id, post_hash)],
                        post: vec![(c_id, post_hash)],
                    };
                    for data in data_vec {
                        let msg = SyncMessage::new(
                            SyncMessageType::UpdateData(c_id, next_data_idx),
                            reqs.clone(),
                            data,
                        );
                        next_data_idx += 1;
                        // eprintln!("pre_req: {:?}", reqs.pre);
                        // eprintln!("post_req: {:?}", reqs.post);
                        let parts = msg.into_parts();
                        // eprintln!("Parts count: {}", parts.len());
                        for part in parts {
                            // eprintln!("SyncData len: {}", part.len());
                            let _ = to_gnome_sender.send(ToGnome::AddData(part));
                        }
                    }
                }
            }
            ToAppData::AppendContent(d_type, data) => {
                if let Some(next_id) = app_data.next_c_id() {
                    let pre: Vec<(ContentID, u64)> = vec![(next_id, 0)];
                    let post: Vec<(ContentID, u64)> = vec![(next_id, data.get_hash())];
                    //TODO: we push this 0 to inform Content::from that we are dealing
                    // Not with a Content::Link, which is 255 but with Content::Data,
                    // whose DataType = 0
                    // We should probably send this in SyncMessage header instead
                    // let mut bytes = vec![0];
                    // bytes.append(&mut data.bytes());
                    // let data = Data::new(bytes).unwrap();
                    let reqs = SyncRequirements { pre, post };
                    let msg = SyncMessage::new(SyncMessageType::AppendContent(d_type), reqs, data);
                    let parts = msg.into_parts();
                    for part in parts {
                        let _ = to_gnome_sender.send(ToGnome::AddData(part));
                    }
                    // next_val += 1;
                }
            }
            ToAppData::AppendData(c_id, data) => {
                let pre_hash = app_data.content_root_hash(c_id).unwrap();
                // eprintln!("Initial AppendData PRE hash: {}", pre_hash.1);
                // let bottom_hashes = app_data.content_bottom_hashes(c_id).unwrap();
                // eprintln!("Initial AppendData bottom hashes: {:?}", bottom_hashes);
                let pre: Vec<(ContentID, u64)> = vec![(c_id, pre_hash.1)];
                let append_result = app_data.append_data(c_id, data);
                let post: Vec<(ContentID, u64)> = vec![(c_id, append_result.unwrap())];
                // eprintln!("Initial AppendData POST hash: {}", post[0].1);
                let data = app_data.pop_data(c_id).unwrap();
                // let bottom_hashes = app_data.content_bottom_hashes(c_id).unwrap();
                // eprintln!("Restored AppendData bottom hashes: {:?}", bottom_hashes);
                // let pre_hash = app_data.content_root_hash(c_id).unwrap();
                // eprintln!("Later AppendData PRE hash: {}", pre_hash.1);
                //TODO: we push this 0 to inform Content::from that we are dealing
                // Not with a Content::Link, which is 255 but with Content::Data,
                // whose DataType = 0
                // We should probably send this in SyncMessage header instead
                // let mut bytes = vec![0];
                // bytes.append(&mut data.bytes());
                // let data = Data::new(bytes).unwrap();
                let reqs = SyncRequirements { pre, post };
                let msg = SyncMessage::new(SyncMessageType::AppendData(c_id), reqs, data);
                let parts = msg.into_parts();
                for part in parts {
                    let _ = to_gnome_sender.send(ToGnome::AddData(part));
                }
            }
            // A local request from application to synchronize with swarm
            ToAppData::UpdateData(c_id, d_id, data) => {
                //TODO:serve this
                // eprintln!(
                //     "Got ToAppData::UpdateData({}, {}, Dlen: {})",
                //     c_id,
                //     d_id,
                //     data.len()
                // );
                let pre_hash = app_data.content_root_hash(c_id).unwrap();
                let pre: Vec<(ContentID, u64)> = vec![(c_id, pre_hash.1)];
                let prev_data = app_data
                    .update_data(c_id, d_id, Data::empty(data.get_hash()))
                    .unwrap();
                let post_hash = app_data.content_root_hash(c_id).unwrap();
                let post: Vec<(ContentID, u64)> = vec![(c_id, post_hash.1)];
                let _res = app_data.update_data(c_id, d_id, prev_data);
                let reqs = SyncRequirements { pre, post };
                let msg = SyncMessage::new(SyncMessageType::UpdateData(c_id, d_id), reqs, data);
                let parts = msg.into_parts();
                for part in parts {
                    let _ = to_gnome_sender.send(ToGnome::AddData(part));
                }
            }
            ToAppData::RemoveData(c_id, d_id) => {
                //TODO:serve this
                // eprintln!("Got ToAppData::RemoveData({}, {})", c_id, d_id,);
                let mut shell = app_data.shell(c_id).unwrap();
                // eprintln!("Bottom hashes before remove: {:?}", shell.data_hashes());
                let pre_hash = shell.hash();
                // let pre_hash = app_data.content_root_hash(c_id).unwrap();
                let pre: Vec<(ContentID, u64)> = vec![(c_id, pre_hash)];
                let _prev_data = shell.remove_data(d_id).unwrap();
                // eprintln!("Bottom hashes after remove: {:?}", shell.data_hashes());
                let post_hash = shell.hash();
                let b_hashes = shell.data_hashes();
                let mut a_tree = ContentTree::empty(0);
                for hash in b_hashes {
                    a_tree.append(Data::empty(hash));
                }
                let a_hash = a_tree.hash();
                if a_hash != post_hash {
                    panic!(
                        "ERR: hash after remove_data: {} != {} (append hash)",
                        post_hash, a_hash
                    );
                }
                // let post_hash = app_data.content_root_hash(c_id).unwrap();
                let post: Vec<(ContentID, u64)> = vec![(c_id, post_hash)];
                // let _res = app_data.insert_data(c_id, d_id, prev_data);
                let reqs = SyncRequirements { pre, post };
                let msg = SyncMessage::new(
                    SyncMessageType::RemoveData(c_id, d_id),
                    reqs,
                    Data::empty(0),
                );
                let parts = msg.into_parts();
                for part in parts {
                    let _ = to_gnome_sender.send(ToGnome::AddData(part));
                }
            }
            // A local request from application to synchronize with swarm
            ToAppData::ChangeContent(c_id, d_type, data_vec) => {
                let (curr_d_type, curr_content_len) = app_data.get_type_and_len(c_id).unwrap();
                let curr_hash = app_data.content_root_hash(c_id).unwrap();
                if curr_d_type == DataType::Link && curr_content_len > 0 {
                    // if let Ok(_hashes) = app_data.read_link_ti_data(c_id, 0, curr_d_type) {
                    eprintln!("We can not change a link with TransformInfo");
                    continue;
                    // }
                } else if curr_d_type != d_type {
                    eprintln!("We can not change content to different DataType");
                    continue;
                }
                // Step #1: We take hashes of provided Data from data_vec
                // There can be up to 65536 of 8-byte hashes so those can fill
                // up to 512 of 1024-byte Data blocks.
                let data_count = data_vec.len();
                let mut bottom_hashes_as_data_blocks = Vec::with_capacity((data_count >> 7) + 1);
                let mut hash_hash_data = Vec::with_capacity(4);
                let mut bottom_hashes = Vec::with_capacity(data_count);
                for data in &data_vec {
                    bottom_hashes.push(data.get_hash());
                }
                // TODO: check if only a single Data block has changed
                let existing_hashes = app_data.content_bottom_hashes(c_id).unwrap();
                let ex_len = existing_hashes.len();
                if ex_len == data_count {
                    let mut only_difference: Option<u16> = None;
                    for i in 0..data_count {
                        if bottom_hashes[i] != existing_hashes[i] {
                            if only_difference.is_some() {
                                only_difference = None;
                                break;
                            } else {
                                only_difference = Some(i as u16);
                            }
                        }
                    }
                    if let Some(d_id) = only_difference {
                        eprintln!("Transforming ChangeContent -> UpdateData");
                        let _ = app_data_send
                            .send(ToAppData::UpdateData(
                                c_id,
                                d_id,
                                data_vec[d_id as usize].clone(),
                            ))
                            .await;
                        continue;
                    }
                }
                // TODO: there can also be a case where a single data has been inserted
                // and all other data stays the same
                // TODO: there can also be a case where a single data has been removed
                // and all other data stays the same
                // eprintln!("bottom hashes: {:?}", bottom_hashes);
                // eprintln!("pre root hash: {}", get_root_hash(&bottom_hashes));
                let bottom_chunks = bottom_hashes.chunks_exact(128);
                let bottom_remainder = bottom_chunks.remainder();
                for chunk in bottom_chunks {
                    let mut bytes = Vec::with_capacity(1024);
                    for hash in chunk {
                        for byte in hash.to_be_bytes() {
                            bytes.push(byte);
                        }
                    }
                    bottom_hashes_as_data_blocks.push(Data::new(bytes).unwrap());
                }
                if !bottom_remainder.is_empty() {
                    let mut bytes = Vec::with_capacity(1024);
                    for hash in bottom_remainder {
                        for byte in hash.to_be_bytes() {
                            bytes.push(byte);
                        }
                    }
                    bottom_hashes_as_data_blocks.push(Data::new(bytes).unwrap());
                }

                // If data_count <= 128 we can immediately update entire Content,
                // by sending a SyncMessage with all the hashes of new Data.
                // So in this case we go directly to Step #4, skipping Steps #2 and #3.
                //
                //
                // Step #2: If data_count > 128 Data blocks we need to make second
                // round for taking hashes from resulting HashData blocks
                //  - now there can be up to 4 new HashHashData blocks.
                if data_count > 128 {
                    panic!("THIS IS NOT FULLY IMPLEMENTED!!!");
                    let hash_hash_chunks = bottom_hashes_as_data_blocks.chunks_exact(128);
                    let hash_hash_remainder = hash_hash_chunks.remainder();

                    for chunk in hash_hash_chunks {
                        let mut bytes = Vec::with_capacity(1024);
                        for data in chunk {
                            for byte in data.get_hash().to_be_bytes() {
                                bytes.push(byte);
                            }
                        }
                        hash_hash_data.push(Data::new(bytes).unwrap());
                    }
                    if !hash_hash_remainder.is_empty() {
                        let mut bytes = Vec::with_capacity(1024);
                        for data in hash_hash_remainder {
                            for byte in data.get_hash().to_be_bytes() {
                                bytes.push(byte);
                            }
                        }
                        hash_hash_data.push(Data::new(bytes).unwrap());
                    }
                    //
                    //
                    // Step #3: If we needed two rounds to fit hashes into single SyncMessage the
                    // procedure has to be more complicated.
                    // There can be two scenarios that can happen here:
                    // 1. We have enough data_ids available to hold all Data blocks from
                    // first round of counting hashes (up to 512 new Data blocks);
                    // 2. we do not have enough data_ids.
                    let available_data_ids = u16::MAX - curr_content_len;
                    if available_data_ids > data_count as u16 {
                        //
                        // Scenario 1:
                        // We send a SyncMessage requesting to append multiple Datas to current
                        // CID with provided list of hashes - here we change root hash of
                        // our Content.
                        // Then we send multiple Updates with actual HashHashData corresponding
                        // to hashes from AppendMultipleDatas SyncMessage.
                        // Next we send one more SyncMessage that requests to
                        // pop all recently appended HashHashDatas (there can only be up to 4 of those)
                        // use their contents as HashData hashes and once again expand CID with as many
                        // Data::empty(hash) as needed.
                        // This action is a second change of Content root hash.
                        // Now we need to fill those appended Data::empty(hash) slots with actual data.
                        // This Data blocks contain all of the bottom hashes of our Content ID.
                        //
                    } else {
                        // Scenario 2:
                        // Here we need to first make enough room for as many new Data blocks as needed
                        // (changing Content's root hash) and then proceed with scenario 1.
                        // This Data removal can be done in a brute-force way by just removing
                        // last N Data blocks,
                        // or in a more sophisticated way removing selected data_ids starting from
                        // data_ids with greatest value and moving down.
                        // One SyncMessage is enough for both scenarios so second one is preffered.
                    }
                    //
                    // So Step #3 requires at least two additional changes to Content root hash.
                    // But right now we do not have any mechanism preventing other Data changes
                    // in between. If there are any and we have precalculated pre- and post- hashes
                    // then those values become obsolete and entire procedure fails with some
                    // invalid Data blocks somewhere in ContentTree.
                    // On the other hand if we calculate every pre- and post- hash just before
                    // sending a SyncMessage those other changes that happened in between will get
                    // wiped out.
                    // So for now it is recommended to first make sure that there is no other
                    // change being submitted to given swarm that modifies given Content
                    // that is undergoing extended change content process.
                    // A solution involving a Reconfigure message comes to mind but for now
                    // this entire procedure is complicated enough to give one a headache.

                    //TODO: implement above
                } else {
                    //
                    //
                    // Step #4: A SyncMessage requesting change of entire Content is issued.
                    // In this step Content root hash is updated with final hash value.
                    // We move all existing Data blocks aside into a HashMap<Hash, Data>
                    // and starting from scratch push Data::empty(hash) one by one into CID.
                    // If there happens to be a match for given hash in a hashmap we've just
                    // created we take that Data and put it in place.
                    //
                    // Additionally we can send all remaining Data blocks in an UpdateData
                    // sync message that does not change Content's root hash.
                    //
                    // All of above means entire Swarm is always synchronized,
                    // so even if a Gnome joins in the middle of this procedure,
                    //  he will stay synced (will just need to ask his Neighbors for
                    // Data to catch up).
                    // TODO: implement above
                    let new_hash = get_root_hash(&bottom_hashes);
                    // eprintln!("New hash: {}", new_hash);
                    let reqs = SyncRequirements {
                        pre: vec![(c_id, curr_hash.1)],
                        post: vec![(c_id, new_hash)],
                    };
                    let msg = SyncMessage::new(
                        SyncMessageType::ChangeContent(d_type, c_id),
                        reqs,
                        bottom_hashes_as_data_blocks[0].clone(),
                    );
                    let parts = msg.into_parts();
                    for part in parts {
                        let _ = to_gnome_sender.send(ToGnome::AddData(part));
                    }

                    //TODO: now we need to send actual Data as well
                    // TODO: we should only send Data blocks that did not exist before CC
                    let reqs = SyncRequirements {
                        pre: vec![(c_id, new_hash)],
                        post: vec![(c_id, new_hash)],
                    };
                    let existing_bottom_hashes = app_data.content_bottom_hashes(c_id).unwrap();
                    for (d_id, data) in data_vec.iter().enumerate() {
                        if existing_bottom_hashes.contains(&data.get_hash()) {
                            eprintln!("Not sending Page {}-{} as it was not changed", c_id, d_id);
                            continue;
                        }
                        let msg = SyncMessage::new(
                            SyncMessageType::UpdateData(c_id, d_id as u16),
                            reqs.clone(),
                            data.clone(),
                        );
                        let parts = msg.into_parts();
                        for part in parts {
                            let _ = to_gnome_sender.send(ToGnome::AddData(part));
                        }
                    }
                }
                //
                //
                // Following is old solution
                // eprintln!(
                //     "app data received CC request… for {} with {} data blocks",
                //     c_id,
                //     data_vec.len()
                // );
                // let mut content_to_work_on = app_data.clone_content(c_id).unwrap();
                // for data in &data_vec {
                //     eprintln!("Hash of data to add: {:?}", data.get_hash().to_be_bytes());
                // }
                // let pre_hash = content_to_work_on.hash();
                // let existing_d_type = content_to_work_on.data_type();
                // // OK, so in order to apply multiple changes we need to:
                // // - verify if d_type allows for this change
                // if existing_d_type == DataType::Link {
                //     if let Ok(_hashes) = content_to_work_on.link_ti_hashes() {
                //         eprintln!("We can not change a link with TransformInfo");
                //         continue;
                //     }
                // } else if existing_d_type != d_type {
                //     eprintln!("We can not change content to different DataType");
                //     continue;
                // }
                // // - make sure there is multiple data blocks that need to change
                // //   and not just one, or even zero
                // let existing_hashes = content_to_work_on.data_hashes();
                // let mut new_hashes = Vec::with_capacity(data_vec.len());
                // for data in &data_vec {
                //     new_hashes.push(data.get_hash());
                // }
                // let existing_len = existing_hashes.len();
                // let new_len = new_hashes.len();
                // let mut diff_indices = vec![];
                // let mut append_indices = vec![];
                // let mut shrink_indices = vec![];
                // if existing_len <= new_len {
                //     eprintln!("existing <= new");
                //     for i in 0..existing_len {
                //         if existing_hashes[i] != new_hashes[i] {
                //             diff_indices.push(i);
                //         }
                //     }
                //     for i in existing_len..new_len {
                //         append_indices.push(i);
                //     }
                // } else {
                //     eprintln!("existing > new");
                //     //We are shrinking content
                //     for i in new_len..existing_len {
                //         shrink_indices.push(i);
                //     }
                //     for i in 0..new_len {
                //         if existing_hashes[i] != new_hashes[i] {
                //             diff_indices.push(i);
                //         }
                //     }
                // }
                // //TODO: send request to pop shrinking data blocks
                // // TODO: this can be a 3-step procedure on a swarm,
                // // and other users might perform changes in between those
                // // steps, so it is not guaranteed to succeed completely.
                // // Probably we should find a better way
                // // Maybe enqueue subsequent changes from diff_indices?
                // if !shrink_indices.is_empty() {
                //     eprintln!("We should pop some Data blocks from CID: {}", c_id);
                //     //TODO: new we need to update pre_hash for subsequent requests
                // } else if new_len - existing_len > 1 {
                //     //TODO: send request to reserve new data blocks
                //     eprintln!("We should reserve some new Data blocks for CID: {}", c_id);
                //     //TODO: new we need to update pre_hash for subsequent requests
                //     let mut new_hashes = vec![];
                //     let mut new_datas = vec![];
                //     for idx in &append_indices {
                //         new_datas.push(data_vec[*idx].clone());
                //     }
                //     for index in &append_indices {
                //         for byte in data_vec[*index].get_hash().to_be_bytes() {
                //             new_hashes.push(byte);
                //         }
                //     }
                //     let _ = app_data_send
                //         .send(ToAppData::AppendShelledDatas(
                //             c_id,
                //             Data::new(new_hashes).unwrap(),
                //             new_datas,
                //         ))
                //         .await;
                // } else if new_len - existing_len == 1 {
                //     eprintln!(
                //         "New len {} - existing len {} = {}",
                //         new_len,
                //         existing_len,
                //         new_len - existing_len
                //     );
                //     eprintln!("Append indices: {:?}", append_indices);
                //     let _ = app_data_send
                //         .send(ToAppData::AppendData(
                //             c_id,
                //             data_vec[append_indices[0]].clone(),
                //         ))
                //         .await;
                // }

                // //TODO: for every changed block apply changes one by one
                // // We assume no data was appended nor removed!!!
                // for index in diff_indices {
                //     eprintln!("we should do something with idx: {}", index);
                //     let hash_bundle = app_data.content_root_hash(c_id).unwrap();
                //     let old_data = app_data
                //         .update_data(c_id, index as u16, data_vec[index].clone())
                //         .unwrap();
                //     let new_hash_bundle = app_data.content_root_hash(c_id).unwrap();
                //     let reqs = SyncRequirements {
                //         pre: vec![(c_id, hash_bundle.1)],
                //         post: vec![(c_id, new_hash_bundle.1)],
                //     };
                //     let _data = app_data.update_data(c_id, index as u16, old_data);
                //     let msg = SyncMessage::new(
                //         SyncMessageType::UpdateData(c_id, index as u16),
                //         reqs,
                //         data_vec[index].clone(),
                //     );
                //     let parts = msg.into_parts();
                //     for part in parts {
                //         let _ = to_gnome_sender.send(ToGnome::AddData(part));
                //     }
                // }
            }
            ToAppData::ListNeighbors => {
                let _ = to_gnome_sender.send(ToGnome::ListNeighbors);
            }
            ToAppData::TransformLinkRequest(_sync_data) => {
                eprintln!("Sending TransformLinkRequest to gnome");
                if let Some(c_id) = link_with_transform_info {
                    let sync_data = SyncData::new(c_id.to_be_bytes().into()).unwrap();
                    let _ = to_gnome_sender.send(ToGnome::Reconfigure(0, sync_data));
                    link_with_transform_info = None;
                } else {
                    eprintln!("No link with TransformInfo defined for transformation to begin");
                }
            }
            ToAppData::TransformLink(s_data) => {
                eprintln!("Received TransformLink from gnome");
                let s_bytes = s_data.bytes();
                let c_id = u16::from_be_bytes([s_bytes[0], s_bytes[1]]);
                let result = app_data.transform_link(c_id);
                eprintln!("Link transformation result: {:?}", result);
            }
            ToAppData::UploadData => {
                //TODO: send to AppMgr UploadData message
                // this logic should be moved to app mgr
                if b_cast_origin.is_none() {
                    eprintln!("Unable to upload - no active broadcast.");
                    if !b_req_sent {
                        eprintln!("Requesting broadcast channel.");
                        let res = to_gnome_sender.send(ToGnome::StartBroadcast);
                        b_req_sent = res.is_ok();
                    }
                    continue;
                }
                let (broadcast_id, bcast_send) = b_cast_origin.clone().unwrap();
                // TODO: here we need to write a procedure for data upload
                // 1. Select data to upload
                // 2. Split data into 64MibiByte chunks
                //
                let d_type = DataType::from(7);
                let total_parts = 128;
                let big_chunks = vec![BigChunk(0, total_parts)];
                // Then for each big-chunk:
                for mut big_chunk in big_chunks.into_iter() {
                    let description = content::Description::new(String::new()).unwrap();
                    let missing_hashes = HashSet::new();
                    let data_hashes = vec![];
                    let mut data_vec = Vec::with_capacity(big_chunk.1 as usize);
                    let mut hashes = Vec::with_capacity(big_chunk.1 as usize);
                    eprintln!("// 3. Split big-chunk into 1024byte small-chunks");
                    while let Some(small_chunk) = big_chunk.next() {
                        // 4. Compute hash for each small-chunk
                        hashes.push(small_chunk.get_hash());
                        // TODO: build proper CastData from Data
                        data_vec.push(small_chunk);
                    }
                    eprintln!("// 5. Compute root hash from previous hashes.");
                    let root_hash = get_root_hash(&hashes);
                    eprintln!("// 6. Instantiate TransformInfo");
                    let ti = TransformInfo {
                        d_type,
                        size: 0,
                        root_hash,
                        broadcast_id,
                        // description,
                        missing_hashes,
                        data_hashes,
                        data: HashMap::new(),
                    };

                    eprintln!(
                        "// 7. SyncMessage::Append as many Data::Link to Datastore as necessary"
                    );
                    if let Some(content_id) = app_data.next_c_id() {
                        eprintln!("ContentID: {}", content_id);
                        let pre: Vec<(ContentID, u64)> = vec![(content_id, 0)];
                        let link = Content::Link(
                            SwarmName {
                                founder: GnomeId(u64::MAX),
                                name: String::new(),
                            },
                            u16::MAX,
                            description,
                            Data::empty(0),
                            Some(ti),
                        );
                        let link_hash = link.hash();
                        eprintln!("Link hash: {}", link_hash);
                        let data = link.to_data().unwrap();
                        eprintln!("Link data: {:?}", data);
                        eprintln!("Data hash: {}", data.get_hash());
                        let post: Vec<(ContentID, u64)> = vec![(content_id, link_hash)];
                        let reqs = SyncRequirements { pre, post };
                        let msg = SyncMessage::new(
                            SyncMessageType::AppendContent(DataType::Link),
                            reqs,
                            data,
                        );
                        let parts = msg.into_parts();
                        for part in parts {
                            let _ = to_gnome_sender.send(ToGnome::AddData(part));
                        }
                        //TODO: we need to set this upon receiving Gnome's confirmation
                        link_with_transform_info = Some(content_id);
                        // next_val += 1;
                        eprintln!("// 8. For each Link Send computed Hashes via broadcast");
                        let (done_send, done_recv) = channel();
                        let mut hash_bytes = vec![];
                        let chunks = hashes.chunks(128);
                        let total = chunks.len() - 1;
                        for (i, chunk) in chunks.enumerate() {
                            let mut outgoing_bytes = Vec::with_capacity(1024);
                            for hash in chunk {
                                for byte in u64::to_be_bytes(*hash) {
                                    outgoing_bytes.push(byte)
                                }
                            }
                            hash_bytes.push(
                                AppMessage::new(
                                    content_id,
                                    true,
                                    i as u16,
                                    total as u16,
                                    Data::new(outgoing_bytes).unwrap(),
                                )
                                .to_cast(),
                            )
                        }
                        eprintln!("duplicating hashes");
                        hash_bytes.append(&mut hash_bytes.clone());
                        eprintln!("spawning serve_broadcast_origin");
                        spawn(serve_broadcast_origin(
                            broadcast_id,
                            Duration::from_millis(512),
                            bcast_send.clone(),
                            hash_bytes,
                            done_send.clone(),
                        ));
                        sleep(sleep_time).await;
                        let _done_res = done_recv.recv();
                        eprintln!("Hashes sent: {}", _done_res.is_ok());
                        // TODO
                        // 9. SyncMessage::Transform a Link into Data

                        //10. Send Data chunks via broadcast
                        let mut c_data_vec = Vec::with_capacity(data_vec.len());
                        let total_parts = total_parts - 1;
                        for (i, data) in data_vec.into_iter().enumerate() {
                            c_data_vec.push(
                                AppMessage::new(content_id, false, i as u16, total_parts, data)
                                    .to_cast(),
                            )
                        }
                        spawn(serve_broadcast_origin(
                            broadcast_id,
                            Duration::from_millis(512),
                            bcast_send.clone(),
                            c_data_vec,
                            done_send.clone(),
                        ));
                    }
                }
            }
            ToAppData::Response(GnomeToApp::Neighbors(s_id, neighbors)) => {
                let _ = to_app_mgr_send.send(ToAppMgr::NeighborsListing(s_id, neighbors));
            }
            ToAppData::SendFirstPage(n_id, c_id, data) => {
                let sync_type = 0;
                let (data_type, len) = app_data.get_type_and_len(c_id).unwrap();
                let sync_response = SyncResponse::Page(c_id, data_type, 0, len, data);
                // println!("Sending bytes: {:?}", bytes);
                let res = to_gnome_sender.send(ToGnome::SendData(
                    n_id,
                    NeighborResponse::Custom(
                        sync_type,
                        CastData::new(sync_response.serialize()).unwrap(),
                    ),
                ));
                if res.is_ok() {
                    eprintln!("SFP Main page of {} sent successfully.", c_id,);
                }
            }
            ToAppData::Response(GnomeToApp::Block(_id, data)) => {
                // println!("Processing data...");
                let process_result = app_data.process(data);
                if process_result.is_none() {
                    continue;
                }
                // println!("Process response: {:?}", process_result);
                let SyncMessage {
                    m_type,
                    requirements,
                    data,
                } = process_result.unwrap();

                // println!("Received m_type: {:?}", m_type);
                match m_type {
                    // SyncMessageType::SetManifest => {
                    //     let old_manifest = app_data.get_all_data(0);
                    //     if !requirements.pre_validate(0, &app_data) {
                    //         eprintln!("PRE validation failed");
                    //     } else {
                    //         let content = Content::Data(0, ContentTree::Filled(data));
                    //         let next_id = app_data.next_c_id().unwrap();
                    //         let res = if next_id == 0 {
                    //             app_data.append(content).is_ok()
                    //         } else {
                    //             app_data.update(0, content).is_ok()
                    //         };
                    //         // println!("Manifest result: {:?}", res);
                    //         if !requirements.post_validate(0, &app_data) {
                    //             eprintln!("POST validation failed");
                    //             if let Ok(data_vec) = old_manifest {
                    //                 let c_tree = ContentTree::from(data_vec);
                    //                 let old_content = Content::Data(0, c_tree);
                    //                 let res = app_data.update(0, old_content);
                    //                 eprintln!("Restored old manifest {:?}", res.is_ok());
                    //             } else {
                    //                 let content = Content::Data(0, ContentTree::Empty(0));
                    //                 let _ = app_data.update(0, content);
                    //                 eprintln!("Zeroed manifest");
                    //             }
                    //         }
                    //         let hash = app_data.root_hash();
                    //         eprintln!("Sending updated hash: {}", hash);
                    //         let res = to_gnome_sender.send(ToGnome::UpdateAppRootHash(hash));
                    //         eprintln!("Send res: {:?}", res);
                    //         // println!("Root hash: {}", app_data.root_hash());
                    //     }
                    // }
                    SyncMessageType::AppendContent(d_type) => {
                        // TODO: potentially for AddContent & ChangeContent
                        // post requirements could be empty
                        // pre requirements can not be empty since we need
                        // ContentID
                        let next_id = app_data.next_c_id().unwrap();
                        if !requirements.pre_validate(next_id, &app_data) {
                            eprintln!("PRE validation failed for AddContent");
                        } else {
                            if requirements.post.len() != 1 {
                                eprintln!(
                                    "POST validation failed for AddContent 1 ({:?})",
                                    requirements.post
                                );
                                continue;
                            }
                            let content = Content::from(d_type, data).unwrap();
                            // println!("Content: {:?}", content);
                            let (recv_id, recv_hash) = requirements.post[0];
                            if recv_id == next_id && recv_hash == content.hash() {
                                let d_type = content.data_type();
                                let main_page = if let Ok(d) = content.read_data(0) {
                                    d
                                } else {
                                    Data::empty(0)
                                };
                                let _res = app_data.append(content);
                                // eprintln!("Content added: {:?}", res);
                                // let hash = app_data.root_hash();
                                // eprintln!("Sending updated hash: {}", hash);
                                // let _res = to_gnome_sender.send(ToGnome::UpdateAppRootHash(hash));
                                // eprintln!("Send res: {:?}", res);
                                let _to_mgr_res = to_app_mgr_send.send(ToAppMgr::ContentAdded(
                                    swarm_id, recv_id, d_type, main_page,
                                ));
                            } else {
                                eprintln!("Recv id: {}, next id: {}", recv_id, next_id);
                                eprintln!(
                                    "Recv hash: {}, next hash: {}",
                                    recv_hash,
                                    content.hash()
                                );
                                eprintln!("POST validation failed for AddContent two");
                            }
                            // println!("Root hash: {}", app_data.root_hash());
                        }
                    }
                    SyncMessageType::ChangeContent(d_type, c_id) => {
                        // println!("ChangeContent");
                        //TODO: we need to verify we actually can change content
                        // this is possible only if existing and new d_types match
                        // or if existing d_type is Link
                        if !requirements.pre_validate(c_id, &app_data) {
                            eprintln!("PRE validation failed for ChangeContent");
                            continue;
                        }
                        //TODO: take 8-byte hashes from data and push Data::empty(hash)
                        // to new Content
                        let bytes = data.bytes();
                        let mut byte_groups_iter = bytes.chunks_exact(8);
                        let new_data_len = byte_groups_iter.len() as u16;
                        let first_hash = u64::from_be_bytes(
                            byte_groups_iter.next().unwrap().try_into().unwrap(),
                        );
                        let mut new_hashes = vec![first_hash];
                        // eprintln!("New content\n{}", first_hash);
                        let mut new_content =
                            Content::from(d_type, Data::empty(first_hash)).unwrap();
                        // eprintln!("New content hash: {}", new_content.hash());
                        while let Some(bytes_group) = byte_groups_iter.next() {
                            let hash = u64::from_be_bytes(bytes_group.try_into().unwrap());
                            // eprintln!("push: {}", hash);
                            let _ = new_content.push_data(Data::empty(hash));
                            new_hashes.push(hash);
                            // let d_hashes = new_content.data_hashes();
                            // for d_hash in d_hashes {
                            // eprintln!("NCDH: {:?}", d_hashes);
                            // }
                            // eprintln!("New content hash: {}", new_content.hash());
                        }
                        // let d_hashes = new_content.data_hashes();
                        // for d_hash in d_hashes {
                        // eprintln!("Final NCDH: {:?}", d_hashes);
                        // }
                        let res = app_data.update(c_id, new_content);
                        if let Ok(mut old_content) = res {
                            if !requirements.post_validate(c_id, &app_data) {
                                let restore_res = app_data.update(c_id, old_content);
                                eprintln!("POST validation failed on ChangeContent");
                                eprintln!("Restore result: {:?}", restore_res);
                            } else {
                                //TODO: insert any old Data with same hash into new content
                                let mut old_data =
                                    HashMap::with_capacity(old_content.len() as usize);
                                let mut first_hash_old = 0;
                                while let Ok(data) = old_content.pop_data() {
                                    first_hash_old = data.get_hash();
                                    // eprintln!("old hash after pop: {}", old_content.hash());
                                    old_data.insert(first_hash_old, data);
                                }
                                for (d_id, hash) in new_hashes.iter().enumerate() {
                                    if let Some(data) = old_data.remove(&hash) {
                                        eprintln!(
                                            "Restoring Page {}-{} from existing data",
                                            c_id, d_id
                                        );
                                        let _ = app_data.update_data(c_id, d_id as u16, data);
                                    }
                                }
                                let main_page_option = if first_hash_old == first_hash {
                                    None
                                } else {
                                    Some(Data::empty(first_hash))
                                };
                                let _to_mgr_res = to_app_mgr_send.send(ToAppMgr::ContentChanged(
                                    swarm_id,
                                    c_id,
                                    d_type,
                                    main_page_option,
                                ));
                                let hash = app_data.root_hash();
                                // let _res = to_gnome_sender.send(ToGnome::UpdateAppRootHash(hash));
                                eprintln!("ChangeContent completed successfully ({})", hash);
                            }
                        } else {
                            eprintln!("Update procedure failed: {:?}", res);
                        }
                    }
                    SyncMessageType::AppendShelledDatas(c_id) => {
                        eprintln!("SyncMessageType::AppendShelledDatas");
                        if !requirements.pre_validate(c_id, &app_data) {
                            eprintln!("PRE validation failed for AppendShelledDatas");
                            continue;
                        }
                        //TODO: we unpack hashes from received data, and for each
                        // hash we create an Data::empty(hash) that we append to
                        // given c_id in order received
                        // let mut bytes = data.bytes().into_iter();
                        let bytes = data.bytes();
                        let mut hashes = vec![];
                        for hash_bytes in bytes.chunks_exact(8) {
                            let hash = u64::from_be_bytes(hash_bytes.try_into().unwrap());
                            hashes.push(hash);
                        }
                        let to_add_total = hashes.len();
                        let hash_iter = hashes.iter();
                        // let mut all_res_ok = true;
                        let mut added_count = 0;
                        for hash in hash_iter {
                            let res = app_data.append_data(c_id, Data::empty(*hash));
                            if res.is_ok() {
                                added_count += 1;
                            } else {
                                eprintln!("Failure adding shelled data: {:?}", res.err().unwrap());
                                break;
                            }
                        }
                        if added_count == to_add_total {
                            if !requirements.post_validate(c_id, &app_data) {
                                eprintln!("POST validation failed for AppendShelledDatas");
                                // TODO: restore previous order
                                for i in 0..added_count {
                                    let res = app_data.pop_data(c_id);
                                    eprintln!("Pop {} result: {:?}", i, res);
                                }
                            } else {
                                let hash = app_data.root_hash();
                                // eprintln!("Sending updated hash: {}", hash);
                                // let res = to_gnome_sender.send(ToGnome::UpdateAppRootHash(hash));
                                // eprintln!("Send res: {:?}", res);
                                eprintln!(
                                    "Data shells appended successfully ({}, added: {})",
                                    hash, added_count
                                );
                            }
                        } else {
                            for i in 0..added_count {
                                let res = app_data.pop_data(c_id);
                                eprintln!("Pop {} result: {:?}", i, res);
                            }
                        }
                    }
                    SyncMessageType::AppendData(c_id) => {
                        //TODO
                        eprintln!("SyncMessageType::AppendData ");
                        // eprintln!(
                        //     "Sync Append bottom hashes before: {:?}",
                        //     app_data.content_bottom_hashes(c_id).unwrap()
                        // );
                        if !requirements.pre_validate(c_id, &app_data) {
                            eprintln!("PRE validation failed for AppendData");
                            continue;
                        }
                        // TODO
                        let res = app_data.append_data(c_id, data);
                        if res.is_ok() {
                            if !requirements.post_validate(c_id, &app_data) {
                                eprintln!("POST validation failed for AppendData");
                                // TODO: restore previous order
                                let res = app_data.pop_data(c_id);
                                eprintln!("Restore result: {:?}", res);
                            } else {
                                let hash = app_data.root_hash();
                                // eprintln!("Sending updated hash: {}", hash);
                                // let _res = to_gnome_sender.send(ToGnome::UpdateAppRootHash(hash));
                                // eprintln!("Send res: {:?}", res);
                                eprintln!("Data appended successfully ({})", hash);
                            }
                        }
                    }
                    SyncMessageType::RemoveData(c_id, d_id) => {
                        //TODO
                        eprintln!("SyncMessageType::RemoveData ");
                        if !requirements.pre_validate(c_id, &app_data) {
                            eprintln!("PRE validation failed for RemoveData");
                            continue;
                        }
                        // TODO
                        // eprintln!(
                        //     "CID-{} root hash befor remove: {}",
                        //     c_id,
                        //     app_data.content_root_hash(c_id).unwrap().1
                        // );
                        let res = app_data.remove_data(c_id, d_id);
                        if let Ok(removed_data) = res {
                            if !requirements.post_validate(c_id, &app_data) {
                                eprintln!("POST validation failed for RemoveData");
                                // TODO: restore previous order
                                // TODO: make sure hash after insert_data is equal to
                                //       that before removal!!!
                                let res = app_data.insert_data(c_id, d_id, removed_data);
                                // eprintln!(
                                //     "CID-{} root hash after restoration: {}",
                                //     c_id,
                                //     app_data.content_root_hash(c_id).unwrap().1
                                // );
                                eprintln!("Restore result: {:?}", res);
                            } else {
                                eprintln!("Data removed successfully ({})", app_data.root_hash());
                            }
                        }
                    }
                    SyncMessageType::UpdateData(c_id, d_id) => {
                        //TODO
                        eprintln!("SyncMessageType::UpdateData {}-{}", c_id, d_id);
                        let (d_type, len) = app_data.get_type_and_len(c_id).unwrap();
                        // eprintln!("Len: {}", len);
                        // let bot_hashes = app_data.content_bottom_hashes(c_id).unwrap();
                        // for bot in bot_hashes {
                        //     eprintln!("hash: {}", bot);
                        // }
                        if !requirements.pre_validate(c_id, &app_data) {
                            eprintln!("PRE validation failed for UpdateData");
                            continue;
                        }
                        // TODO
                        // let (_t, len) = app_data.get_type_and_len(c_id).unwrap();
                        // eprintln!("Before update len: {}", len);
                        // eprintln!("all current hashes: {}", c_id);
                        // let bottoms = app_data.get_all_page_hashes(c_id).unwrap();
                        // for bot in bottoms {
                        //     eprintln!("hash2: {}", bot);
                        // }
                        let main_page_option = if d_id == 0 { Some(data.clone()) } else { None };
                        let res = app_data.update_data(c_id, d_id, data);
                        if let Ok(updated_data) = res {
                            if !requirements.post_validate(c_id, &app_data) {
                                eprintln!("POST validation failed for UpdateData");
                                // TODO: restore previous order
                                let res = app_data.update_data(c_id, d_id, updated_data);
                                eprintln!("Restore result: {:?}", res);
                            } else {
                                let hash = app_data.root_hash();
                                // eprintln!("Sending updated hash: {}", hash);
                                // let _res = to_gnome_sender.send(ToGnome::UpdateAppRootHash(hash));
                                // eprintln!("Send res: {:?}", res);
                                eprintln!("Data updated successfully ({})", hash);
                                let _to_mgr_res = to_app_mgr_send.send(ToAppMgr::ContentChanged(
                                    swarm_id,
                                    c_id,
                                    d_type,
                                    main_page_option,
                                ));
                            }
                        } else {
                            eprintln!("UpdateData failed: {}", res.err().unwrap());
                        }
                    }
                    SyncMessageType::InsertData(c_id, d_id) => {
                        //TODO
                        eprintln!("SyncMessageType::InsertData ");
                    }
                    SyncMessageType::ExtendData(c_id, d_id) => {
                        //TODO
                        eprintln!("SyncMessageType::ExtendData ");
                    }
                    SyncMessageType::UserDefined(_value) => {
                        //TODO
                        eprintln!("SyncMessageType::UserDefined({})", _value);
                    }
                }
            }
            ToAppData::CustomRequest(m_type, neighbor_id, cast_data) => {
                //TODO: Cases not cover here should be allowed to be served by dapp-lib's user
                // We need to cover different bootstrap sync requests:
                // - root hashes only of all Contents
                // - data/page hashes of specified ContentIDs
                //  In future we could define a generic sync request to send
                //   back Contents that have been tagged with specific tags.
                // - actual Content Data (aka future: Pages) for specified ContentIDs
                //   Here we should distinguish between sending all pages or only specific
                //   ones like only first page that usually consists of description of a
                //   content.
                // In order to do so, we should only take a single m_type value,
                // following bytes should be used to further specify our request
                //
                // When sending Response we should also cover the same cases as above,
                // also by using a single m_type value.
                // Special case to consider is when we are sending main Page of a Link.
                // Link has optional TransformInfo attribute, and it is necessary
                // when sideloading Pages via broadcast or other means than SyncMessages.
                match m_type {
                    0 => {
                        let sync_requests = deserialize_requests(cast_data.bytes());
                        // println!("Sync Request: {:?}", sync_requests);
                        // println!("Got AppSync inquiry");
                        // TODO: we have to unconditionally sync partial_data!!!
                        let sync_type = 0;
                        let partial_hashes = app_data.get_partial_hashes();
                        for hdata in partial_hashes.into_iter() {
                            let sync_response = SyncResponse::Partial(true, hdata);
                            let _ = to_gnome_sender.send(ToGnome::SendData(
                                neighbor_id,
                                NeighborResponse::Custom(
                                    sync_type,
                                    CastData::new(sync_response.serialize()).unwrap(),
                                ),
                            ));
                        }
                        let partial_datas = app_data.get_partial_data();
                        for hdata in partial_datas.into_iter() {
                            let sync_response = SyncResponse::Partial(false, hdata);
                            let _ = to_gnome_sender.send(ToGnome::SendData(
                                neighbor_id,
                                NeighborResponse::Custom(
                                    sync_type,
                                    CastData::new(sync_response.serialize()).unwrap(),
                                ),
                            ));
                        }
                        eprintln!("Sent Partial response");
                        let mut sync_req_iter = sync_requests.into_iter();
                        while let Some(req) = sync_req_iter.next() {
                            match req {
                                SyncRequest::Datastore => {
                                    // TODO: here we should trigger a separate task for sending
                                    // all root hashes to specified Neighbor
                                    // let sync_type = 0;
                                    let hashes = app_data.all_content_typed_root_hashes();
                                    let hashes_len = hashes.len() as u16;
                                    for (part_no, group) in hashes.into_iter().enumerate() {
                                        let sync_response = SyncResponse::Datastore(
                                            part_no as u16,
                                            hashes_len - 1,
                                            group,
                                        );
                                        let _ = to_gnome_sender.send(ToGnome::SendData(
                                            neighbor_id,
                                            NeighborResponse::Custom(
                                                sync_type,
                                                CastData::new(sync_response.serialize()).unwrap(),
                                            ),
                                        ));
                                    }
                                    eprintln!("Sent Datastore response");
                                }
                                SyncRequest::AllFirstPages(tags_opt) => {
                                    // TODO: here we should trigger a separate task for sending
                                    // all Content's main pages to specified Neighbor
                                    eprintln!(
                                        "We are requested to send all first pages to neighbor."
                                    );
                                    if let Some(tags) = tags_opt {
                                        //TODO: we send tags and all first pages to App and wait
                                        // when we get response we send first pages to Neighbor
                                        // (Different apps may implement different tagging strategies)
                                        eprintln!("Only first pages of select Contents");

                                        let next_c_id = app_data.next_c_id().unwrap();
                                        let mut all_first_pages =
                                            Vec::with_capacity(next_c_id as usize);
                                        for c_id in 1..next_c_id {
                                            if let Ok(data) = app_data.read_data(c_id, 0) {
                                                all_first_pages.push((c_id, data));
                                                // let (data_type, len) =
                                                //     app_data.get_type_and_len(c_id).unwrap();
                                                // let sync_response = SyncResponse::Page(
                                                //     c_id, data_type, 0, len, data,
                                                // );
                                            }
                                        }
                                        let _ = to_app_mgr_send.send(ToAppMgr::GetCIDsForTags(
                                            swarm_id,
                                            neighbor_id,
                                            tags,
                                            all_first_pages,
                                        ));
                                    } else {
                                        let sync_type = 0;
                                        for c_id in 1..app_data.next_c_id().unwrap() {
                                            eprintln!(
                                                "We need to send main page of ContentID-{}",
                                                c_id
                                            );
                                            if let Ok(data) = app_data.read_data(c_id, 0) {
                                                let (data_type, len) =
                                                    app_data.get_type_and_len(c_id).unwrap();
                                                let sync_response = SyncResponse::Page(
                                                    c_id, data_type, 0, len, data,
                                                );
                                                // println!("Sending bytes: {:?}", bytes);
                                                let res = to_gnome_sender.send(ToGnome::SendData(
                                                    neighbor_id,
                                                    NeighborResponse::Custom(
                                                        sync_type,
                                                        CastData::new(sync_response.serialize())
                                                            .unwrap(),
                                                    ),
                                                ));
                                                if res.is_ok() {
                                                    eprintln!(
                                                        "Main page of {} sent successfully.",
                                                        c_id,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                // SyncRequest::Hashes(c_id, d_type, h_ids) => {
                                SyncRequest::Hashes(c_id, h_ids) => {
                                    //TODO
                                    eprintln!(
                                        "We should send some page hashes of Contents: {:?}",
                                        c_id
                                    );
                                    let hash_res = if let Ok((data_type, _len)) =
                                        app_data.get_type_and_len(c_id)
                                    {
                                        // if data_type == d_type {
                                        if data_type == DataType::Link {
                                            app_data.get_all_transform_info_hashes(c_id)
                                        } else {
                                            // Err(AppError::DatatypeMismatch)
                                            app_data.get_all_page_hashes(c_id)
                                        }
                                    } else {
                                        Err(AppError::IndexingError)
                                    };
                                    if let Ok(mut hashes) = hash_res {
                                        if !hashes.is_empty() {
                                            let sync_type = 0;
                                            let hashes_len = hashes.len() as u16 - 1;
                                            if h_ids.is_empty() {
                                                // eprintln!("We send all hashes of CID {}", c_id);
                                                for i in 0..=hashes_len {
                                                    let hash_data = std::mem::replace(
                                                        &mut hashes[i as usize],
                                                        Data::empty(0),
                                                    );
                                                    let sync_response = SyncResponse::Hashes(
                                                        c_id, i, hashes_len, hash_data,
                                                    );
                                                    let res =
                                                        to_gnome_sender.send(ToGnome::SendData(
                                                            neighbor_id,
                                                            NeighborResponse::Custom(
                                                                sync_type,
                                                                CastData::new(
                                                                    sync_response.serialize(),
                                                                )
                                                                .unwrap(),
                                                            ),
                                                        ));
                                                    if res.is_ok() {
                                                        eprintln!(
                                                            "Hashes [{}/{}] of {} sent",
                                                            i, hashes_len, c_id
                                                        );
                                                    }
                                                }
                                            } else {
                                                for h_id in h_ids {
                                                    if h_id <= hashes_len {
                                                        let hash_data = std::mem::replace(
                                                            &mut hashes[h_id as usize],
                                                            Data::empty(0),
                                                        );
                                                        let sync_response = SyncResponse::Hashes(
                                                            c_id, h_id, hashes_len, hash_data,
                                                        );
                                                        let res = to_gnome_sender.send(
                                                            ToGnome::SendData(
                                                                neighbor_id,
                                                                NeighborResponse::Custom(
                                                                    sync_type,
                                                                    CastData::new(
                                                                        sync_response.serialize(),
                                                                    )
                                                                    .unwrap(),
                                                                ),
                                                            ),
                                                        );
                                                        if res.is_ok() {
                                                            eprintln!(
                                                                "Hashes [{}/{}] of {} sent",
                                                                h_id, hashes_len, c_id
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                SyncRequest::Pages(c_id, d_type, _p_ids) => {
                                    //TODO
                                    eprintln!(
                                        "We are requested to send some pages to neighbor ({:?}: {:?}).",c_id,
                                        _p_ids
                                    );
                                    if let Ok((data_type, len)) = app_data.get_type_and_len(c_id) {
                                        if data_type == d_type {
                                            if len > 0 {
                                                let total = len as u16 - 1;
                                                let sync_type = 0;
                                                for p_id in _p_ids {
                                                    if let Ok(data) = app_data.read_data(c_id, p_id)
                                                    {
                                                        let sync_response = SyncResponse::Page(
                                                            c_id, data_type, p_id, total, data,
                                                        );
                                                        let r = to_gnome_sender.send(
                                                            ToGnome::SendData(
                                                                neighbor_id,
                                                                NeighborResponse::Custom(
                                                                    sync_type,
                                                                    CastData::new(
                                                                        sync_response.serialize(),
                                                                    )
                                                                    .unwrap(),
                                                                ),
                                                            ),
                                                        );
                                                        if r.is_ok() {
                                                            eprintln!(
                                                                "Sent {}[{}/{}]",
                                                                c_id, p_id, total
                                                            );
                                                        }
                                                    } else {
                                                        eprintln!("No Data read.");
                                                    }
                                                }
                                            }
                                        } else if data_type == DataType::Link {
                                            eprintln!("Link len: {}", len);
                                            if len > 0 {
                                                let total = len as u16 - 1;
                                                let sync_type = 0;
                                                for p_id in _p_ids {
                                                    if let Ok((data, size)) = app_data
                                                        .read_link_ti_data(c_id, p_id, d_type)
                                                    {
                                                        let sync_response = SyncResponse::Page(
                                                            c_id, d_type, p_id, size, data,
                                                        );
                                                        let r = to_gnome_sender.send(
                                                            ToGnome::SendData(
                                                                neighbor_id,
                                                                NeighborResponse::Custom(
                                                                    sync_type,
                                                                    CastData::new(
                                                                        sync_response.serialize(),
                                                                    )
                                                                    .unwrap(),
                                                                ),
                                                            ),
                                                        );
                                                        if r.is_ok() {
                                                            eprintln!(
                                                                "Sent {}[{}/{}]",
                                                                c_id, p_id, total
                                                            );
                                                        }
                                                    } else {
                                                        eprintln!("No Data read.");
                                                    }
                                                }
                                            }
                                        } else {
                                            // Err(AppError::DatatypeMismatch)
                                        };
                                    }
                                }
                                SyncRequest::AllPages(c_ids) => {
                                    // TODO: here we should trigger a separate task for
                                    // each ContentID received in order to send
                                    // all Content's pages to specified Neighbor
                                    eprintln!(
                                        "We are requested to send all pages of Contents: {:?}.",
                                        c_ids
                                    );
                                    let sync_type = 0;
                                    for c_id in c_ids {
                                        eprintln!("Sending CID {}", c_id);
                                        //TODO: first we send all page hashes
                                        //      second we send pages
                                        if let Ok(data_vec) = app_data.get_all_data(c_id) {
                                            if data_vec.is_empty() {
                                                continue;
                                            }
                                            let (data_type, len) =
                                                app_data.get_type_and_len(c_id).unwrap();
                                            let total = len - 1;

                                            for (part_no, data) in data_vec.into_iter().enumerate()
                                            {
                                                let sync_response = SyncResponse::Page(
                                                    c_id,
                                                    data_type,
                                                    part_no as u16,
                                                    total,
                                                    data,
                                                );
                                                let _ = to_gnome_sender.send(ToGnome::SendData(
                                                    neighbor_id,
                                                    NeighborResponse::Custom(
                                                        sync_type,
                                                        CastData::new(sync_response.serialize())
                                                            .unwrap(),
                                                    ),
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    other => {
                        eprintln!("Request {}", other);
                    }
                }
            }
            ToAppData::CustomResponse(m_type, _neighbor_id, cast_data) => {
                match m_type {
                    0 => {
                        if let Ok(response) = SyncResponse::deserialize(cast_data.bytes()) {
                            //TODO:
                            // eprintln!("Deserialized response!: {:?}", response);
                            match response {
                                SyncResponse::Partial(is_hash_data, data) => {
                                    app_data.update_partial(is_hash_data, data);
                                }
                                SyncResponse::Datastore(part_no, total, mut hashes) => {
                                    eprintln!("Got SyncResponse::Datastore");
                                    let prev_dstore_sync =
                                        std::mem::replace(&mut datastore_sync, None);
                                    // eprintln!(
                                    //     "SID-{} PrevDSync: {:?}",
                                    //     swarm_id.0, prev_dstore_sync
                                    // );
                                    if let Some((next_awaited_part_no, mut missing_hashes)) =
                                        prev_dstore_sync
                                    {
                                        // eprintln!(
                                        //     "next_awaited_part_no: {}, part_no : {}, total: {}",
                                        //     next_awaited_part_no, part_no, total
                                        // );
                                        if next_awaited_part_no != part_no {
                                            missing_hashes.insert(part_no, hashes);
                                            eprintln!("datastore_sync is Some again");
                                            datastore_sync =
                                                Some((next_awaited_part_no, missing_hashes));
                                            continue;
                                        }
                                        //TODO: append all existing hashes
                                        //    we have cached until now to hashes
                                        //    and then decide value of datastore_sync
                                        let mut last_processed = part_no;
                                        for i in part_no + 1..=total {
                                            if let Some(mut append_hashes) =
                                                missing_hashes.remove(&i)
                                            {
                                                hashes.append(&mut append_hashes);
                                                last_processed = i;
                                            } else {
                                                break;
                                            }
                                        }
                                        if last_processed != total {
                                            datastore_sync =
                                                Some((last_processed + 1, missing_hashes));
                                            eprintln!(
                                                "Waiting for SyncResponse::Datastore [{}/{}]…",
                                                last_processed + 1,
                                                total,
                                            );
                                            continue;
                                        }

                                        // TODO: from now on, we have a complete snapshot
                                        // of Swarm's datastore, we need to compare it against
                                        // our own, and request missing data from neighbors
                                        // and load valid data from disk
                                        // let c_id = app_data.next_c_id().unwrap();
                                        let mut curr_cid = 0;

                                        for (data_type, hash) in hashes {
                                            if let Ok((dtype, dhash)) =
                                                app_data.content_root_hash(curr_cid)
                                            {
                                                if data_type == dtype {
                                                    if hash == dhash {
                                                        if let Some(content) =
                                                            load_content_from_disk(
                                                                s_storage.clone(),
                                                                curr_cid,
                                                                dtype,
                                                                dhash,
                                                            )
                                                            .await
                                                        {
                                                            let main_page = if let Ok(d) =
                                                                content.read_data(0)
                                                            {
                                                                d
                                                            } else {
                                                                Data::empty(0)
                                                            };
                                                            let _res =
                                                                app_data.update(curr_cid, content);
                                                            eprintln!(
                                                                "Load {:?} CID-{} from disk ok: {}",
                                                                swarm_id,
                                                                curr_cid,
                                                                _res.is_ok()
                                                            );
                                                            let _ = to_app_mgr_send.send(
                                                                ToAppMgr::ContentLoadedFromDisk(
                                                                    swarm_id, curr_cid, dtype,
                                                                    main_page,
                                                                ),
                                                            );
                                                        } else {
                                                            eprintln!(
                                                                "Load CID-{} from disk failed",
                                                                curr_cid,
                                                            );
                                                            request_content_from_any_neighbor(
                                                                curr_cid,
                                                                true,
                                                                true,
                                                                to_gnome_sender.clone(),
                                                            );
                                                            let _ = to_app_mgr_send.send(
                                                                ToAppMgr::ContentRequestedFromNeighbor(
                                                                    swarm_id, curr_cid, data_type,
                                                                ),
                                                            );
                                                        }
                                                    } else {
                                                        //TODO: hashes differ
                                                        let _ = app_data.update(
                                                            curr_cid,
                                                            Content::Data(
                                                                data_type,
                                                                ContentTree::Empty(hash),
                                                            ),
                                                        );
                                                        request_content_from_any_neighbor(
                                                            curr_cid,
                                                            true,
                                                            true,
                                                            to_gnome_sender.clone(),
                                                        );
                                                        let _ = to_app_mgr_send.send(
                                                            ToAppMgr::ContentRequestedFromNeighbor(
                                                                swarm_id, curr_cid, data_type,
                                                            ),
                                                        );
                                                        eprintln!("CID-{} Hash mismatch(disk: {} != {} swarm)", curr_cid,dhash,hash);
                                                    }
                                                } else {
                                                    //TODO: mismatching data types
                                                    // only valid scenario is when
                                                    // Link got transposed into Data
                                                    let _ = app_data.update(
                                                        curr_cid,
                                                        Content::Data(
                                                            data_type,
                                                            ContentTree::Empty(hash),
                                                        ),
                                                    );
                                                    request_content_from_any_neighbor(
                                                        curr_cid,
                                                        true,
                                                        true,
                                                        to_gnome_sender.clone(),
                                                    );
                                                    let _ = to_app_mgr_send.send(
                                                        ToAppMgr::ContentRequestedFromNeighbor(
                                                            swarm_id, curr_cid, data_type,
                                                        ),
                                                    );
                                                    eprintln!("CID-{} DType mismatch", curr_cid);
                                                }
                                            } else {
                                                let _ = app_data.append(Content::Data(
                                                    data_type,
                                                    ContentTree::Empty(hash),
                                                ));
                                                //TODO: we do not have this on disk
                                                request_content_from_any_neighbor(
                                                    curr_cid,
                                                    true,
                                                    true,
                                                    to_gnome_sender.clone(),
                                                );
                                                let _ = to_app_mgr_send.send(
                                                    ToAppMgr::ContentRequestedFromNeighbor(
                                                        swarm_id, curr_cid, data_type,
                                                    ),
                                                );
                                                eprintln!("CID-{} not found on disk", curr_cid);
                                            }
                                            curr_cid += 1;
                                        }
                                    } else {
                                        eprintln!("Datastore is synced, why sent this?");
                                    }
                                }
                                SyncResponse::Hashes(c_id, part_no, total, hashes) => {
                                    // eprintln!(
                                    //     "Received hashes [{}/{}] of {} (len: {})!",
                                    //     part_no,
                                    //     total,
                                    //     c_id,
                                    //     hashes.len()
                                    // );
                                    if let Ok((d_type, _len)) = app_data.get_type_and_len(c_id) {
                                        if matches!(d_type, DataType::Link) {
                                            let upd_res = app_data.update_transformative_link(
                                                true, c_id, part_no, total, hashes,
                                            );
                                            // eprintln!("Update res: {:?}", upd_res);
                                        } else {
                                            //TODO: not yet implemented
                                            if part_no == 0 && part_no == total {
                                                let mut data_vec =
                                                    Vec::with_capacity(hashes.len() >> 3);
                                                let bytes = hashes.bytes();
                                                for chunk in bytes.chunks_exact(8) {
                                                    let hash = u64::from_be_bytes(
                                                        chunk.try_into().unwrap(),
                                                    );
                                                    // eprintln!("Hash from Neighbor: {}", hash);
                                                    data_vec.push(Data::empty(hash));
                                                }

                                                let ct = ContentTree::from(data_vec);
                                                let c = Content::Data(d_type, ct);
                                                // eprintln!(
                                                //     "New bottom hashes: {:?}",
                                                //     c.data_hashes()
                                                // );
                                                let new_hash = c.hash();
                                                let (_type, old_hash) =
                                                    app_data.content_root_hash(c_id).unwrap();
                                                if old_hash == new_hash {
                                                    let _old_c = app_data.update(c_id, c).unwrap();
                                                    // eprintln!(
                                                    //     "Updated Content {} with hashes",
                                                    //     c_id
                                                    // );
                                                } else {
                                                    eprintln!("Can not update CID {} since old {} != {} new",c_id,old_hash,new_hash);
                                                }
                                            } else {
                                                todo!("dapp-lib/lib.rs:1981 implement me!");
                                            }
                                        }
                                    }
                                }
                                SyncResponse::Page(c_id, data_type, page_no, total, data) => {
                                    //TODO: make it proper
                                    if page_no == 0 {
                                        // eprintln!(
                                        //     "We've got main page of {}, data type: {:?}",
                                        //     c_id, data_type
                                        // );
                                        if let Ok((d_type, len)) = app_data.get_type_and_len(c_id) {
                                            // eprintln!("CID {} already exists", c_id);
                                            if d_type == DataType::Link {
                                                if len > 0 {
                                                    // eprintln!("Update existing Link");
                                                    let _ = app_data.update_transformative_link(
                                                        false, c_id, page_no, total, data,
                                                    );
                                                } else {
                                                    // eprintln!("Create new Link");
                                                    let res = app_data
                                                        .update(c_id, data_to_link(data).unwrap());
                                                    // eprintln!("Update result: {:?}", res);
                                                }
                                            } else {
                                                // eprintln!("Inserting page 0 of non-Link content");

                                                let res = app_data.update_data(
                                                    c_id,
                                                    page_no,
                                                    data.clone(),
                                                );
                                                if res.is_ok() {
                                                    // We send an update to print content
                                                    // on screen
                                                    let _ = to_app_mgr_send.send(
                                                        ToAppMgr::ContentChanged(
                                                            swarm_id,
                                                            c_id,
                                                            d_type,
                                                            Some(data),
                                                        ),
                                                    );
                                                    eprintln!(
                                                        "C-{} Page #{} update result: ok",
                                                        c_id, page_no,
                                                    );
                                                } else {
                                                    eprintln!(
                                                        "C-{} Page #{} update failed: {:?}",
                                                        c_id,
                                                        page_no,
                                                        res.err().unwrap()
                                                    );
                                                }
                                            }
                                        } else if data_type < DataType::Link {
                                            // eprintln!("Create new Data");
                                            let _ = to_app_mgr_send.send(ToAppMgr::ContentAdded(
                                                swarm_id,
                                                c_id,
                                                data_type,
                                                data.clone(),
                                            ));
                                            let _res = app_data.update(
                                                c_id,
                                                Content::Data(data_type, ContentTree::Filled(data)),
                                            );
                                        } else {
                                            // eprintln!("Create new Link 2");
                                            let _ = to_app_mgr_send.send(ToAppMgr::ContentAdded(
                                                swarm_id,
                                                c_id,
                                                data_type,
                                                data.clone(),
                                            ));
                                            let link_result = data_to_link(data);
                                            if let Ok(link) = link_result {
                                                let _res = app_data.update(c_id, link);
                                            // eprintln!("Update result: {:?}", res);
                                            } else {
                                                eprintln!(
                                                    "Failed to create a link: {:?}",
                                                    link_result.err().unwrap()
                                                );
                                            }
                                        }
                                    } else if let Ok((d_type, len)) =
                                        app_data.get_type_and_len(c_id)
                                    {
                                        if d_type == data_type {
                                            // let res = app_data.append_data(c_id, data);
                                            let res = app_data.update_data(c_id, page_no, data);
                                            if res.is_ok() {
                                                eprintln!(
                                                    "C-{} Page #{} update result: ok",
                                                    c_id, page_no,
                                                );
                                            } else {
                                                eprintln!(
                                                    "C-{} Page #{} update error: {:?}",
                                                    c_id,
                                                    page_no,
                                                    res.err().unwrap()
                                                );
                                            }
                                        } else if d_type == DataType::Link {
                                            let res = app_data.update_transformative_link(
                                                false, c_id, page_no, total, data,
                                            );
                                            // println!("Update TI result: {:?}", res);
                                        } else {
                                            eprintln!(
                                                "Error: Stored:{:?} len:{}\nreceived:{:?} len:{}",
                                                d_type, len, data_type, total
                                            );
                                        }
                                    } else {
                                        eprintln!("Datastore couldn't find ContentID {}", c_id);
                                    }
                                }
                            }
                        }
                    }
                    other => {
                        eprintln!("Unhandled response {}", other);
                    }
                }
            }
            ToAppData::ReadData(c_id) => {
                // eprintln!(
                //     "ToAppData::ReadData({}) when DStore synced: {}",
                //     c_id,
                //     datastore_sync.is_none()
                // );
                //TODO: find a way to decide if we are completely synced with swarm, or not
                //      if not request to sync from any Neighbor
                if datastore_sync.is_some() {
                    let _ = to_app_mgr_send.send(ToAppMgr::ReadError(
                        swarm_id,
                        c_id,
                        AppError::AppDataNotSynced,
                    ));
                    continue;
                }
                let type_and_len_result = app_data.get_type_and_len(c_id);
                if type_and_len_result.is_err() {
                    let error = type_and_len_result.err().unwrap();
                    let _ = to_app_mgr_send.send(ToAppMgr::ReadError(swarm_id, c_id, error));
                    continue;
                }
                let (_t, len) = type_and_len_result.unwrap();
                let (t, root_hash) = app_data.content_root_hash(c_id).unwrap();
                // eprintln!("Read {}, data len: {}", c_id, len);
                let all_data_result = app_data.get_all_data(c_id);
                if let Ok(data_vec) = all_data_result {
                    // let calculated_hash =
                    // if c_id == 0 {
                    //     eprintln!("Sending read result with {} data blocks", data_vec.len());
                    // }
                    let _ =
                        to_app_mgr_send.send(ToAppMgr::ReadSuccess(swarm_id, c_id, t, data_vec));
                } else {
                    let error = all_data_result.err().unwrap();
                    if matches!(error, AppError::ContentEmpty) {
                        let sync_requests: Vec<SyncRequest> =
                            vec![SyncRequest::AllPages(vec![c_id])];
                        let _ = to_gnome_sender.send(ToGnome::AskData(
                            GnomeId::any(),
                            NeighborRequest::Custom(
                                0,
                                CastData::new(serialize_requests(sync_requests)).unwrap(),
                            ),
                        ));
                    }
                    let _ = to_app_mgr_send.send(ToAppMgr::ReadError(swarm_id, c_id, error));
                }
            }
            ToAppData::BCastOrigin(c_id, send) => b_cast_origin = Some((c_id, send)),
            ToAppData::BCastData(_c_id, c_data) => {
                // TODO: serve this
                let a_msg_res = parse_cast(c_data);
                if let Ok(a_msg) = a_msg_res {
                    let upd_res = app_data.update_transformative_link(
                        a_msg.is_hash,
                        a_msg.content_id,
                        a_msg.part_no,
                        a_msg.total_parts,
                        a_msg.data,
                    );
                    // println!("update_transformative_link result: {:?}", upd_res);
                    if let Ok((d_type, missing_hashes, missing_data)) = upd_res {
                        //TODO: request hashes if missing not empty
                        if !missing_hashes.is_empty() {
                            //TODO: there can be multiple SyncRequests
                            eprintln!("Missing hashes: {:?}", missing_hashes);
                            let sync_request =
                                SyncRequest::Hashes(a_msg.content_id, missing_hashes);
                            to_gnome_sender
                                .send(ToGnome::AskData(
                                    GnomeId::any(),
                                    NeighborRequest::Custom(
                                        0,
                                        CastData::new(serialize_requests(vec![sync_request]))
                                            .unwrap(),
                                    ),
                                ))
                                .unwrap();
                        }
                        if !missing_data.is_empty() {
                            //TODO: there can be multiple SyncRequests
                            eprintln!("Missing data: {:?}", missing_data);
                            let sync_request =
                                SyncRequest::Pages(a_msg.content_id, d_type, missing_data);
                            to_gnome_sender
                                .send(ToGnome::AskData(
                                    GnomeId::any(),
                                    NeighborRequest::Custom(
                                        0,
                                        CastData::new(serialize_requests(vec![sync_request]))
                                            .unwrap(),
                                    ),
                                ))
                                .unwrap();
                        }
                    } else {
                        eprintln!("Unable to update: {:?}", upd_res);
                    }
                } else {
                    let data = a_msg_res.err().unwrap();
                    eprintln!("App Data: {} ", data);
                }
            }
            ToAppData::Terminate => {
                // TODO: determine whether or not we want to store this Swarm on disk
                if store_on_disk {
                    // TODO: pass parameters indicating what data to store
                    store_data_on_disk(s_storage, app_data).await;
                }

                eprintln!("Done serving AppData");
                break;
            }
            _ => {
                eprintln!("Unserved by app: {:?}", resp);
            }
        }
    }
}

async fn serve_swarm(
    sleep_time: Duration,
    user_res: Receiver<GnomeToApp>,
    to_app_data_send: ASender<ToAppData>,
) {
    loop {
        sleep(sleep_time).await;
        while let Ok(resp) = user_res.try_recv() {
            // println!("SUR: {:?}", resp);
            match resp {
                GnomeToApp::SwarmReady(s_name) => {
                    // eprintln!("Gnome says if synced: {}", synced);
                    let _ = to_app_data_send.send(ToAppData::SwarmReady(s_name)).await;
                }
                GnomeToApp::Broadcast(_s_id, c_id, recv_d) => {
                    spawn(serve_broadcast(
                        c_id,
                        Duration::from_millis(100),
                        recv_d,
                        to_app_data_send.clone(),
                    ));
                }
                GnomeToApp::Unicast(_s_id, c_id, recv_d) => {
                    spawn(serve_unicast(c_id, Duration::from_millis(100), recv_d));
                }
                GnomeToApp::BroadcastOrigin(_s_id, ref c_id, cast_data_send, cast_data_recv) => {
                    spawn(serve_broadcast(
                        *c_id,
                        Duration::from_millis(100),
                        cast_data_recv,
                        to_app_data_send.clone(),
                    ));
                    let _ = to_app_data_send
                        .send(ToAppData::BCastOrigin(*c_id, cast_data_send))
                        .await;
                }
                GnomeToApp::UnicastOrigin(_s_id, c_id, send_d) => {
                    spawn(serve_unicast_origin(
                        c_id,
                        Duration::from_millis(500),
                        send_d,
                    ));
                }
                GnomeToApp::BCastData(c_id, _data) => {
                    // TODO: convert it to local BCastMessage
                    // and apply to app_data
                    // println!("Got data from {}", c_id.0);
                }
                GnomeToApp::Custom(is_request, m_type, gnome_id, data) => {
                    if is_request {
                        let _ = to_app_data_send
                            .send(ToAppData::CustomRequest(m_type, gnome_id, data))
                            .await;
                    } else {
                        let _ = to_app_data_send
                            .send(ToAppData::CustomResponse(m_type, gnome_id, data))
                            .await;
                    }
                    // println!("Got data from {}", gnome_id);
                }
                GnomeToApp::Reconfig(id, sync_data) => match id {
                    0 => {
                        let _ = to_app_data_send
                            .send(ToAppData::TransformLink(sync_data))
                            .await;
                    }
                    other => {
                        eprintln!("Unserved Reconfig from {}: {:?}", id, other);
                    }
                },
                _ => {
                    // println!("UNserved swarm data: {:?}", _res);
                    let _ = to_app_data_send.send(ToAppData::Response(resp)).await;
                }
            }
        }
    }
}

fn request_content_from_any_neighbor(
    cid: u16,
    hashes: bool,
    // first_page: bool,
    all_pages: bool,
    to_gnome_sender: Sender<ToGnome>,
) {
    eprintln!(
        "Sending a request for CID-{} (hashes: {}, all_pages: {})",
        cid, hashes, all_pages
    );
    let mut sync_requests: Vec<SyncRequest> = vec![];
    if hashes {
        sync_requests.push(SyncRequest::Hashes(cid, vec![]));
    }
    // if first_page {
    //     sync_requests.push(SyncRequest::AllFirstPages(Some(vec![cid])));
    // }
    if all_pages {
        sync_requests.push(SyncRequest::AllPages(vec![cid]));
    }
    let _ = to_gnome_sender.send(ToGnome::AskData(
        GnomeId::any(),
        NeighborRequest::Custom(0, CastData::new(serialize_requests(sync_requests)).unwrap()),
    ));
}
async fn serve_unicast(c_id: CastID, sleep_time: Duration, user_res: Receiver<CastData>) {
    eprintln!("Serving unicast {:?}", c_id);
    loop {
        let recv_res = user_res.try_recv();
        if let Ok(data) = recv_res {
            eprintln!("U{:?}: {}", c_id, data);
        }
        sleep(sleep_time).await;
    }
}
async fn serve_broadcast_origin(
    c_id: CastID,
    sleep_time: Duration,
    user_res: Sender<CastData>,
    data_vec: Vec<CastData>,
    done: Sender<()>,
) {
    eprintln!("Originating broadcast {:?}", c_id);
    sleep(Duration::from_secs(2)).await;
    eprintln!("Initial sleep is over");
    // TODO: indexing
    for (i, data) in data_vec.into_iter().enumerate() {
        let send_res = user_res.send(data);
        if send_res.is_ok() {
            print!("BCed: {}\t", i + 1);
        } else {
            eprintln!(
                "Error while trying to broadcast: {:?}",
                send_res.err().unwrap()
            );
            break;
        }
        // println!("About to go to sleep for: {:?}", sleep_time);
        sleep(sleep_time).await;
    }
    let _ = done.send(());
}

async fn serve_broadcast(
    c_id: CastID,
    sleep_time: Duration,
    cast_data_recv: Receiver<CastData>,
    to_app_data_send: ASender<ToAppData>,
) {
    eprintln!("Serving broadcast {:?}", c_id);
    loop {
        let recv_res = cast_data_recv.try_recv();
        if let Ok(data) = recv_res {
            // println!("B{:?}: {}", c_id, data);
            let _ = to_app_data_send
                .send(ToAppData::BCastData(c_id, data))
                .await;
        }
        sleep(sleep_time).await;
    }
}

async fn serve_unicast_origin(c_id: CastID, sleep_time: Duration, user_res: Sender<CastData>) {
    eprintln!("Originating unicast {:?}", c_id);
    let mut i: u8 = 0;
    loop {
        let send_res = user_res.send(CastData::new(vec![i]).unwrap());
        if send_res.is_ok() {
            eprintln!("Unicasted {}", i);
        } else {
            eprintln!(
                "Error while trying to unicast: {:?}",
                send_res.err().unwrap()
            );
        }
        i = i.wrapping_add(1);

        sleep(sleep_time).await;
    }
}

#[derive(Debug)]
struct AppMessage {
    pub is_hash: bool,
    pub content_id: ContentID,
    // pub d_type: DataType,
    pub part_no: u16,
    pub total_parts: u16,
    pub data: Data,
}

impl AppMessage {
    pub fn new(
        content_id: ContentID,
        is_hash: bool,
        part_no: u16,
        total_parts: u16,
        data: Data,
    ) -> Self {
        AppMessage {
            is_hash,
            content_id,
            part_no,
            total_parts,
            data,
        }
    }

    pub fn to_sync(self) -> SyncData {
        SyncData::new(self.bytes(0)).unwrap()
    }

    pub fn to_cast(self) -> CastData {
        CastData::new(self.bytes(0)).unwrap()
    }

    fn bytes(self, header_byte: u8) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(7 + self.data.len());
        bytes.push(header_byte);
        if self.is_hash {
            bytes.push(255);
        } else {
            bytes.push(0);
        }
        for byte in self.content_id.to_be_bytes() {
            bytes.push(byte);
        }
        for byte in self.part_no.to_be_bytes() {
            bytes.push(byte);
        }
        for byte in self.total_parts.to_be_bytes() {
            bytes.push(byte);
        }
        bytes.append(&mut self.data.bytes());
        bytes
    }
}

fn parse_cast(cast_data: CastData) -> Result<AppMessage, Data> {
    // println!("Parse cast: {:?}", cast_data);
    let mut bytes_iter = cast_data.bytes().into_iter();
    match bytes_iter.next().unwrap() {
        0 => {
            let is_hash = match bytes_iter.next() {
                Some(0) => false,
                Some(255) => true,
                _other => return Err(Data::empty(0)),
            };
            let b1 = bytes_iter.next().unwrap();
            let b2 = bytes_iter.next().unwrap();
            let content_id: ContentID = u16::from_be_bytes([b1, b2]);
            // println!("Decoded content ID: {}", content_id);
            let b1 = bytes_iter.next().unwrap();
            let b2 = bytes_iter.next().unwrap();
            let part_no: u16 = u16::from_be_bytes([b1, b2]);
            let b1 = bytes_iter.next().unwrap();
            let b2 = bytes_iter.next().unwrap();
            let total_parts: u16 = u16::from_be_bytes([b1, b2]);
            let data: Data = Data::new(bytes_iter.collect()).unwrap();
            Ok(AppMessage::new(
                content_id,
                is_hash,
                part_no,
                total_parts,
                data,
            ))
        }
        1 => Err(Data::new(bytes_iter.collect()).unwrap()),
        _ => {
            panic!("TODO parse cast");
        }
    }
}
pub struct ApplicationData {
    change_reg: ChangeRegistry,
    contents: Datastore,
    hash_to_temp_idx: HashMap<u64, u16>,
    partial_data: HashMap<u16, (Vec<u64>, HashMap<u64, Data>)>,
}

impl ApplicationData {
    // pub fn empty() -> Self {
    //     ApplicationData {
    //         change_reg: ChangeRegistry::new(),
    //         contents: Datastore::Empty,
    //         hash_to_temp_idx: HashMap::new(),
    //         partial_data: HashMap::new(),
    //     }
    // }
    // pub fn new(manifest: Manifest) -> Self {
    pub fn new(app_type: AppType) -> Self {
        ApplicationData {
            change_reg: ChangeRegistry::new(),
            contents: Datastore::new(app_type),
            hash_to_temp_idx: HashMap::new(),
            partial_data: HashMap::new(),
        }
    }

    pub fn next_c_id(&self) -> Option<ContentID> {
        let next_id = self.contents.len();
        if next_id < u16::MAX {
            Some(next_id)
        } else {
            None
        }
    }
    pub fn update_partial(&mut self, is_hash_data: bool, data: Data) {
        if is_hash_data {
            let mut next_idx = 0;
            for i in 0..=u16::MAX {
                if !self.partial_data.contains_key(&i) {
                    next_idx = i;
                    break;
                }
            }
            let total_parts = data.len() >> 3;
            let mut all_hashes = Vec::with_capacity(total_parts);
            let mut bytes = data.clone().bytes();
            all_hashes.push(0);
            for _i in 0..total_parts {
                let mut hash: [u8; 8] = [0; 8];
                for item in &mut hash {
                    *item = bytes.drain(0..1).next().unwrap();
                }
                let hash = u64::from_be_bytes(hash);
                // eprintln!("Expecting hash: {}", hash);
                all_hashes.push(hash);
                self.hash_to_temp_idx.insert(hash, next_idx);
            }
            let mut new_hm = HashMap::new();
            // eprintln!("Inserting data len: {}", data.len());
            new_hm.insert(0, data);
            self.partial_data.insert(next_idx, (all_hashes, new_hm));
        } else {
            let hash = data.get_hash();
            // eprintln!("Got hash: {}", hash);
            if let Some(temp_idx) = self.hash_to_temp_idx.get(&hash) {
                // println!("Oh yeah");
                if let Some((vec, mut hm)) = self.partial_data.remove(temp_idx) {
                    // eprintln!("Inserting data len: {}", data.len());
                    hm.insert(hash, data);
                    // println!("{} ==? {}", vec.len(), hm.len());
                    if vec.len() == hm.len() {
                        // eprintln!("two");
                        // Some(SyncMessage::from_data(vec, hm).unwrap())
                    } else {
                        self.partial_data.insert(*temp_idx, (vec, hm));
                        // None
                    }
                } else {
                    // None
                }
            } else {
                // None
            }
        }
    }
    // TODO: this needs rework as well as SyncMessage::from_data
    pub fn process(&mut self, data: SyncData) -> Option<SyncMessage> {
        let d_len = data.len();
        let mut bytes = data.bytes();
        let m_type = SyncMessageType::new(&mut bytes);
        let mut drained_bytes = m_type.as_bytes();
        let part_no = bytes.drain(0..1).next().unwrap();
        let total_parts = bytes.drain(0..1).next().unwrap();
        drained_bytes.push(part_no);
        drained_bytes.push(total_parts);
        // eprintln!(
        //     "Process SyncData {:?} [{}/{}] {} bytes",
        //     m_type, part_no, total_parts, d_len,
        // );
        if part_no == 0 {
            if total_parts == 0 {
                let mut hm = HashMap::new();
                drained_bytes.append(&mut bytes);
                hm.insert(0, Data::new(drained_bytes).unwrap());
                // eprintln!("one");
                Some(SyncMessage::from_data(vec![0], hm).unwrap())
            } else {
                let mut next_idx = 0;
                for i in 0..=u16::MAX {
                    if !self.partial_data.contains_key(&i) {
                        next_idx = i;
                        break;
                    }
                }
                let mut all_hashes = Vec::with_capacity((total_parts as usize) + 1);
                all_hashes.push(0);
                for _i in 0..total_parts {
                    let mut hash: [u8; 8] = [0; 8];
                    for item in &mut hash {
                        *item = bytes.drain(0..1).next().unwrap();
                        drained_bytes.push(*item);
                    }
                    let hash = u64::from_be_bytes(hash);
                    eprintln!("Expecting hash: {}", hash);
                    all_hashes.push(hash);
                    self.hash_to_temp_idx.insert(hash, next_idx);
                }
                let mut new_hm = HashMap::new();
                drained_bytes.append(&mut bytes);
                let data = Data::new(drained_bytes).unwrap();
                // eprintln!("Inserting data len: {}", data.len());
                new_hm.insert(0, data);
                self.partial_data.insert(next_idx, (all_hashes, new_hm));
                None
            }
        } else {
            // Second byte is non zero, so we received a non-head partial Data
            drained_bytes.append(&mut bytes);
            let data = Data::new(drained_bytes).unwrap();
            let hash = data.get_hash();
            // eprintln!("Got hash: {}", hash);
            if let Some(temp_idx) = self.hash_to_temp_idx.get(&hash) {
                // println!("Oh yeah");
                if let Some((vec, mut hm)) = self.partial_data.remove(temp_idx) {
                    // eprintln!("Inserting data len: {}", data.len());
                    hm.insert(hash, data);
                    // println!("{} ==? {}", vec.len(), hm.len());
                    if vec.len() == hm.len() {
                        // eprintln!("two");
                        Some(SyncMessage::from_data(vec, hm).unwrap())
                    } else {
                        self.partial_data.insert(*temp_idx, (vec, hm));
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }
    }

    pub fn root_hash(&self) -> u64 {
        self.contents.hash()
    }

    pub fn shell(&self, c_id: ContentID) -> Result<Content, AppError> {
        self.contents.shell(c_id)
    }
    pub fn clone_content(&self, c_id: ContentID) -> Result<Content, AppError> {
        self.contents.clone_content(c_id)
    }
    pub fn content_root_hash(&self, c_id: ContentID) -> Result<(DataType, u64), AppError> {
        let result = self.contents.get_root_content_typed_hash(c_id);
        if let Ok(value) = result {
            Ok(value)
        } else {
            Err(result.err().unwrap())
        }
    }
    pub fn registry(&self) -> Vec<ContentID> {
        self.change_reg.read()
    }

    pub fn all_content_typed_root_hashes(&self) -> Vec<Vec<(DataType, u64)>> {
        self.contents.all_typed_root_hashes()
    }
    pub fn transform_link(&mut self, content_id: ContentID) -> Result<Content, AppError> {
        //TODO
        let ti = self.contents.take_transform_info(content_id)?;
        let d_type = ti.d_type;
        let c_tree = ti.into_tree();
        let new_content = Content::Data(d_type, c_tree);
        self.contents.update(content_id, new_content)
    }
    pub fn update_transformative_link(
        &mut self,
        is_hash: bool,
        content_id: ContentID,
        part_no: u16,
        total_parts: u16,
        data: Data,
    ) -> Result<(DataType, Vec<u16>, Vec<u16>), AppError> {
        // println!("Call update_transformative_link");
        self.contents
            .update_transformative_link(is_hash, content_id, part_no, total_parts, data)
    }
    pub fn append(&mut self, content: Content) -> Result<u64, AppError> {
        let index_to_add = self.contents.len();
        if index_to_add == u16::MAX {
            return Err(AppError::DatastoreFull);
        }
        self.contents.append(content)
    }
    pub fn get_type_and_len(&self, c_id: ContentID) -> Result<(DataType, u16), AppError> {
        self.contents.type_and_len(c_id)
    }
    pub fn read_data(&self, c_id: ContentID, data_id: u16) -> Result<Data, AppError> {
        self.contents.read_data((c_id, data_id))
    }
    pub fn read_link_ti_data(
        &self,
        c_id: ContentID,
        data_id: u16,
        d_type: DataType,
    ) -> Result<(Data, u16), AppError> {
        self.contents.read_link_data((c_id, data_id), d_type)
    }
    pub fn insert_data(&mut self, c_id: ContentID, d_id: u16, data: Data) -> Result<u64, AppError> {
        self.contents.insert_data(c_id, d_id, data)
    }
    pub fn append_data(&mut self, c_id: ContentID, data: Data) -> Result<u64, AppError> {
        self.contents.append_data(c_id, data)
    }
    pub fn pop_data(&mut self, c_id: ContentID) -> Result<Data, AppError> {
        self.contents.pop_data(c_id)
    }
    pub fn update_data(
        &mut self,
        c_id: ContentID,
        d_id: u16,
        data: Data,
    ) -> Result<Data, AppError> {
        self.contents.update_data((c_id, d_id), data)
    }
    pub fn remove_data(&mut self, c_id: ContentID, d_id: u16) -> Result<Data, AppError> {
        self.contents.remove_data(c_id, d_id)
    }
    pub fn update(&mut self, c_id: ContentID, content: Content) -> Result<Content, AppError> {
        self.contents.update(c_id, content)
    }
    pub fn get_all_data(&self, c_id: ContentID) -> Result<Vec<Data>, AppError> {
        let read_result = self.contents.read_data((c_id, 0));
        let mut data_vec = vec![];
        if let Ok(data) = read_result {
            data_vec.push(data);
        } else {
            return Err(read_result.err().unwrap());
        }
        for i in 1..u16::MAX {
            let read_result = self.contents.read_data((c_id, i));
            if let Ok(data) = read_result {
                data_vec.push(data);
            } else {
                break;
            }
        }
        // eprintln!("CID-{} has {} Data blocks", c_id, data_vec.len());
        Ok(data_vec)
    }
    pub fn content_bottom_hashes(&self, c_id: ContentID) -> Result<Vec<u64>, AppError> {
        self.contents.content_bottom_hashes(c_id)
    }
    pub fn get_all_page_hashes(&self, c_id: ContentID) -> Result<Vec<Data>, AppError> {
        let hashes_vec = self.contents.content_bottom_hashes(c_id)?;
        // eprintln!("Got hashes vec");
        let mut results = vec![];
        let mut hash_iter = hashes_vec.into_iter();
        let mut i = 0;
        let mut bytes = Vec::with_capacity(1024);
        while let Some(hash) = hash_iter.next() {
            if i == 128 {
                i = 0;
                let ready_bytes = std::mem::replace(&mut bytes, Vec::with_capacity(1024));
                results.push(Data::new(ready_bytes).unwrap());
            }
            for byte in hash.to_be_bytes() {
                bytes.push(byte);
            }
            i += 1;
        }
        results.push(Data::new(bytes).unwrap());
        Ok(results)
    }

    pub fn get_all_transform_info_hashes(
        &self,
        c_id: ContentID,
        // d_type: DataType,
    ) -> Result<Vec<Data>, AppError> {
        self.contents.link_transform_info_hashes(c_id)
        // let hashes_vec = self.contents.link_transform_info_hashes(c_id,d_type)?;
        // println!("Got hashes vec");
        // let mut results = vec![];
        // let mut hash_iter = hashes_vec.into_iter();
        // let mut i = 0;
        // let mut bytes = Vec::with_capacity(1024);
        // while let Some(hash) = hash_iter.next() {
        //     if i == 128 {
        //         i = 0;
        //         let ready_bytes = std::mem::replace(&mut bytes, Vec::with_capacity(1024));
        //         results.push(Data::new(ready_bytes).unwrap());
        //     }
        //     for byte in hash.to_be_bytes() {
        //         bytes.push(byte);
        //     }
        //     i += 1;
        // }
        // results.push(Data::new(bytes).unwrap());
        // Ok(results)
    }

    pub fn get_partial_hashes(&self) -> Vec<Data> {
        let mut results = Vec::with_capacity(self.partial_data.len());
        for (_vec, pdata) in self.partial_data.values() {
            if let Some(hdata) = pdata.get(&0) {
                results.push(hdata.clone());
            }
        }
        results
    }
    pub fn get_partial_data(&self) -> Vec<Data> {
        let mut results = Vec::with_capacity(self.partial_data.len());
        for (_vec, pdata) in self.partial_data.values() {
            for (h, data) in pdata {
                if *h == 0 {
                    continue;
                } else {
                    results.push(data.clone());
                }
            }
        }
        results
    }

    // TODO: design application level interface
}
fn get_root_hash(hashes: &Vec<u64>) -> u64 {
    let h_len = hashes.len();
    let mut sub_hashes = Vec::with_capacity((h_len >> 1 as usize) + 1);
    for i in 0..h_len >> 1 {
        sub_hashes.push(double_hash(hashes[2 * i], hashes[2 * i + 1]));
    }
    if h_len % 2 == 1 {
        sub_hashes.push(hashes[h_len - 1]);
    }
    if sub_hashes.len() == 1 {
        return sub_hashes[0];
    } else {
        get_root_hash(&sub_hashes)
    }
} // An entire application data consists of a structure called Datastore.
  // There is also a helper change_reg useful for syncing.

// Datastore is a binary tree whose leafs store Content.
// It is constructed in such a way that when we traverse it from top
// to bottom we also fill a 16-bit registry, we put 0 when we go left, and 1 when
// we go right. Then we shift the register by one bit to the left.
// This 16-bit registry is ContentID. When we have a small amount of CIDs in our
// Datastore, CIDs are small and Datastore tree is also small - it consists of
// just a few layers, or floors.

// Content within Datastore can be identified with ContentID.
// First ContentID=0 should always define the ApplicationManifest
// in a form of HashMap<u8,Vec<u8: max 63 bytes>>.

// Once given ContentID is assigned with some specific Content constructor,
// other than a Link, no other type of constructor can be assigned to given CID.
// After given ContentID is assigned there is no going back, we can not remove it
// from Datastore, but we can update it within given Datatype.
// This is to prevent any existing links to given content from becoming invalid.
// Only if given ContentID is a Link it can be set to any other type of data.
// A Link can not point to other link. It can only point to
// a non-empty and non-link ContentID.
// If we try to create a Link to a Link, then it is simply cloned.
// You could for example clone applications default interface phrases ("File", "Open"...)
// and change them within your own Data app. Then you go back to originating application
// and set your preferred phrases to those you just created in your Data app.
// This way users can customize apps in any way they wish.

// Datastore is a binary tree storing in it's nodes references to content trees.
// This is in order to enable fast synchronization of data between gnomes.
// First you only exchange (CID,Hash) pairs with neighbors and when you see
// they are different, you sync them to match with the swarm's.

// A receiving gnome will see that list of (CID, hash) pairs and compare them one by one
// with his hashes. If they are not the same, he will add given CID to unsynced_cids
// list. Once he finds the first CID that matches his own he will know that remaining
// CIDs are also the same, since they were ordered by their update time.
// He can skip the verification of items following CID with hash equal to what he has.
// If all of received hashes are different two things can happen:
// - we request a subsequent list of most recently updated CIDs if such exists,
// - or we request our neighbor to send all the hashes of CID he has in his storage.
// The second option might consist of up to 640 messages exchanged between gnomes
// (2 byte CID + 8 byte hash = 10 byte per Content
// 10 bytes * 65536 = 655360 bytes
// and we assume data limit in a single DGram to be 1024 bytes).
// The amount of DGrams sent can be reduced by sending Datastore hashes in order
// from top to bottom, layer by layer, left to right. A single 1024 byte
// Data will contain 128 hashes, enough to transmit all Datastore's layer 7 hashes.
// But four DGrams with 4 Data containing 512 hashes will cover entire layer 9.

// This procedure needs to be dynamic.
// We first have to determine size of Datastore. Now we can calculate how many
// layers it consists of (when we view it as a binary tree, layer one has one, root
// element, layer two has two elems, layer three has four and so on).
// If we have no more than 128 CIDs one message will contain every Content root hash.
// Between 129-256 - two messages will do,
// 257-512 - 4 messages,
// 513-1024 - 8 msgs,
// ...2048 - 16,
// ...4096 - 32,
// ...8192 - 64,
// ...16k  - 128,
// ...32k  - 256,
// ...64k  - 512.

// Simplest procedure will gather all Content root hashes available (with possible
// retransmissions) in ordered fashion, and then install them as Phantom content into
// Datastore.

// For up to 512 CID simplest method is enough.
// Later a more sophisticated method can be implemented.

// If an application data is ordered by how frequent it is changed, we might
// only need to sync one part of Datastore's tree that has changed.

// An example of more complicated procedure:
// If we have more than 512 CIDs defined, we first send 4 DGrams with layer-9 hashes
// and construct a Datastore consisting of those 9 layers only.
// Then we compare each of these hashes with our own, if they are different
// we save that layer-9 masked CID. We collect all those masked CIDs and send
// them in a single request message to a neighbor.
// For each l9mCID neighbor responds with a single message containing sub-datastore
// for that l9mCID branch. In our root Datastore we substitute given l9mCID
// Phantom data with sub-datastore we just received. And so on for every other we
// requested.
// Up to 517 messages will get sent, excluding retransmissions, for Datastore pre-sync.

// This will minimize the amount of bytes required to be sent over network for
// synchronization.

// Using above procedure we have determined which CIDs need to be synced.

// Now we need to synchronize each content whose CID is marked for syncing.
// We have three options at hand:
// 1. We only store a root hash for given CID, since we don't need actual contents.
//    So by this moment we are done with this CID, since previous procedure has
//    fulfilled this requirement.
// 2. We ask our Neighbor to send us only datahashes of given CID and not actual Data
//    Depending on size of given Content it may be up ta about 65536 * 8 bottom
//    hashes, so this will require sending at most 512 DGrams per Content.
//    Now we can compare received Data hashes with our own and request only those
//    that do not match. Some Data might have been shifted, so we need to consider this.
// 3. We ask our Neighbor(s) for all Data chunks for given CID
//    Here it can take up to 65536 DGrams per Content.
// Depending on networking and storage conditions we may only synchronize most important
// CIDs, keeping less important CIDs either in Phantom mode or entirely bodyless,
// or something in between.

// If we decide not to download any Content only it's root hashes this will take up to
// 65536 * 8 bytes, half a Mibibyte.

// If we decide to store all the hashes and none of the Data then it will take up to
// 65536 CIDs * 65536 data hashes * 8 bytes per hash = 32Gibibytes of data hashes,
// and twice that much when we also count non-data hashes.

// If we decide to store all Data of all Contents then it would take up to
// 65536 CID * 65536 Data slots * 1024 bytes = 4 Tibibytes.

// So it is clear that an average gnome will only store content root hashes,
// downloading only Data chunks when necessary to provide application's functionality.
// This requires that all messages that Gnomes are sending have to include
// final root hashes of all Contents that given message will influence.
// Also all the starting root hashes of Contents that are required for
// given message to be processed by the application.
// This way all the gnomes will always stay synced with each other.
// Application logic will take accepted message and verify all the starting
// root hashes declared in given message, if they are not matching, message is
// discarded, and this event may be logged, if application supports logging.
// If a message uses some Content, but it was not declared in starting root hashes,
// it is also discarded.
// After above requirements are met we can evaluate given message.
// When evaluating application produces (CID, final hash) pairs of all the Contents
// that will be affected by given message. Only if pairs provided by evaluator
// are matching those contained in message by both their numbers and values given
// message is applied to Datastore.
// Now all provided CIDs root hashes are updated.
// All messages are signed and authorized against defined Requirements, so it is
// not possible for a random Gnome to mess up applications Datastore.

// It is very easy to detect when someone with Capabilities is trying to
// misbehave by any gnome that contains all required Contents and has the
// Proof of wrongdoing.

// There are two validation layers:
// - first one only verifies that starting root-hashes of CID match those in
//   Datastore, and application logic does not require any additional CIDs to work,
// - second one can only be used by Gnomes containing all necessary Content Data
//   and in addition to first validation runs application layer validation.

// The Proof consists of a Data (or multiples of Data) that contains information,
// that can be used by application's validate function to show that given message
// should be discarded. Now every gnome can validate that given data is indeed in
// Datastore, and can run the required validation on it's own.
// Once given message is proven to be invalid it gets reverted and Gnome is being
// Suspended.

// When syncing, all content should be stored in byte chunks of up to 1024 bytes each.
// Those chunks should be stored in a BHTree containing as many Leafs as needed to store
// them. All non-leaf nodes in BHTree should contain a hash that is built from
// hashes of both of his left and right children.
// This way we can easily detect which chunks of data are to be synced and request
// them from neighbors. No need to exchange entire data structures when only one byte
// has changed.
// But our neighbor needs to dedicate his resources for data structure
// conversion into BHT and data transfer over network.

// In some cases BHT may be the default representation of a data structure, then
// no conversion will be needed.
// When BHT is the default representation, we can define smart update procedures
// in order to shift bytes in Leafs left and right
// for inserting/replacing/deleting parts of that structure.
// BHT requires only around 2% overhead for storing hashes while allowing for fast
// synchronization.

// When downloading an external Link we send a resource request to Manager.
// His job is to localize given resource and send it back.
// This resource should be sent in serialized form of 1024 byte chunks,
// since other applications might have different type definitions than ours
// while still storing the same data.
// Now we can store this data locally in order to prevent from repeating same requests.

// We can also open a Link, but only if our environment allows for it to be open.
// This will cause our environment to switch from current application to that hidden
// under the link and selecting linked data.

// In case of an internal Link we have the data at hand, no request needed.

// One powerful concept to implement is to allow for changing ApplicationManifest
// while operating an app.
// This can serve many purposes like for example in a Voting app where you have
// multiple phases: you first need to have a Discussion over given problem,
// then you come up with one or multiple solutions, later you can submit solutions
// and after that enable voting for some specified timeframe, and so on and on.
// During each State an app shape-shifts between different modes of operation, while the
// data structures stay the same. Some structures may be read-only in one phase,
// and editable in another.

// yeah, tests... ... ...
#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    fn it_works() {
        // let result = add(2, 2);
        // assert_eq!(result, 4);
    }
}
