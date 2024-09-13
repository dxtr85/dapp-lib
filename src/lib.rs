use crate::prelude::SyncRequirements;
use crate::prelude::TransformInfo;
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
use std::collections::HashMap;
use std::collections::HashSet;

use crate::content::double_hash;
use async_std::task::sleep;
use async_std::task::spawn;
pub use config::Configuration;
use content::ContentTree;
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
    pub use crate::content::{double_hash, Content, ContentID, ContentTree, TransformInfo};
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
    pub use gnome::prelude::CastData;
    pub use gnome::prelude::GnomeId;
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

pub enum ToUser {
    //TODO
}

pub enum ToAppMgr {
    UploadData,
    StartUnicast,
    StartBroadcast,
    SendManifest,
    ListNeighbors,
    ChangeContent,
    AddContent,
}
// enum ToApp {}

#[derive(Debug)]
pub enum ToAppData {
    Response(GnomeToApp),
    UploadData,
    StartUnicast,
    StartBroadcast,
    SendManifest,
    ListNeighbors,
    AppDataSynced(bool),
    BCastData(CastID, CastData),
    BCastOrigin(CastID, Sender<CastData>),
    ChangeContent,
    AddContent,
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

pub fn initialize(config: Configuration) -> (Sender<ToAppMgr>, Receiver<ToUser>) {
    let (gmgr_send, gmgr_recv) = init(config.work_dir.clone(), config.app_data_root_hash);
    let (to_user_send, to_user_recv) = channel();
    let (to_app_mgr_send, to_app_mgr_recv) = channel();
    spawn(serve_gnome_manager(
        gmgr_send,
        gmgr_recv,
        to_user_send,
        to_app_mgr_recv,
    ));
    (to_app_mgr_send, to_user_recv)
}

async fn serve_gnome_manager(
    send: Sender<ToGnomeManager>,
    recv: Receiver<FromGnomeManager>,
    to_user_send: Sender<ToUser>,
    to_app_mgr_recv: Receiver<ToAppMgr>,
) {
    // TODO: AppMgr should hold state
    let mut app_mgr = ApplicationManager::new();

    let sleep_time = Duration::from_millis(128);
    loop {
        sleep(sleep_time).await;
        while let Ok(FromGnomeManager::SwarmJoined(s_id, _s_name, to_swarm, from_swarm)) =
            recv.try_recv()
        {
            // println!("recv swarm joined");
            // TODO: we need to identify to which application should we assign
            //       given swarm
            // TODO: we need a bi-directional communication between
            // AppMgr and each AppData
            // TODO: we need a bi-directional comm between AppMgr and user App
            let (to_app_data_send, to_app_data_recv) = achannel::bounded(32);
            app_mgr.add_app_data(s_id, to_app_data_send.clone());
            let app_data = ApplicationData::empty();
            spawn(serve_app_data(app_data, to_app_data_recv, to_swarm));
            spawn(serve_swarm(
                Duration::from_millis(64),
                from_swarm,
                to_app_data_send.clone(),
            ));
            // } else {
            //     return;
        }
        while let Ok(message) = to_app_mgr_recv.try_recv() {
            match message {
                ToAppMgr::UploadData => {
                    let _ = app_mgr.active_app_data.send(ToAppData::UploadData).await;
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
                ToAppMgr::SendManifest => {
                    let _ = app_mgr.active_app_data.send(ToAppData::SendManifest).await;
                }
                ToAppMgr::ListNeighbors => {
                    let _ = app_mgr.active_app_data.send(ToAppData::ListNeighbors).await;
                }
                ToAppMgr::ChangeContent => {
                    let _ = app_mgr.active_app_data.send(ToAppData::ChangeContent).await;
                }
                ToAppMgr::AddContent => {
                    let _ = app_mgr.active_app_data.send(ToAppData::AddContent).await;
                }
            }
        }
    }
}

async fn serve_app_data(
    mut app_data: ApplicationData,
    app_data_recv: AReceiver<ToAppData>,
    to_gnome_sender: Sender<ToGnome>,
) {
    let mut b_cast_origin: Option<(CastID, Sender<CastData>)> = None;
    let mut b_req_sent = false;
    let mut next_val = 0;
    let sleep_time = Duration::from_millis(128);
    // loop {
    //     sleep(sleep_time).await;
    while let Ok(resp) = app_data_recv.recv().await {
        match resp {
            ToAppData::AppDataSynced(is_synced) => {
                if !is_synced {
                    println!("App not synced!");
                    // service_request.send(Request::AskData(GnomeId::any(), NeighborRequest::AppSyncRequest( )))
                }
            }
            ToAppData::StartUnicast => {
                let res =
                    to_gnome_sender.send(ToGnome::StartUnicast(GnomeId(15561580566906229863)));
                println!("UnicastReq: {:?}", res);
                // next_val += 1;
            }
            ToAppData::StartBroadcast => {
                let res = to_gnome_sender.send(ToGnome::StartBroadcast);
                b_req_sent = res.is_ok();
            }
            ToAppData::AddContent => {
                if let Some(next_id) = app_data.next_c_id() {
                    let pre: Vec<(ContentID, u64)> = vec![(next_id, 0)];
                    let data = Data::new(vec![next_val]).unwrap();
                    let post: Vec<(ContentID, u64)> = vec![(next_id, data.hash())];
                    let data = Data::new(vec![0, next_val]).unwrap();
                    let reqs = SyncRequirements { pre, post };
                    let msg = SyncMessage::new(SyncMessageType::AddContent, reqs, data);
                    let parts = msg.into_parts();
                    for part in parts {
                        let _ = to_gnome_sender.send(ToGnome::AddData(part));
                    }
                    next_val += 1;
                }
            }
            ToAppData::ChangeContent => {
                let c_id: u16 = 1;
                let pre_hash_result = app_data.content_root_hash(c_id);
                // println!("About to change content {:?}", pre_hash_result);
                if let Ok(pre_hash) = pre_hash_result {
                    // let pre_hash = pre_hash_result.unwrap();
                    let pre: Vec<(ContentID, u64)> = vec![(c_id, pre_hash)];
                    let data = Data::new(vec![next_val]).unwrap();
                    let post: Vec<(ContentID, u64)> = vec![(c_id, data.hash())];
                    // We prepend 0 to indicate it is not a Link
                    let data = Data::new(vec![0, next_val]).unwrap();
                    let reqs = SyncRequirements { pre, post };
                    let msg = SyncMessage::new(SyncMessageType::ChangeContent(c_id), reqs, data);
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
                // let manifest_hash = mani.hash();
                // prebytes.append(&mut Vec::from(manifest_hash.to_be_bytes()));
                // println!("Prebytes: {:?}", prebytes);

                for part in parts {
                    let _ = to_gnome_sender.send(ToGnome::AddData(part));
                }
            }
            ToAppData::ListNeighbors => {
                let _ = to_gnome_sender.send(ToGnome::ListNeighbors);
            }
            ToAppData::UploadData => {
                //TODO: send to AppMgr UploadData message
                // this logic should be moved to app mgr
                if b_cast_origin.is_none() {
                    println!("Unable to upload - no active broadcast.");
                    if !b_req_sent {
                        println!("Requesting broadcast channel.");
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
                    println!("// 3. Split big-chunk into 1024byte small-chunks");
                    while let Some(small_chunk) = big_chunk.next() {
                        // 4. Compute hash for each small-chunk
                        hashes.push(small_chunk.hash());
                        // TODO: build proper CastData from Data
                        data_vec.push(small_chunk);
                    }
                    println!("// 5. Compute root hash from previous hashes.");
                    let root_hash = get_root_hash(&hashes);
                    println!("// 6. Instantiate TransformInfo");
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
                    //
                    println!(
                        "// 7. SyncMessage::Append as many Data::Link to Datastore as necessary"
                    );
                    if let Some(content_id) = app_data.next_c_id() {
                        println!("ContentID: {}", content_id);
                        let pre: Vec<(ContentID, u64)> = vec![(content_id, 0)];
                        let link =
                            Content::Link(GnomeId(u64::MAX), String::new(), u16::MAX, Some(ti));
                        let link_hash = link.hash();
                        println!("Link hash: {}", link_hash);
                        let data = link.to_data().unwrap();
                        println!("Link data: {:?}", data);
                        println!("Data hash: {}", data.hash());
                        let post: Vec<(ContentID, u64)> = vec![(content_id, link_hash)];
                        let reqs = SyncRequirements { pre, post };
                        let msg = SyncMessage::new(SyncMessageType::AddContent, reqs, data);
                        let parts = msg.into_parts();
                        for part in parts {
                            let _ = to_gnome_sender.send(ToGnome::AddData(part));
                        }
                        next_val += 1;
                        println!("// 8. For each Link Send computed Hashes via broadcast");
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
                        println!("duplicating hashes");
                        hash_bytes.append(&mut hash_bytes.clone());
                        println!("spawning serve_broadcast_origin");
                        spawn(serve_broadcast_origin(
                            broadcast_id,
                            Duration::from_millis(1000),
                            bcast_send.clone(),
                            hash_bytes,
                            done_send.clone(),
                        ));
                        sleep(sleep_time).await;
                        let _done_res = done_recv.recv();
                        println!("Hashes sent: {}", _done_res.is_ok());
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
                            Duration::from_millis(1000),
                            bcast_send.clone(),
                            c_data_vec,
                            done_send.clone(),
                        ));
                        // let done_res = done_recv.recv();
                    }
                }
            }
            ToAppData::Response(GnomeToApp::Block(_id, data)) => {
                // println!("Processing data...");
                let process_result = app_data.process(data);
                if process_result.is_none() {
                    continue;
                }
                // println!("Process response: {:?}", process_result);
                // println!("Process response");
                let SyncMessage {
                    m_type,
                    requirements,
                    data,
                } = process_result.unwrap();

                // let b_type = data.first_byte();
                // println!("Received m_type: {:?}", m_type);
                match m_type {
                    SyncMessageType::SetManifest => {
                        let old_manifest = app_data.get_all_data(0);
                        if !requirements.pre_validate(0, &app_data) {
                            println!("PRE validation failed");
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
                                println!("POST validation failed");
                                if let Ok(data_vec) = old_manifest {
                                    let c_tree = ContentTree::from(data_vec);
                                    let old_content = Content::Data(0, c_tree);
                                    let res = app_data.update(0, old_content);
                                    println!("Restored old manifest {:?}", res.is_ok());
                                } else {
                                    let content = Content::Data(0, ContentTree::Empty(0));
                                    let _ = app_data.update(0, content);
                                    println!("Zeroed manifest");
                                }
                            }
                            let hash = app_data.root_hash();
                            println!("Sending updated hash: {}", hash);
                            let res = to_gnome_sender.send(ToGnome::UpdateAppRootHash(hash));
                            println!("Send res: {:?}", res);
                            // println!("Root hash: {}", app_data.root_hash());
                        }
                    }
                    SyncMessageType::AddContent => {
                        // TODO: potentially for AddContent & ChangeContent
                        // post requirements could be empty
                        // pre requirements can not be empty since we need
                        // ContentID
                        let next_id = app_data.next_c_id().unwrap();
                        if !requirements.pre_validate(next_id, &app_data) {
                            println!("PRE validation failed for AddContent");
                        } else {
                            if requirements.post.len() != 1 {
                                println!(
                                    "POST validation failed for AddContent 1 ({:?})",
                                    requirements.post
                                );
                                continue;
                            }
                            let content = Content::from(data).unwrap();
                            // println!("Content: {:?}", content);
                            let (recv_id, recv_hash) = requirements.post[0];
                            println!("Recv id: {}, next id: {}", recv_id, next_id);
                            println!("Recv hash: {}, next hash: {}", recv_hash, content.hash());
                            if recv_id == next_id && recv_hash == content.hash() {
                                let res = app_data.append(content);
                                println!("Content added: {:?}", res);
                            } else {
                                println!("POST validation failed for AddContent two");
                            }
                            // println!("Root hash: {}", app_data.root_hash());
                        }
                    }
                    SyncMessageType::ChangeContent(c_id) => {
                        // println!("ChangeContent");
                        if !requirements.pre_validate(c_id, &app_data) {
                            println!("PRE validation failed for ChangeContent");
                            continue;
                        }
                        // let (pre_recv_id, _hash) = requirements.pre[0];
                        // let (post_recv_id, recv_hash) = requirements.post[0];
                        // if pre_recv_id != post_recv_id {
                        //     println!("POST validation failed for ChangeContent 1");
                        //     continue;
                        // }
                        // if requirements.post.len() != 1 {
                        //     println!("POST validation failed for ChangeContent 2");
                        //     continue;
                        // }
                        let content = Content::from(data).unwrap();
                        let res = app_data.update(c_id, content);
                        if let Ok(old_content) = res {
                            if !requirements.post_validate(c_id, &app_data) {
                                let restore_res = app_data.update(c_id, old_content);
                                println!("POST validation failed on ChangeContent");
                                println!("Restore result: {:?}", restore_res);
                            } else {
                                println!(
                                    "ChangeContent completed successfully ({})",
                                    app_data.root_hash()
                                );
                            }
                        } else {
                            println!("Update procedure failed: {:?}", res);
                        }
                        // if recv_hash == content.hash() {
                        //     println!("Content changed: {:?}", res);
                        // } else {
                        //     println!("POST validation failed for ChangeContent");
                        // }
                    }
                    SyncMessageType::AppendData(c_id) => {
                        //TODO
                        println!("SyncMessageType::AppendData ");
                        if !requirements.pre_validate(c_id, &app_data) {
                            println!("PRE validation failed for AppendData");
                            continue;
                        }
                        // let (pre_recv_id, _hash) = requirements.pre[0];
                        // let (post_recv_id, _recv_hash) = requirements.post[0];
                        // if pre_recv_id != post_recv_id {
                        //     println!("POST validation failed for ChangeContent 1");
                        //     continue;
                        // }
                        // TODO
                        let res = app_data.append_data(c_id, data);
                        if res.is_ok() {
                            if !requirements.post_validate(c_id, &app_data) {
                                println!("POST validation failed for AppendData");
                                // TODO: restore previous order
                                let res = app_data.pop_data(c_id);
                                println!("Restore result: {:?}", res);
                            } else {
                                println!("Data appended successfully ({})", app_data.root_hash());
                            }
                        }
                    }
                    SyncMessageType::RemoveData(c_id, d_id) => {
                        //TODO
                        println!("SyncMessageType::RemoveData ");
                        if !requirements.pre_validate(c_id, &app_data) {
                            println!("PRE validation failed for RemoveData");
                            continue;
                        }
                        // let (pre_recv_id, _hash) = requirements.pre[0];
                        // let (post_recv_id, _recv_hash) = requirements.post[0];
                        // if pre_recv_id != post_recv_id {
                        //     println!("POST validation failed for RemoveData 1");
                        //     continue;
                        // }
                        // TODO
                        // let mut bytes = data.bytes();
                        // let data_idx =
                        //     u16::from_be_bytes([data.first_byte(), data.second_byte()]);
                        let res = app_data.remove_data(c_id, d_id);
                        if let Ok(removed_data) = res {
                            if !requirements.post_validate(c_id, &app_data) {
                                println!("POST validation failed for RemoveData");
                                // TODO: restore previous order
                                let res = app_data.insert_data(c_id, d_id, removed_data);
                                println!("Restore result: {:?}", res);
                            } else {
                                println!("Data appended successfully ({})", app_data.root_hash());
                            }
                        }
                    }
                    SyncMessageType::UpdateData(c_id, d_id) => {
                        //TODO
                        println!("SyncMessageType::UpdateData ");
                        if !requirements.pre_validate(c_id, &app_data) {
                            println!("PRE validation failed for UpdateData");
                            continue;
                        }
                        // let (pre_recv_id, _hash) = requirements.pre[0];
                        // let (post_recv_id, _recv_hash) = requirements.post[0];
                        // if pre_recv_id != post_recv_id {
                        //     println!("POST validation failed for UpdateData 1");
                        //     continue;
                        // }
                        // TODO
                        // let mut bytes = data.bytes();
                        // let data_idx =
                        //     u16::from_be_bytes([data.first_byte(), data.second_byte()]);
                        let res = app_data.remove_data(c_id, d_id);
                        if let Ok(removed_data) = res {
                            if !requirements.post_validate(c_id, &app_data) {
                                println!("POST validation failed for RemoveData");
                                // TODO: restore previous order
                                let res = app_data.insert_data(c_id, d_id, removed_data);
                                println!("Restore result: {:?}", res);
                            } else {
                                println!("Data appended successfully ({})", app_data.root_hash());
                            }
                        }
                    }
                    SyncMessageType::InsertData(c_id, d_id) => {
                        //TODO
                        println!("SyncMessageType::InsertData ");
                    }
                    SyncMessageType::ExtendData(c_id, d_id) => {
                        //TODO
                        println!("SyncMessageType::ExtendData ");
                    }
                    SyncMessageType::UserDefined(_value) => {
                        //TODO
                        println!("SyncMessageType::UserDefined({})", _value);
                    }
                }
            }
            ToAppData::Response(GnomeToApp::AppDataSynced(is_synced)) => {
                println!(
                    "AppDataSynced: {}, hash: {}",
                    is_synced,
                    app_data.root_hash()
                );
            }
            ToAppData::Response(GnomeToApp::AppSync(
                sync_type,
                data_type,
                c_id,
                part_no,
                total,
                data,
            )) => {
                println!(
                    "Got AppSync response {} for CID-{} of type {} [{}/{}]:\n{:?}",
                    sync_type,
                    c_id,
                    data_type,
                    part_no,
                    total,
                    data.len()
                );

                match sync_type {
                    0 => {
                        println!("0 data: {}", data);
                        let bytes = data.bytes();
                        let data_type = bytes.first().unwrap();
                        for chunk in bytes[1..].chunks(8) {
                            let hash = u64::from_be_bytes(chunk[0..8].try_into().unwrap());
                            let tree = ContentTree::empty(hash);
                            let content = Content::Data(*data_type, tree);
                            let res = app_data.append(content);
                            println!("Datastore add: {:?}", res);
                            // let _ = service_request.send(Request::AskData(
                            //     gnome_id,
                            //     NeighborRequest::AppSyncRequest(1, data),
                            // ));
                        }
                    }
                    1 => {
                        println!("Got Link with TransformInfo!");
                    }
                    255 => {
                        println!("Content {} add part {} of {}", c_id, part_no, total);
                        if c_id == 0 {
                            println!("App manifest to add");
                            let content = Content::Data(
                                0,
                                ContentTree::Filled(Data::new(data.bytes()).unwrap()),
                            );
                            let res = app_data.update(0, content);
                            println!("App manifest add result: {:?}", res);
                        }
                    }
                    _ => {
                        //TODO
                    }
                }
            }
            ToAppData::Response(GnomeToApp::AppSyncInquiry(gnome_id, sync_type, _data)) => {
                println!("Got AppSync inquiry");
                let hashes = app_data.all_content_root_hashes();
                let c_id = 0;
                let data_type = 0;
                let total = hashes.len() as u16 - 1;
                for (part_no, group) in hashes.into_iter().enumerate() {
                    let mut byte_hashes = vec![];
                    for hash in group.iter() {
                        for byte in hash.to_be_bytes() {
                            byte_hashes.push(byte);
                        }
                    }
                    let _ = to_gnome_sender.send(ToGnome::SendData(
                        gnome_id,
                        NeighborResponse::AppSync(
                            sync_type,
                            data_type,
                            c_id,
                            part_no as u16,
                            total,
                            SyncData::new(byte_hashes).unwrap(),
                        ),
                    ));
                }
                println!("Sent Datastore response");

                let content_id = 0;
                if let Ok(data_vec) = app_data.get_all_data(content_id) {
                    let sync_type = 255;
                    let data_type = 0;
                    let total = data_vec.len();
                    for (part_no, data) in data_vec.into_iter().enumerate() {
                        let _ = to_gnome_sender.send(ToGnome::SendData(
                            gnome_id,
                            NeighborResponse::AppSync(
                                sync_type,
                                data_type,
                                content_id,
                                part_no as u16,
                                total as u16 - 1,
                                data.to_sync(),
                            ),
                        ));
                    }
                    println!("Sent CID response");
                }
            }
            ToAppData::BCastOrigin(c_id, send) => {
                //TODO: we need an option to also receive what we have broadcasted!
                b_cast_origin = Some((c_id, send))
            }
            ToAppData::BCastData(c_id, c_data) => {
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
                    if let Ok(missing) = upd_res {
                        // println!("Missing hashes: {:?}", missing);
                        //TODO: request hashes if missing not empty
                    } else {
                        println!("Unable to update: {:?}", upd_res);
                    }
                } else {
                    let data = a_msg_res.err().unwrap();
                    println!("App Data: {} ", data);
                }
            }
            _ => {
                println!("Unserved by app: {:?}", resp);
            }
        }
    }
    // end here
    // } //loop
}

async fn serve_swarm(
    sleep_time: Duration,
    user_res: Receiver<GnomeToApp>,
    to_app_data_send: ASender<ToAppData>,
) {
    loop {
        sleep(sleep_time).await;
        // let data = user_res.try_recv();
        while let Ok(resp) = user_res.try_recv() {
            // println!("SUR: {:?}", resp);
            match resp {
                GnomeToApp::AppDataSynced(synced) => {
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
                GnomeToApp::BroadcastOrigin(_s_id, ref c_id, cast_data_send) => {
                    let _ = to_app_data_send
                        .send(ToAppData::BCastOrigin(*c_id, cast_data_send))
                        .await;
                    // spawn(serve_broadcast_origin(
                    //     c_id,
                    //     Duration::from_millis(200),
                    //     send_d,
                    // ));
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
                    println!("Got data from {}", c_id.0);
                }
                _ => {
                    // println!("UNserved swarm data: {:?}", _res);
                    let _ = to_app_data_send.send(ToAppData::Response(resp)).await;
                }
            }
            // } else {
            // println!("{:?}", data);
        }
        // sleep(sleep_time).await;
    }
}

async fn serve_unicast(c_id: CastID, sleep_time: Duration, user_res: Receiver<CastData>) {
    println!("Serving unicast {:?}", c_id);
    loop {
        let recv_res = user_res.try_recv();
        if let Ok(data) = recv_res {
            println!("U{:?}: {}", c_id, data);
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
    println!("Originating broadcast {:?}", c_id);
    sleep(Duration::from_secs(4)).await;
    println!("Initial sleep is over");
    // TODO: indexing
    for (i, data) in data_vec.into_iter().enumerate() {
        // loop {
        let send_res = user_res.send(data);
        if send_res.is_ok() {
            print!("BCed: {}\t", i + 1);
        } else {
            println!(
                "Error while trying to broadcast: {:?}",
                send_res.err().unwrap()
            );
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
    println!("Serving broadcast {:?}", c_id);
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
    println!("Originating unicast {:?}", c_id);
    let mut i: u8 = 0;
    loop {
        let send_res = user_res.send(CastData::new(vec![i]).unwrap());
        if send_res.is_ok() {
            println!("Unicasted {}", i);
        } else {
            println!(
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
                _other => return Err(Data::empty()),
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
        // let mut bytes_iter = data.ref_bytes().iter();
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
                    // for j in 0..8 {
                    for item in &mut hash {
                        // hash[j] = bytes.drain(0..1).next().unwrap();
                        *item = bytes.drain(0..1).next().unwrap();
                        drained_bytes.push(*item);
                    }
                    let hash = u64::from_be_bytes(hash);
                    // println!("Expecting hash: {}", hash);
                    all_hashes.push(hash);
                    self.hash_to_temp_idx.insert(hash, next_idx);
                }
                // drop(bytes_iter);
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
            let hash = data.hash();
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
        self.contents.get_root_content_hash(c_id)
    }
    pub fn registry(&self) -> Vec<ContentID> {
        self.change_reg.read()
    }

    pub fn all_content_root_hashes(&self) -> Vec<Vec<u64>> {
        self.contents.all_root_hashes()
    }
    pub fn update_transformative_link(
        &mut self,
        is_hash: bool,
        content_id: ContentID,
        part_no: u16,
        total_parts: u16,
        data: Data,
    ) -> Result<HashSet<u16>, AppError> {
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
