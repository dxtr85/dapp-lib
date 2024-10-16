use crate::content::data_to_link;
use crate::prelude::SyncRequirements;
use crate::prelude::TransformInfo;
use crate::sync_message::serialize_requests;
use std::time::Duration;
mod config;
mod content;
mod data;
mod datastore;
mod error;
mod manager;
mod manifest;
mod message;
mod registry;
mod sync_message;
use std::collections::HashMap;
use std::collections::HashSet;
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
use manifest::ApplicationManifest;
use message::{SyncMessage, SyncMessageType};
use registry::ChangeRegistry;
// TODO: probably better to use async channels for this lib where possible
use std::sync::mpsc::channel;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;

use async_std::channel as achannel;
use async_std::channel::Receiver as AReceiver;
use async_std::channel::Sender as ASender;

pub mod prelude {
    pub use crate::content::{
        double_hash, Content, ContentID, ContentTree, DataType, TransformInfo,
    };
    pub use crate::data::Data;
    pub use crate::error::AppError;
    pub use crate::initialize;
    pub use crate::manifest::ApplicationManifest;
    pub use crate::message::SyncMessage;
    pub use crate::message::SyncMessageType;
    pub use crate::message::SyncRequirements;
    pub use crate::ApplicationData;
    pub use crate::ApplicationManager;
    pub use crate::Configuration;
    pub use crate::ToApp;
    pub use gnome::prelude::CastData;
    pub use gnome::prelude::GnomeId;
    pub use gnome::prelude::SwarmID;
    pub use gnome::prelude::SyncData;
    pub use gnome::prelude::ToGnome;
}
fn manifest() -> ApplicationManifest {
    let mut header: [u8; 495] = [0; 495];
    for (i, byte) in "Catalog".bytes().enumerate() {
        header[i] = byte;
    }

    ApplicationManifest::new(header, HashMap::new())
}

pub enum ToApp {
    ActiveSwarm(GnomeId, SwarmID),
    Neighbors(SwarmID, Vec<GnomeId>),
    NewContent(SwarmID, ContentID, DataType),
    ReadResult(SwarmID, ContentID, Vec<Data>),
    Disconnected,
}

pub enum ToAppMgr {
    ReadData(SwarmID, ContentID),
    ReadResult(SwarmID, ContentID, Vec<Data>),
    UploadData,
    SetActiveApp(GnomeId),
    StartUnicast,
    StartBroadcast,
    EndBroadcast,
    UnsubscribeBroadcast,
    SendManifest,
    ListNeighbors,
    NeighborsListing(SwarmID, Vec<GnomeId>),
    ChangeContent(DataType, Data),
    AddContent(SwarmID, Data),
    ContentAdded(SwarmID, ContentID, DataType),
    TransformLinkRequest(Box<SyncData>),
    Quit,
}
// enum ToApp {}

#[derive(Debug)]
pub enum ToAppData {
    Response(GnomeToApp),
    ReadData(ContentID),
    UploadData,
    StartUnicast,
    StartBroadcast,
    EndBroadcast,
    UnsubscribeBroadcast,
    SendManifest,
    ListNeighbors,
    AppDataSynced(bool),
    BCastData(CastID, CastData),
    BCastOrigin(CastID, Sender<CastData>),
    ChangeContent(DataType, Data),
    AddContent(Data),
    CustomRequest(u8, GnomeId, CastData),
    CustomResponse(u8, GnomeId, CastData),
    TransformLinkRequest(SyncData),
    TransformLink(SyncData),
    Terminate,
}
// enum ToSwarm {}

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
    config: Configuration,
) -> GnomeId {
    let (gmgr_send, gmgr_recv, my_id) = init(config.work_dir.clone(), config.app_data_root_hash);
    spawn(serve_gnome_manager(
        gmgr_send,
        gmgr_recv,
        to_user_send,
        to_app_mgr_send,
        to_app_mgr_recv,
    ));
    my_id
}

async fn serve_gnome_manager(
    to_gnome_mgr: Sender<ToGnomeManager>,
    from_gnome_mgr: Receiver<FromGnomeManager>,
    to_user: Sender<ToApp>,
    to_app_mgr: Sender<ToAppMgr>,
    to_app_mgr_recv: Receiver<ToAppMgr>,
) {
    // TODO: AppMgr should hold state
    let message = from_gnome_mgr
        .recv()
        .expect("First message sent from gnome mgr has to be MyID");
    let mut my_id = if let FromGnomeManager::MyID(gnome_id) = message {
        gnome_id
    } else {
        GnomeId(u64::MAX)
    };
    eprintln!("My-ID: {}", my_id);
    let mut app_mgr = ApplicationManager::new(my_id);

    let sleep_time = Duration::from_millis(128);
    let mut own_swarm_started = false;
    'outer: loop {
        sleep(sleep_time).await;
        while let Ok(message) = from_gnome_mgr.try_recv() {
            match message {
                FromGnomeManager::MyID(m_id) => my_id = m_id,
                FromGnomeManager::SwarmFounderDetermined(swarm_id, f_id) => {
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
                        }
                    } else {
                        own_swarm_started = true;
                    }
                }
                FromGnomeManager::NewSwarmAvailable(swarm_name) => {
                    eprintln!("NewSwarm available, joining: {}", swarm_name);
                    let _ = to_gnome_mgr.send(ToGnomeManager::JoinSwarm(swarm_name));
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
                    let app_data = ApplicationData::empty();
                    eprintln!("spawning new serve_app_data");
                    spawn(serve_app_data(
                        s_id,
                        app_data,
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
                    eprintln!("AppMgr received Disconnected");
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
                    let _ = app_mgr
                        .active_app_data
                        .send(ToAppData::ReadData(c_id))
                        .await;
                }
                ToAppMgr::ReadResult(s_id, c_id, data_vec) => {
                    let _ = to_user.send(ToApp::ReadResult(s_id, c_id, data_vec));
                }
                ToAppMgr::TransformLinkRequest(boxed_s_data) => {
                    let _ = app_mgr
                        .active_app_data
                        .send(ToAppData::TransformLinkRequest(*boxed_s_data))
                        .await;
                }
                ToAppMgr::SetActiveApp(gnome_id) => {
                    if let Ok(s_id) = app_mgr.set_active(&gnome_id) {
                        to_user.send(ToApp::ActiveSwarm(gnome_id, s_id));
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
                ToAppMgr::SendManifest => {
                    let _ = app_mgr.active_app_data.send(ToAppData::SendManifest).await;
                }
                ToAppMgr::ListNeighbors => {
                    let _ = app_mgr.active_app_data.send(ToAppData::ListNeighbors).await;
                }
                ToAppMgr::NeighborsListing(s_id, neighbors) => {
                    let _ = to_user.send(ToApp::Neighbors(s_id, neighbors));
                }
                ToAppMgr::ChangeContent(d_type, data) => {
                    let _ = app_mgr
                        .active_app_data
                        .send(ToAppData::ChangeContent(d_type, data))
                        .await;
                }
                ToAppMgr::AddContent(s_id, data) => {
                    let _ = app_mgr
                        .active_app_data
                        .send(ToAppData::AddContent(data))
                        .await;
                }
                ToAppMgr::ContentAdded(s_id, c_id, d_type) => {
                    let _ = to_user.send(ToApp::NewContent(s_id, c_id, d_type));
                }
                ToAppMgr::Quit => {
                    eprintln!("AppMgr received Quit");
                    let _ = to_gnome_mgr.send(ToGnomeManager::Disconnect);
                    let _ = app_mgr.active_app_data.send(ToAppData::Terminate).await;
                    break;
                }
            }
        }
    }
    eprintln!("Done serving AppMgr");
}

async fn serve_app_data(
    swarm_id: SwarmID,
    mut app_data: ApplicationData,
    app_data_recv: AReceiver<ToAppData>,
    to_gnome_sender: Sender<ToGnome>,
    to_app_mgr_send: Sender<ToAppMgr>,
) {
    let mut b_cast_origin: Option<(CastID, Sender<CastData>)> = None;
    let mut link_with_transform_info: Option<ContentID> = None;
    let mut b_req_sent = false;
    let mut next_val = 0;
    let sleep_time = Duration::from_millis(32);
    while let Ok(resp) = app_data_recv.recv().await {
        match resp {
            ToAppData::AppDataSynced(is_synced) => {
                if !is_synced {
                    eprintln!("App not synced!");
                    let sync_requests: Vec<SyncRequest> = vec![
                        SyncRequest::Datastore,
                        SyncRequest::AllFirstPages,
                        SyncRequest::AllPages(vec![0]),
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
                } else {
                    eprintln!("App synced");
                }
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
            ToAppData::AddContent(data) => {
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
                    let msg = SyncMessage::new(SyncMessageType::AddContent(0), reqs, data);
                    let parts = msg.into_parts();
                    for part in parts {
                        let _ = to_gnome_sender.send(ToGnome::AddData(part));
                    }
                    next_val += 1;
                }
            }
            ToAppData::ChangeContent(d_type, data) => {
                let c_id: u16 = 1;
                let pre_hash_result = app_data.content_root_hash(c_id);
                // println!("About to change content {:?}", pre_hash_result);
                if let Ok(pre_hash) = pre_hash_result {
                    let pre: Vec<(ContentID, u64)> = vec![(c_id, pre_hash)];
                    let data = Data::new(vec![next_val]).unwrap();
                    let post: Vec<(ContentID, u64)> = vec![(c_id, data.get_hash())];
                    // We prepend 0 to indicate it is not a Link
                    // let data = Data::new(vec![0, next_val]).unwrap();
                    let reqs = SyncRequirements { pre, post };
                    let msg =
                        SyncMessage::new(SyncMessageType::ChangeContent(d_type, c_id), reqs, data);
                    let parts = msg.into_parts();
                    for part in parts {
                        let _ = to_gnome_sender.send(ToGnome::AddData(part));
                    }
                    next_val += 1;
                }
            }
            ToAppData::SendManifest => {
                let mut prebytes = vec![0];
                if let Ok(hash) = app_data.content_root_hash(0) {
                    for byte in hash.to_be_bytes() {
                        prebytes.push(byte);
                    }
                } else {
                    for _i in 0..8 {
                        prebytes.push(0);
                    }
                };
                // println!("Prebytes: {:?}", prebytes);
                let mani = manifest();
                let pre: Vec<(ContentID, u64)> = vec![(0, 0)];
                let post: Vec<(ContentID, u64)> = vec![(0, mani.hash())];
                let reqs = SyncRequirements { pre, post };
                let msg = SyncMessage::new(SyncMessageType::SetManifest, reqs, mani.to_data(None));
                let parts = msg.into_parts();
                // println!(
                //     "to_data len: {}, hash: {:?}",
                //     mani.to_data(None).len(),
                //     mani.to_data(None).hash().to_be_bytes()
                // );

                for part in parts {
                    let _ = to_gnome_sender.send(ToGnome::AddData(part));
                }
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
                let d_type = 7;
                let total_parts = 128;
                let big_chunks = vec![BigChunk(0, total_parts)];
                // Then for each big-chunk:
                for mut big_chunk in big_chunks.into_iter() {
                    let description = String::new();
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
                        tags: vec![],
                        size: 0,
                        root_hash,
                        broadcast_id,
                        description,
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
                        let link =
                            Content::Link(GnomeId(u64::MAX), String::new(), u16::MAX, Some(ti));
                        let link_hash = link.hash();
                        eprintln!("Link hash: {}", link_hash);
                        let data = link.to_data().unwrap();
                        eprintln!("Link data: {:?}", data);
                        eprintln!("Data hash: {}", data.get_hash());
                        let post: Vec<(ContentID, u64)> = vec![(content_id, link_hash)];
                        let reqs = SyncRequirements { pre, post };
                        let msg = SyncMessage::new(SyncMessageType::AddContent(255), reqs, data);
                        let parts = msg.into_parts();
                        for part in parts {
                            let _ = to_gnome_sender.send(ToGnome::AddData(part));
                        }
                        //TODO: we need to set this upon receiving Gnome's confirmation
                        link_with_transform_info = Some(content_id);
                        next_val += 1;
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
                    SyncMessageType::SetManifest => {
                        let old_manifest = app_data.get_all_data(0);
                        if !requirements.pre_validate(0, &app_data) {
                            eprintln!("PRE validation failed");
                        } else {
                            let content = Content::Data(0, ContentTree::Filled(data));
                            let next_id = app_data.next_c_id().unwrap();
                            let res = if next_id == 0 {
                                app_data.append(content).is_ok()
                            } else {
                                app_data.update(0, content).is_ok()
                            };
                            // println!("Manifest result: {:?}", res);
                            if !requirements.post_validate(0, &app_data) {
                                eprintln!("POST validation failed");
                                if let Ok(data_vec) = old_manifest {
                                    let c_tree = ContentTree::from(data_vec);
                                    let old_content = Content::Data(0, c_tree);
                                    let res = app_data.update(0, old_content);
                                    eprintln!("Restored old manifest {:?}", res.is_ok());
                                } else {
                                    let content = Content::Data(0, ContentTree::Empty(0));
                                    let _ = app_data.update(0, content);
                                    eprintln!("Zeroed manifest");
                                }
                            }
                            let hash = app_data.root_hash();
                            eprintln!("Sending updated hash: {}", hash);
                            let res = to_gnome_sender.send(ToGnome::UpdateAppRootHash(hash));
                            eprintln!("Send res: {:?}", res);
                            // println!("Root hash: {}", app_data.root_hash());
                        }
                    }
                    SyncMessageType::AddContent(d_type) => {
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
                                let res = app_data.append(content);
                                eprintln!("Content added: {:?}", res);
                                let hash = app_data.root_hash();
                                eprintln!("Sending updated hash: {}", hash);
                                let res = to_gnome_sender.send(ToGnome::UpdateAppRootHash(hash));
                                eprintln!("Send res: {:?}", res);
                                let _to_mgr_res = to_app_mgr_send
                                    .send(ToAppMgr::ContentAdded(swarm_id, recv_id, d_type));
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
                        if !requirements.pre_validate(c_id, &app_data) {
                            eprintln!("PRE validation failed for ChangeContent");
                            continue;
                        }
                        let content = Content::from(d_type, data).unwrap();
                        let res = app_data.update(c_id, content);
                        if let Ok(old_content) = res {
                            if !requirements.post_validate(c_id, &app_data) {
                                let restore_res = app_data.update(c_id, old_content);
                                eprintln!("POST validation failed on ChangeContent");
                                eprintln!("Restore result: {:?}", restore_res);
                            } else {
                                let hash = app_data.root_hash();
                                eprintln!("Sending updated hash: {}", hash);
                                let res = to_gnome_sender.send(ToGnome::UpdateAppRootHash(hash));
                                eprintln!("Send res: {:?}", res);
                                eprintln!("ChangeContent completed successfully ({})", hash);
                            }
                        } else {
                            eprintln!("Update procedure failed: {:?}", res);
                        }
                    }
                    SyncMessageType::AppendData(c_id) => {
                        //TODO
                        eprintln!("SyncMessageType::AppendData ");
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
                                eprintln!("Sending updated hash: {}", hash);
                                let res = to_gnome_sender.send(ToGnome::UpdateAppRootHash(hash));
                                eprintln!("Send res: {:?}", res);
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
                        let res = app_data.remove_data(c_id, d_id);
                        if let Ok(removed_data) = res {
                            if !requirements.post_validate(c_id, &app_data) {
                                eprintln!("POST validation failed for RemoveData");
                                // TODO: restore previous order
                                let res = app_data.insert_data(c_id, d_id, removed_data);
                                eprintln!("Restore result: {:?}", res);
                            } else {
                                eprintln!("Data appended successfully ({})", app_data.root_hash());
                            }
                        }
                    }
                    SyncMessageType::UpdateData(c_id, d_id) => {
                        //TODO
                        eprintln!("SyncMessageType::UpdateData ");
                        if !requirements.pre_validate(c_id, &app_data) {
                            eprintln!("PRE validation failed for UpdateData");
                            continue;
                        }
                        // TODO
                        let res = app_data.remove_data(c_id, d_id);
                        if let Ok(removed_data) = res {
                            if !requirements.post_validate(c_id, &app_data) {
                                eprintln!("POST validation failed for RemoveData");
                                // TODO: restore previous order
                                let res = app_data.insert_data(c_id, d_id, removed_data);
                                eprintln!("Restore result: {:?}", res);
                            } else {
                                let hash = app_data.root_hash();
                                eprintln!("Sending updated hash: {}", hash);
                                let res = to_gnome_sender.send(ToGnome::UpdateAppRootHash(hash));
                                eprintln!("Send res: {:?}", res);
                                eprintln!("Data appended successfully ({})", hash);
                            }
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
                        let mut sync_req_iter = sync_requests.into_iter();
                        while let Some(req) = sync_req_iter.next() {
                            match req {
                                SyncRequest::Datastore => {
                                    // TODO: here we should trigger a separate task for sending
                                    // all root hashes to specified Neighbor
                                    let sync_type = 0;
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
                                SyncRequest::AllFirstPages => {
                                    // TODO: here we should trigger a separate task for sending
                                    // all Content's main pages to specified Neighbor
                                    eprintln!(
                                        "We are requested to send all main pages to neighbor."
                                    );
                                    let sync_type = 0;
                                    for c_id in 1..app_data.next_c_id().unwrap() {
                                        eprintln!(
                                            "We need to send main page of ContentID-{}",
                                            c_id
                                        );
                                        if let Ok(data) = app_data.read_data(c_id, 0) {
                                            let (data_type, len) =
                                                app_data.get_type_and_len(c_id).unwrap();
                                            let sync_response =
                                                SyncResponse::Page(c_id, data_type, 0, len, data);
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
                                SyncRequest::Hashes(c_id, d_type, h_ids) => {
                                    //TODO
                                    eprintln!(
                                        "We should send some page hashes of Contents: {:?}",
                                        c_id
                                    );
                                    let hash_res = if let Ok((data_type, len)) =
                                        app_data.get_type_and_len(c_id)
                                    {
                                        if data_type == d_type {
                                            app_data.get_all_page_hashes(c_id)
                                        } else if data_type == 255 {
                                            app_data.get_all_transform_info_hashes(c_id, d_type)
                                        } else {
                                            Err(AppError::DatatypeMismatch)
                                        }
                                    } else {
                                        Err(AppError::IndexingError)
                                    };
                                    if let Ok(mut hashes) = hash_res {
                                        if !hashes.is_empty() {
                                            let sync_type = 0;
                                            let hashes_len = hashes.len() as u16 - 1;
                                            for h_id in h_ids {
                                                if h_id <= hashes_len {
                                                    let hash_data = std::mem::replace(
                                                        &mut hashes[h_id as usize],
                                                        Data::empty(0),
                                                    );
                                                    let sync_response = SyncResponse::Hashes(
                                                        c_id, h_id, hashes_len, hash_data,
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
                                                            h_id, hashes_len, c_id
                                                        );
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
                                        } else if data_type == 255 {
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
                                        "We are requested to send all pages to neighbor ({:?}).",
                                        c_ids
                                    );
                                    let sync_type = 0;
                                    for c_id in c_ids {
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
                                            eprintln!("Sent CID response");
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
                            // println!("Deserialized response!: {:?}", response);
                            match response {
                                SyncResponse::Datastore(part_no, total, hashes) => {
                                    //TODO: we need to cover case where parts come out of order
                                    for (data_type, hash) in hashes {
                                        let tree = ContentTree::empty(hash);
                                        let content = Content::Data(data_type, tree);
                                        let c_id = app_data.next_c_id().unwrap();
                                        let res = app_data.append(content);
                                        let _ = to_app_mgr_send.send(ToAppMgr::ContentAdded(
                                            swarm_id, c_id, data_type,
                                        ));
                                        eprintln!("Datastore add: {:?}", res);
                                    }
                                }
                                SyncResponse::Hashes(c_id, part_no, total, hashes) => {
                                    //TODO: not yet implemented
                                    eprintln!(
                                        "Received hashes [{}/{}] of {} (len: {})!",
                                        part_no,
                                        total,
                                        c_id,
                                        hashes.len()
                                    );
                                    let upd_res = app_data.update_transformative_link(
                                        true, c_id, part_no, total, hashes,
                                    );
                                    eprintln!("Update res: {:?}", upd_res);
                                }
                                SyncResponse::Page(c_id, data_type, page_no, total, data) => {
                                    //TODO: make it proper
                                    if page_no == 0 {
                                        eprintln!(
                                            "We've got main page of {}, data type: {}",
                                            c_id, data_type
                                        );
                                        if let Ok((d_type, len)) = app_data.get_type_and_len(c_id) {
                                            eprintln!("CID {} already exists", c_id);
                                            if d_type == 255 {
                                                if len > 0 {
                                                    eprintln!("Update existing Link");
                                                    let _ = app_data.update_transformative_link(
                                                        false, c_id, page_no, total, data,
                                                    );
                                                } else {
                                                    eprintln!("Create new Link");
                                                    let res = app_data
                                                        .update(c_id, data_to_link(data).unwrap());
                                                    eprintln!("Update result: {:?}", res);
                                                }
                                            } else {
                                                let _res =
                                                    app_data.insert_data(c_id, page_no, data);
                                                eprintln!("Update existing Data: {:?}", _res);
                                            }
                                        } else if data_type < 255 {
                                            eprintln!("Create new Data");
                                            let _ = to_app_mgr_send.send(ToAppMgr::ContentAdded(
                                                swarm_id, c_id, data_type,
                                            ));
                                            let _res = app_data.update(
                                                c_id,
                                                Content::Data(data_type, ContentTree::Filled(data)),
                                            );
                                        } else {
                                            eprintln!("Create new Link 2");
                                            let _ = to_app_mgr_send.send(ToAppMgr::ContentAdded(
                                                swarm_id, c_id, data_type,
                                            ));
                                            let res =
                                                app_data.update(c_id, data_to_link(data).unwrap());
                                            eprintln!("Update result: {:?}", res);
                                        }

                                        // if data_type < 255 {
                                        // } else {
                                        //     //TODO if we received data for existinglink with TransformInfo
                                        // };
                                    } else if let Ok((d_type, len)) =
                                        app_data.get_type_and_len(c_id)
                                    {
                                        if d_type == data_type {
                                            let res = app_data.append_data(c_id, data);
                                            eprintln!("Page #{} add result: {:?}", page_no, res);
                                        } else if d_type == 255 {
                                            let res = app_data.update_transformative_link(
                                                false, c_id, page_no, total, data,
                                            );
                                            // println!("Update TI result: {:?}", res);
                                        } else {
                                            eprintln!(
                                                "Error: Stored:{} len:{}\nreceived:{} len:{}",
                                                d_type, len, data_type, total
                                            );
                                        }
                                    } else {
                                        eprintln!("Datastore couldn't find ContentID {}", c_id);
                                    }
                                }
                            }
                        }
                        // Datastore sync root hashes
                        // println!("Response 0, {}", cast_data);
                        // let mut bytes = cast_data.bytes().into_iter();
                        // let data_type = bytes.next().unwrap();
                        // let _content_id_1 = bytes.next().unwrap();
                        // let _content_id_2 = bytes.next().unwrap();
                        // let _part_no_1 = bytes.next().unwrap();
                        // let _part_no_2 = bytes.next().unwrap();
                        // let _total_1 = bytes.next().unwrap();
                        // let _total_2 = bytes.next().unwrap();
                        // let bytes: Vec<u8> = bytes.collect();

                        // for chunk in bytes.chunks(8) {
                        //     let hash = u64::from_be_bytes(chunk[0..8].try_into().unwrap());
                        //     let tree = ContentTree::empty(hash);
                        //     let content = Content::Data(data_type, tree);
                        //     let res = app_data.append(content);
                        //     println!("Datastore add: {:?}", res);
                        // }
                    }
                    // 1 => {
                    // // Datastore sync Content
                    // println!("Response 1");
                    //     let mut bytes = cast_data.bytes().into_iter();
                    //     // println!("bytes: {:?}", bytes);
                    //     let data_subtype = bytes.next().unwrap();
                    //     let data_type = bytes.next().unwrap();
                    //     println!("subtype: {}", data_subtype);
                    //     let content_id_1 = bytes.next().unwrap();
                    //     let content_id_2 = bytes.next().unwrap();
                    //     let c_id = u16::from_be_bytes([content_id_1, content_id_2]);
                    //     let part_no_1 = bytes.next().unwrap();
                    //     let part_no_2 = bytes.next().unwrap();
                    //     let part_no = u16::from_be_bytes([part_no_1, part_no_2]);
                    //     let total_1 = bytes.next().unwrap();
                    //     let total_2 = bytes.next().unwrap();
                    //     let total = u16::from_be_bytes([total_1, total_2]);
                    //     let bytes: Vec<u8> = bytes.collect();
                    //     if data_subtype == 1 {
                    //         // println!("Content {} add part {} of {}", c_id, part_no, total);
                    //         if c_id == 0 {
                    //             println!("App manifest to add");
                    //             let content = Content::Data(
                    //                 data_type,
                    //                 ContentTree::Filled(Data::new(bytes).unwrap()),
                    //             );
                    //             let res = app_data.update(0, content);
                    //             println!("App manifest add result: {:?}", res);
                    //         } else {
                    //             println!("Content {} of type {} to add", c_id, data_type);
                    //         }
                    //     } else if data_subtype == 0 {
                    //         println!(
                    //             "We have page #{} out of {} belonging to ContentID {}({}).",
                    //             part_no, total, c_id, data_type
                    //         );
                    //         let data = Data::new(bytes).unwrap();
                    //         let content = if data_type < 255 {
                    //             Content::Data(data_type, ContentTree::Filled(data))
                    //         } else {
                    //             data_to_link(data).unwrap()
                    //         };

                    //         let res = app_data.update(c_id, content);
                    //         println!("Page #{} add result: {:?}", part_no, res);
                    //     }
                    // }
                    // 2 => {
                    //     println!("Response 2");
                    // }
                    other => {
                        eprintln!("Response {}", other);
                    }
                }
            }
            ToAppData::ReadData(c_id) => {
                //TODO:
                if let Ok(data_vec) = app_data.get_all_data(c_id) {
                    let _ = to_app_mgr_send.send(ToAppMgr::ReadResult(swarm_id, c_id, data_vec));
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
                                SyncRequest::Hashes(a_msg.content_id, d_type, missing_hashes);
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
                eprintln!("Done serving AppData");
                break;
            }
            _ => {
                eprintln!("Unserved by app: {:?}", resp);
            }
        }
    }
    eprintln!("AppData out of loop");
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
                GnomeToApp::AppDataSynced(synced) => {
                    eprintln!("Gnome says if synced: {}", synced);
                    let _ = to_app_data_send
                        .send(ToAppData::AppDataSynced(synced))
                        .await;
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
                        eprintln!("Unserved Reconfig from Gnome: {}", id);
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
    pub fn empty() -> Self {
        ApplicationData {
            change_reg: ChangeRegistry::new(),
            contents: Datastore::Empty,
            hash_to_temp_idx: HashMap::new(),
            partial_data: HashMap::new(),
        }
    }
    pub fn new(manifest: ApplicationManifest) -> Self {
        ApplicationData {
            change_reg: ChangeRegistry::new(),
            contents: Datastore::new(manifest),
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

    // TODO: this needs rework as well as SyncMessage::from_data
    pub fn process(&mut self, data: SyncData) -> Option<SyncMessage> {
        let mut bytes = data.bytes();
        let m_type = SyncMessageType::new(&mut bytes);
        let mut drained_bytes = m_type.as_bytes();
        let part_no = bytes.drain(0..1).next().unwrap();
        let total_parts = bytes.drain(0..1).next().unwrap();
        drained_bytes.push(part_no);
        drained_bytes.push(total_parts);
        // println!("[{}/{}]", part_no, total_parts);
        if part_no == 0 {
            if total_parts == 0 {
                let mut hm = HashMap::new();
                drained_bytes.append(&mut bytes);
                hm.insert(0, Data::new(drained_bytes).unwrap());
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
                    // println!("Expecting hash: {}", hash);
                    all_hashes.push(hash);
                    self.hash_to_temp_idx.insert(hash, next_idx);
                }
                let mut new_hm = HashMap::new();
                drained_bytes.append(&mut bytes);
                let data = Data::new(drained_bytes).unwrap();
                new_hm.insert(0, data);
                self.partial_data.insert(next_idx, (all_hashes, new_hm));
                None
            }
        } else {
            // Second byte is non zero, so we received a non-head partial Data
            drained_bytes.append(&mut bytes);
            let data = Data::new(drained_bytes).unwrap();
            let hash = data.get_hash();
            // println!("Got hash: {}", hash);
            if let Some(temp_idx) = self.hash_to_temp_idx.get(&hash) {
                // println!("Oh yeah");
                if let Some((vec, mut hm)) = self.partial_data.remove(temp_idx) {
                    hm.insert(hash, data);
                    // println!("{} ==? {}", vec.len(), hm.len());
                    if vec.len() == hm.len() {
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
    pub fn content_root_hash(&self, c_id: ContentID) -> Result<u64, AppError> {
        let result = self.contents.get_root_content_typed_hash(c_id);
        if let Ok((_d_type, hash)) = result {
            Ok(hash)
        } else {
            Err(result.err().unwrap())
        }
    }
    pub fn registry(&self) -> Vec<ContentID> {
        self.change_reg.read()
    }

    pub fn all_content_typed_root_hashes(&self) -> Vec<Vec<(u8, u64)>> {
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
    pub fn remove_data(&mut self, c_id: ContentID, d_id: u16) -> Result<Data, AppError> {
        self.contents.remove_data(c_id, d_id)
    }
    pub fn update(&mut self, c_id: ContentID, content: Content) -> Result<Content, AppError> {
        self.contents.update(c_id, content)
    }
    pub fn get_all_data(&self, c_id: ContentID) -> Result<Vec<Data>, AppError> {
        let mut data_vec = vec![];
        for i in 0..u16::MAX {
            if let Ok(data) = self.contents.read_data((c_id, i)) {
                data_vec.push(data);
            } else {
                break;
            }
        }
        Ok(data_vec)
    }
    pub fn get_all_page_hashes(&self, c_id: ContentID) -> Result<Vec<Data>, AppError> {
        let hashes_vec = self.contents.content_bottom_hashes(c_id)?;
        eprintln!("Got hashes vec");
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
        d_type: DataType,
    ) -> Result<Vec<Data>, AppError> {
        self.contents.link_transform_info_hashes(c_id, d_type)
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
