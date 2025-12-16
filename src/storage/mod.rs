use std::collections::HashMap;
// use std::fs;
use std::path::{Path, PathBuf};

// use async_std::channel::Sender;
// use async_std::fs::{self, File, OpenOptions};
use smol::fs::{self, File, OpenOptions};
// use async_std::io::prelude::SeekExt;
// use async_std::io::{BufReader, BufWriter, ReadExt, WriteExt};
use gnome::prelude::{GnomeId, SwarmName};
use smol::io::AsyncSeekExt as SeekExt;
use smol::io::{AsyncReadExt as ReadExt, AsyncWriteExt as WriteExt, BufReader, BufWriter};
// use gnome::prelude::{GnomeId, SwarmName};

use crate::content::{data_to_link, Content, ContentID, ContentTree, DataType};
use crate::prelude::AppType;
use crate::{ApplicationData, Data};

// TODO: We need to define different storage policies given swarm can have:
// - Discard - do not store given swarm on disk
// - Datastore - store only root hashes of CIDs and up to 64 first pages of CID-0
// - SelectMainPages(Vec<CID>) - Datastore + MainPages of selected CIDs
// - MainPages - Datastore + MainPage of every CID
// - SelectStore(bool, Vec<CID>)- Datastore (+ optionally all MainPages) + all pages of select CIDs
// - Everything - Datastore + MainPages + all other pages
//
// Depending on what storage policy we are provided with we decide what to do with data
#[derive(Clone, Debug)]
pub enum StorageCondition {
    IamFounder,
    FounderIs(GnomeId),
    SwarmName(SwarmName),
    CatalogApp,
    ForumApp,
    SearchMatch,
    Default,
}
impl StorageCondition {
    pub fn get(id: usize) -> Self {
        match id {
            0 => Self::IamFounder,
            1 => Self::FounderIs(GnomeId(0)),
            2 => Self::SwarmName(SwarmName {
                founder: GnomeId(0),
                name: "".to_string(),
            }),
            3 => Self::CatalogApp,
            4 => Self::ForumApp,
            5 => Self::SearchMatch,
            _o => Self::Default,
        }
    }
    pub fn get_string(&self) -> String {
        match self {
            Self::IamFounder => "IamFounder".to_string(),
            Self::FounderIs(g_id) => {
                format!("FounderIs {g_id}")
            }
            Self::SwarmName(s_n) => {
                let delimiter = 31 as char;
                format!(
                    "SwarmName {} {delimiter}{}{delimiter}",
                    s_n.founder.to_string(),
                    s_n.name
                )
            }
            Self::CatalogApp => "CatalogApp".to_string(),
            Self::ForumApp => "ForumApp".to_string(),
            Self::SearchMatch => "SearchMatch".to_string(),
            Self::Default => "Default".to_string(),
        }
    }
    pub fn get_id(&self) -> usize {
        match self {
            Self::IamFounder => 0,
            Self::FounderIs(_id) => 1,
            Self::SwarmName(_n) => 2,
            Self::CatalogApp => 3,
            Self::ForumApp => 4,
            Self::SearchMatch => 5,
            Self::Default => 6,
        }
    }
    pub fn string_vec() -> Vec<String> {
        vec![
            "IamFounder".to_string(),
            "FounderIs".to_string(),
            "SwarmName".to_string(),
            "CatalogApp".to_string(),
            "ForumApp".to_string(),
            "SearchMatch".to_string(),
            "Default".to_string(),
        ]
    }
    pub fn update(&self, gid_opt: Option<GnomeId>, n_opt: Option<String>) -> Self {
        match self {
            Self::FounderIs(_old_gid) => {
                if let Some(new_gid) = gid_opt {
                    Self::FounderIs(new_gid)
                } else {
                    Self::FounderIs(*_old_gid)
                }
            }
            Self::SwarmName(old_name) => {
                if let Some(new_gid) = gid_opt {
                    if let Some(new_name) = n_opt {
                        let sn = SwarmName::new(new_gid, new_name).unwrap();
                        Self::SwarmName(sn)
                    } else {
                        let sn = SwarmName::new(new_gid, old_name.name.clone()).unwrap();
                        Self::SwarmName(sn)
                    }
                } else {
                    if let Some(new_name) = n_opt {
                        let sn = SwarmName::new(old_name.founder, new_name).unwrap();
                        Self::SwarmName(sn)
                    } else {
                        Self::SwarmName(old_name.clone())
                    }
                }
            }
            other => (*other).clone(),
        }
    }
    pub fn is_met(
        &self,
        my_id: GnomeId,
        app_type: Option<AppType>,
        s_name: &SwarmName,
        is_any_content_marked_by_search_engine: bool,
    ) -> bool {
        match self {
            StorageCondition::IamFounder => my_id.0 == s_name.founder.0,
            StorageCondition::FounderIs(g_id) => g_id.0 == s_name.founder.0,
            StorageCondition::SwarmName(s_n) => {
                s_n.founder.0 == s_name.founder.0 && s_n.name == s_name.name
            }
            StorageCondition::CatalogApp => {
                if let Some(a_t) = app_type {
                    a_t.is_catalog()
                } else {
                    false
                }
            }
            StorageCondition::ForumApp => {
                if let Some(a_t) = app_type {
                    a_t.is_forum()
                } else {
                    false
                }
            }
            StorageCondition::SearchMatch => is_any_content_marked_by_search_engine,
            StorageCondition::Default => true,
        }
    }
}

#[derive(Clone, Debug)]
pub enum StoragePolicy {
    All,
    Datastore,
    Manifest,
    FirstPages,
    MatchOrFirstPages,
    MatchOrForget,
    MatchAndManifestOrFirstPages,
    MatchAndManifestOrForget,
    Forget,
}
impl StoragePolicy {
    pub fn get(id: usize) -> Self {
        match id {
            0 => Self::All,
            1 => Self::Datastore,
            2 => Self::Manifest,
            3 => Self::FirstPages,
            4 => Self::MatchOrFirstPages,
            5 => Self::MatchOrForget,
            6 => Self::MatchAndManifestOrFirstPages,
            7 => Self::MatchAndManifestOrForget,
            _o => Self::Forget,
        }
    }
    pub fn get_string(&self) -> String {
        match self {
            Self::All => "All".to_string(),
            Self::Datastore => "Datastore".to_string(),
            Self::Manifest => "Manifest".to_string(),
            Self::FirstPages => "FirstPages".to_string(),
            Self::MatchOrFirstPages => "MatchOrFirstPages".to_string(),
            Self::MatchOrForget => "MatchOrForget".to_string(),
            Self::MatchAndManifestOrFirstPages => "MatchAndManifestOrFirstPages".to_string(),
            Self::MatchAndManifestOrForget => "MatchAndManifestOrForget".to_string(),
            Self::Forget => "Forget".to_string(),
        }
    }
    pub fn get_id(&self) -> usize {
        match self {
            Self::All => 0,
            Self::Datastore => 1,
            Self::Manifest => 2,
            Self::FirstPages => 3,
            Self::MatchOrFirstPages => 4,
            Self::MatchOrForget => 5,
            Self::MatchAndManifestOrFirstPages => 6,
            Self::MatchAndManifestOrForget => 7,
            Self::Forget => 8,
        }
    }
    pub fn string_vec() -> Vec<String> {
        vec![
            "All".to_string(),
            "Datastore".to_string(),
            "Manifest".to_string(),
            "FirstPages".to_string(),
            "MatchOrFirstPages".to_string(),
            "MatchOrForget".to_string(),
            "MatchAndManifestOrFirstPages".to_string(),
            "MatchAndManifestOrForget".to_string(),
            "Forget".to_string(),
        ]
    }
    pub fn is_a_match_policy(&self) -> bool {
        match self {
            Self::MatchOrFirstPages => true,
            Self::MatchOrForget => true,
            Self::MatchAndManifestOrFirstPages => true,
            Self::MatchAndManifestOrForget => true,
            _other => false,
        }
    }
}

async fn parse_datastore_file(
    file_path: PathBuf,
    temp_store: &mut HashMap<u16, (DataType, u64)>,
    root_hash: &mut u64,
) -> u16 {
    if !file_path.exists() {
        let _ = File::create(file_path.clone()).await;
    }
    let mut file = BufReader::new(File::open(file_path).await.unwrap());
    let mut highest_inserted_id = 0;

    let mut buffer = [0; 19];
    // let read_result = file.read(&mut buffer).await;
    while let Ok(count) = file.read(&mut buffer).await {
        if count == 0 {
            break;
        }
        let i0 = buffer[0];
        let i1 = buffer[1];
        let dtype = buffer[2];
        let id = u16::from_be_bytes([i0, i1]);
        let hash = u64::from_be_bytes([
            buffer[3], buffer[4], buffer[5], buffer[6], buffer[7], buffer[8], buffer[9], buffer[10],
        ]);
        *root_hash = u64::from_be_bytes([
            buffer[11], buffer[12], buffer[13], buffer[14], buffer[15], buffer[16], buffer[17],
            buffer[18],
        ]);
        // eprintln!("temp insert CID-{} hash: {}", id, hash);
        temp_store.insert(id, (DataType::from(dtype), hash));
        if id > highest_inserted_id {
            highest_inserted_id = id;
        }
        // eprintln!("Read from file: {}, {}", id, hash);
    }
    highest_inserted_id
}

pub async fn read_datastore_from_disk(
    storage: PathBuf,
    autosave: bool,
    policy: StoragePolicy,
    // to_app_data: Sender<ToAppData>
) -> ApplicationData {
    let file_path = storage.join("datastore.sync");

    eprintln!("Reading Datastore from {:?}…", file_path);
    // TODO: here we read all the contents of given file and process it line-by-line.
    // Only when done we send response back and finish task.
    let mut temp_store = HashMap::new();
    let mut root_hash = 0;
    let highest_inserted_id =
        parse_datastore_file(file_path.clone(), &mut temp_store, &mut root_hash).await;

    let heap_auto_forward = false;
    let mut app_data =
        ApplicationData::empty(storage, autosave, (policy, vec![]), heap_auto_forward);
    for i in 0..=highest_inserted_id {
        if let Some((dtype, hash)) = temp_store.remove(&i) {
            eprintln!("Disk read CID-{} with hash: {}", i, hash);
            let content =
            // = if dtype == DataType::Link {
            //     Content::Link(
            //         SwarmName {
            //             founder: GnomeId::any(),
            //             name: String::new(),
            //         },
            //         0,
            //         Description::new(String::new()).unwrap(),
            //         Data::empty(hash),
            //         None,
            //     )
            // } else {
            // 
            // We do not distinguish between Link and Data at this point
                // let ctree = ContentTree::empty(hash);
                Content::Data(dtype, 0, ContentTree::empty(hash));
            // };
            // eprintln!("RH: {}", app_data.root_hash());
            if let Some(next_cid) = app_data.next_c_id() {
                if next_cid == i {
                    let _res = app_data.append(content);
                    // eprintln!("Append res: {:?}", _res);
                } else {
                    let _res = app_data.update(i, content);
                    // eprintln!("Update res: {:?}", _res);
                }
            }
        } else {
            eprintln!("Error reading Application Data {}", i);
            break;
        }
    }
    app_data.set_disk_hash();
    // eprintln!(
    //     "Loaded from file: {}, expected: {}",
    //     app_data.root_hash(),
    //     root_hash
    // );
    app_data
}

pub fn should_store_content_on_disk(
    policy: &(StoragePolicy, Vec<ContentID>),
    c_id: ContentID,
) -> (bool, u16) {
    // TODO: rewrite this logic
    let default_p_count = if c_id == 0 { 64 } else { 0 };
    match policy.0 {
        StoragePolicy::Forget => (false, 0),
        StoragePolicy::Datastore => {
            if c_id == 0 {
                (true, default_p_count)
            } else {
                (false, default_p_count)
            }
        }
        StoragePolicy::Manifest => {
            if c_id == 0 {
                (true, u16::MAX)
            } else {
                (false, 0)
            }
        }
        StoragePolicy::FirstPages => (true, default_p_count),
        // StoragePolicy::SelectMainPages(c_ids) => (c_ids.contains(&c_id), default_p_count),
        // StoragePolicy::SelectedContents(store_main_pages, c_ids) => {
        //     if c_ids.contains(&c_id) {
        //         (true, u16::MAX)
        //     } else if *store_main_pages {
        //         (true, default_p_count)
        //     } else if c_id == 0 {
        //         (true, default_p_count)
        //     } else {
        //         (false, default_p_count)
        //     }
        // }
        StoragePolicy::MatchOrFirstPages => {
            if policy.1.contains(&c_id) {
                (true, u16::MAX)
            } else {
                (true, 0)
            }
        }
        StoragePolicy::MatchOrForget => {
            if policy.1.contains(&c_id) {
                (true, u16::MAX)
            } else {
                (false, 0)
            }
        }
        StoragePolicy::MatchAndManifestOrFirstPages => {
            if c_id == 0 || policy.1.contains(&c_id) {
                (true, u16::MAX)
            } else {
                (true, 0)
            }
        }
        StoragePolicy::MatchAndManifestOrForget => {
            if c_id == 0 || policy.1.contains(&c_id) {
                (true, u16::MAX)
            } else {
                (false, 0)
            }
        }
        StoragePolicy::All => (true, u16::MAX),
    }
}

pub async fn store_data_on_disk(s_storage: PathBuf, mut app_data: ApplicationData) {
    if matches!(app_data.policy.0, StoragePolicy::Forget) {
        eprintln!("STORAGE: Not writing to disk: Discard Policy");
        return;
    }
    if app_data.disk_root_hash == app_data.root_hash() {
        eprintln!("STORAGE: Not writing to disk: all synced");
        return;
    }
    let dsync_store = s_storage.join("datastore.sync");
    let last_defined_c_id = if let Some(next_c_id) = app_data.next_c_id() {
        next_c_id - 1
    } else {
        u16::MAX
    };
    let content_changed = write_datastore_to_disk(dsync_store, &app_data).await;
    if !content_changed {
        return;
    }
    let (should_store, max_page) = should_store_content_on_disk(&app_data.policy, 0);
    let mut first_pages_to_store = vec![];
    if should_store {
        let write_fresh_file = false;
        if let Some(first_page) = store_content_on_disk(
            0,
            &s_storage,
            &app_data.contents.take(0).unwrap(),
            max_page,
            write_fresh_file,
        )
        .await
        {
            first_pages_to_store.push((0, first_page));
        }
    }

    if matches!(app_data.policy.0, StoragePolicy::Datastore) {
        return;
    }
    for c_id in 1..=last_defined_c_id {
        let (should_store, max_page) = should_store_content_on_disk(&app_data.policy, c_id);
        if should_store {
            if let Some(first_page) = store_content_on_disk(
                c_id,
                &s_storage,
                &app_data.contents.take(c_id).unwrap(),
                max_page,
                false,
            )
            .await
            {
                first_pages_to_store.push((c_id, first_page));
            }
        }
    }
    if !first_pages_to_store.is_empty() {
        store_first_pages_on_disk(first_pages_to_store, &app_data.storage).await;
    }
    eprintln!("STORAGE: Done writing Contents to Disk");
    // if matches!(policy, StoragePolicy::Datastore) {
    //     // Do nothing more
    // } else if let StoragePolicy::SelectMainPages(selection) = policy {
    //     // Right now we did not implement main_pages.dat file that stores only
    //     // main pages of contents.
    //     // So for now we will store each in different cid.dat file
    //     for c_id in selection {
    //         store_content_on_disk(c_id, &s_storage, &app_data.contents.take(c_id).unwrap(), 0)
    //             .await;
    //     }
    //     return;
    // } else if let StoragePolicy::MainPages = policy {
    //     for c_id in 1..=last_defined_c_id {
    //         store_content_on_disk(c_id, &s_storage, &app_data.contents.take(c_id).unwrap(), 0)
    //             .await;
    //     }
    // } else if let StoragePolicy::SelectedContents(store_main_pages, selection) = policy {
    //     if store_main_pages {
    //         for c_id in 0..=last_defined_c_id {
    //             if selection.contains(&c_id) {
    //                 store_content_on_disk(
    //                     c_id,
    //                     &s_storage,
    //                     &app_data.contents.take(c_id).unwrap(),
    //                     u16::MAX,
    //                 )
    //                 .await;
    //             } else {
    //                 store_content_on_disk(
    //                     c_id,
    //                     &s_storage,
    //                     &app_data.contents.take(c_id).unwrap(),
    //                     0,
    //                 )
    //                 .await;
    //             }
    //         }
    //     } else {
    //         for c_id in selection {
    //             store_content_on_disk(
    //                 c_id,
    //                 &s_storage,
    //                 &app_data.contents.take(c_id).unwrap(),
    //                 u16::MAX,
    //             )
    //             .await;
    //         }
    //     }
    // } else if let StoragePolicy::Everything = policy {
    //     for c_id in 0..=last_defined_c_id {
    //         store_content_on_disk(
    //             c_id,
    //             &s_storage,
    //             &app_data.contents.take(c_id).unwrap(),
    //             u16::MAX,
    //         )
    //         .await;
    //     }
    // } else {
    //     eprintln!("This is not expected to happen");
    // }
    // TODO: build logic to update file contents
    // If we have data in memory we can decide to only store data hashes, or Pages
    // Once we've decided we read existing on-disk data and only store missing parts
    // First we read existing file header into memory, from that we construct a shell
    // of a content and then compare it's root hash with what we have in memory.
    // If mem_root_hash is different we append to .dat each page whose hash in memory
    // is different from that on disk.
    // If we do not have actual contents of given page we mark that by setting
    // Offset=0 and Size=0 - this way we know we are missing a Page data, but we know it's hash.
    // hdr file format:
    // PID(2B)    PageHash(8B)    Offset(4B)    Size(2B)
    //
    // TODO: In future we want to store how many "dead bytes" we are storing od disk.
    // When some threshold is exceeded, we write both files from scratch to save disk space
}
pub async fn store_content_on_disk(
    c_id: ContentID,
    s_storage: &Path,
    content: &Content,
    break_on_page: u16,
    // first_page_on_disk: u64,// read this frox c_id.hdr file
    write_to_fresh_file: bool,
) -> Option<Data> {
    eprintln!("in store_content_on_disk");
    // TODO: here we should define logic to determine
    // if we should store first page of c_id in heads.dat.
    //
    // Since we can store each Content separately, we can not use a HashMap<CID,Data> to store
    // header files, we need a different approach.
    //
    // TODO: New approach: c_id.hdr file will still hold empty pointer to first page,
    // just to indicate what hash that first page has.
    // This entry can be used to determine if first page has changed.
    // If it has, we update the pointer in c_id.hdr file
    // and return first page as Some(Data) from this fn.
    //
    // TODO: Caller of this fn should take care of updating heads.hdr and heads.dat files.
    //
    // introduce a write_to_fresh_file: bool option?
    // This way we can force writing to a file in orderly fashion even if none of pages has changed.
    //
    let dtype = content.data_type();
    let mut buff_header: [u8; 16] = [0; 16];
    // eprintln!("CID-{} hash {}", c_id, rhash);
    // Load existing file contents into memory
    let header_file = s_storage.join(format!("{}.hdr", c_id));
    // if !header_file.exists() {
    //     let _ = File::create(header_file.clone()).await;
    // }
    let data_file = s_storage.join(format!("{}.dat", c_id));
    // if !data_file.exists() {
    //     let _ = File::create(data_file.clone()).await;
    // }
    let mut temp_storage = HashMap::new();
    let mut return_opt = None;
    let mut byte_pointer: u32 = 0;
    let load_from_header_opt = if write_to_fresh_file {
        None
    } else {
        load_content_from_header_file(
            header_file.clone(),
            dtype,
            &mut temp_storage,
            &mut byte_pointer,
        )
        .await
    };
    if load_from_header_opt.is_some() {
        eprintln!("Header file for CID-{} read", c_id);
        // Calculate it's root hash
        // if file_content.hash() != rhash {
        // Only if hashes are different append pages that differ
        // eprintln!(
        //     "CID-{} on disk {} differs from {} in memory\n(file: {:?})",
        //     c_id,
        //     file_content.hash(),
        //     rhash,
        //     header_file,
        // );
        // eprintln!("on disk page hashes: {:?}", file_content.data_hashes());
        // eprintln!(
        //     "in memr page hashes: {:?}",
        //     app_data.content_bottom_hashes(c_id).unwrap()
        // );
        let header_file = OpenOptions::new()
            .write(true)
            .append(true)
            .open(header_file)
            .await
            .unwrap();
        let mut header_file = BufWriter::new(header_file);
        let data_file = OpenOptions::new()
            .write(true)
            .append(true)
            .open(data_file)
            .await
            .unwrap();
        let mut data_file = BufWriter::new(data_file);
        // let mem_data_hashes = content.content_bottom_hashes(c_id).unwrap();
        let mem_data_hashes = content.data_hashes();
        for (i, mem_hash) in mem_data_hashes.into_iter().enumerate() {
            // if i == 0 && mem_hash != first_page_on_disk {
            //     // call specialized fn
            //     let data = content.read_data(0).unwrap();
            //     store_first_pages_on_disk(vec![(c_id, data)], s_storage).await;
            //     continue;
            // }
            if let Some((hash, _offset, _size)) = temp_storage.get(&(i as u16)) {
                eprintln!("Header for D-{i} h: {hash}, off: {_offset} size: {_size}");
                if *hash != mem_hash || *_size == 0 {
                    eprintln!("PgID-{} Disk: {}, mem: {} ", i, hash, mem_hash);
                    //TODO: send updated contents to disk
                    // hdr file format:
                    // PID(2B)    PageHash(8B)    Offset(4B)    Size(2B)
                    // let [d0, d1] = (i as u16).to_be_bytes();
                    let read_result = content.read_data(i as u16);
                    if read_result.is_err() {
                        eprintln!(
                            "Failed to read data for CID-{}/{}:\n{:?}",
                            c_id, i, read_result
                        );
                        continue;
                    }
                    let data = read_result.unwrap();
                    // buff_header[0] = d0;
                    // buff_header[1] = d1;
                    // let mut i = 2;
                    // for byte in mem_hash.to_be_bytes() {
                    //     buff_header[i] = byte;
                    //     i += 1;
                    // }
                    if i == 0 {
                        // Here, we only update CID.hdr file for first page
                        write_data_to_disk(
                            0,
                            Data::empty(data.get_hash()),
                            &mut byte_pointer,
                            &mut buff_header,
                            &mut header_file,
                            &mut data_file,
                        )
                        .await;
                        // let _ = header_file.write(&buff_header).await;
                        return_opt = Some(data);
                        continue;
                    }
                    write_data_to_disk(
                        i as u16,
                        data,
                        &mut byte_pointer,
                        &mut buff_header,
                        &mut header_file,
                        &mut data_file,
                    )
                    .await;
                    // if data.is_empty() {
                    //     // eprintln!("Data is empty, not writing anything.");
                    //     //Only write hdr file
                    //     for i in 10..16 {
                    //         buff_header[i] = 0;
                    //     }
                    //     let _ = header_file.write(&buff_header).await;
                    // } else {
                    //     eprintln!("Data with bytes");
                    //     let mut i = 10;
                    //     for byte in byte_pointer.to_be_bytes() {
                    //         buff_header[i] = byte;
                    //         i += 1;
                    //     }
                    //     let data_len = data.len() as u32;
                    //     for byte in (data_len as u16).to_be_bytes() {
                    //         buff_header[i] = byte;
                    //         i += 1;
                    //     }
                    //     let _ = header_file.write(&buff_header).await;
                    //     let _ = data_file.write(&data.bytes()).await;
                    //     byte_pointer += data_len;
                    // }
                }
            } else {
                // TODO: too much copy-pasting!!!
                let data = content.read_data(i as u16).unwrap();
                if i == 0 {
                    write_data_to_disk(
                        0,
                        Data::empty(data.get_hash()),
                        &mut byte_pointer,
                        &mut buff_header,
                        &mut header_file,
                        &mut data_file,
                    )
                    .await;
                    // let _ = header_file.write(&buff_header).await;
                    return_opt = Some(data);
                    continue;
                }
                write_data_to_disk(
                    i as u16,
                    data,
                    &mut byte_pointer,
                    &mut buff_header,
                    &mut header_file,
                    &mut data_file,
                )
                .await;
                // if i == 0 {
                //     return_opt = Some(data);
                //     continue;
                // }
                // let [d0, d1] = (i as u16).to_be_bytes();
                // buff_header[0] = d0;
                // buff_header[1] = d1;
                // let mut i = 2;
                // for byte in mem_hash.to_be_bytes() {
                //     buff_header[i] = byte;
                //     i += 1;
                // }
                // if data.is_empty() {
                //     //Only write hdr file
                //     for i in 10..16 {
                //         buff_header[i] = 0;
                //     }
                //     let _ = header_file.write(&buff_header).await;
                // } else {
                //     let mut i = 10;
                //     for byte in byte_pointer.to_be_bytes() {
                //         buff_header[i] = byte;
                //         i += 1;
                //     }
                //     let data_len = data.len() as u32;
                //     for byte in (data_len as u16).to_be_bytes() {
                //         buff_header[i] = byte;
                //         i += 1;
                //     }
                //     let _ = header_file.write(&buff_header).await;
                //     let _ = data_file.write(&data.bytes()).await;
                //     byte_pointer += data_len;
                // }
            }
            if i as u16 >= break_on_page {
                break;
            }
        }
        let _ = header_file.flush().await;
        let _ = data_file.flush().await;
        // TODO: we need to update what has changed into disk
        // }
    } else {
        eprintln!("Creating new header and data for CID-{}", c_id);
        // eprintln!("H: {:?}", header_file);
        // eprintln!("D: {:?}", data_file);
        if !s_storage.exists() {
            let _ = fs::create_dir(s_storage).await;
        }
        let mut header_file = BufWriter::new(File::create(header_file).await.unwrap());
        let mut data_file = BufWriter::new(File::create(data_file).await.unwrap());
        let mut byte_pointer: u32 = 0;
        if let Ok(data) = content.read_data(0) {
            // buff_header is all zeroes now, so we only have to update hash
            write_data_to_disk(
                0,
                Data::empty(data.get_hash()),
                &mut byte_pointer,
                &mut buff_header,
                &mut header_file,
                &mut data_file,
            )
            .await;
            // let mut i = 2;
            // for byte in data.get_hash().to_be_bytes() {
            //     buff_header[i] = byte;
            //     i += 1;
            // }
            // let _ = header_file.write(&buff_header).await;
            return_opt = Some(data);
        }
        let mut data_id = 1;
        // TODO: write both hdr & dat from scratch
        while let Ok(data) = content.read_data(data_id) {
            write_data_to_disk(
                0,
                data,
                &mut byte_pointer,
                &mut buff_header,
                &mut header_file,
                &mut data_file,
            )
            .await;
            // let [d0, d1] = data_id.to_be_bytes();
            // buff_header[0] = d0;
            // buff_header[1] = d1;
            // let mut i = 2;
            // for byte in data.get_hash().to_be_bytes() {
            //     buff_header[i] = byte;
            //     i += 1;
            // }
            // if data.is_empty() {
            //     //Only write hdr file
            //     eprintln!("CID{c_id} D{data_id} is empty!");
            //     for i in 10..16 {
            //         buff_header[i] = 0;
            //     }
            //     let _ = header_file.write(&buff_header).await;
            // } else {
            //     let mut i = 10;
            //     for byte in byte_pointer.to_be_bytes() {
            //         buff_header[i] = byte;
            //         i += 1;
            //     }
            //     let data_len = data.len() as u32;
            //     for byte in (data_len as u16).to_be_bytes() {
            //         buff_header[i] = byte;
            //         i += 1;
            //     }
            //     let _ = header_file.write(&buff_header).await;
            //     let _r = data_file.write(&data.bytes()).await;
            //     eprintln!("DID {data_id} Data file res: {_r:?}");
            //     byte_pointer += data_len;
            // }
            if data_id >= break_on_page {
                break;
            }
            data_id += 1;
        }

        let _ = header_file.flush().await;
        let _ = data_file.flush().await;
    }
    eprintln!("store_content_on_disk ret: {:?}", return_opt);
    return_opt
    // } else {
    //     // eprintln!("Unable to read root hash for {}, breaking", c_id);
    //     break;
    // }
}

async fn write_data_to_disk(
    data_id: u16,
    data: Data,
    byte_pointer: &mut u32,
    buff_header: &mut [u8; 16],
    header_file: &mut BufWriter<File>,
    data_file: &mut BufWriter<File>,
) {
    eprintln!("in write_data_to_disk id:{}, dlen:{}", data_id, data.len());
    eprintln!("header: {:?}", header_file);
    eprintln!("data: {:?}", data_file);
    // TODO
    let [d0, d1] = (data_id).to_be_bytes();
    buff_header[0] = d0;
    buff_header[1] = d1;
    let mut i = 2;
    for byte in data.get_hash().to_be_bytes() {
        buff_header[i] = byte;
        i += 1;
    }
    if data.is_empty() {
        eprintln!("Data is empty, only writing header.");
        //Only write hdr file
        for i in 10..16 {
            buff_header[i] = 0;
        }
        let _ = header_file.write(buff_header).await;
    } else {
        eprintln!("Data with bytes");
        let mut i = 10;
        for byte in byte_pointer.to_be_bytes() {
            buff_header[i] = byte;
            i += 1;
        }
        let data_len = data.len() as u32;
        for byte in (data_len as u16).to_be_bytes() {
            buff_header[i] = byte;
            i += 1;
        }
        let _res1 = header_file.write(buff_header).await;
        eprintln!("header write: {:?}", _res1);
        let _res2 = data_file.write(&data.bytes()).await;
        eprintln!("data write: {:?}", _res2);
        *byte_pointer += data_len;
    }
}
pub async fn store_first_pages_on_disk(first_pages: Vec<(ContentID, Data)>, s_storage: &Path) {
    eprintln!("in store first pages on disk");
    let heads_header_file = s_storage.join(format!("heads.hdr"));
    let heads_data_file = s_storage.join(format!("heads.dat"));
    if !heads_data_file.exists() {
        eprintln!("heads.dat does not exist, creating");
        let _fc_res = File::create(heads_data_file.clone()).await;
    }
    // TODO: first we load headers from heads.hdr
    // Logic is very similar to that of store_content_on_disk
    let mut temp_storage = HashMap::with_capacity(1024);
    let mut byte_pointer = 0;
    if !heads_header_file.exists() {
        eprintln!("heads.hdr does not exist, creating");
        let _fc_res = File::create(heads_header_file.clone()).await;
    } else {
        let _head_load_res = load_content_from_header_file(
            heads_header_file.clone(),
            DataType::Data(0),
            &mut temp_storage,
            &mut byte_pointer,
        )
        .await;
        eprintln!("Load heads from header: {}", _head_load_res.is_some());
    }

    // now we can decide, one-by-ony if given Data should be stored.
    // If so, we update heads.hdr, and append to heads.dat.
    // TODO: byte pointer should be set to proper position!
    let mut buff_header: [u8; 16] = [0; 16];
    let header_file = OpenOptions::new()
        .write(true)
        .append(true)
        .open(heads_header_file)
        .await
        .unwrap();
    let mut header_file = BufWriter::new(header_file);

    let data_file = OpenOptions::new()
        .write(true)
        .append(true)
        .open(heads_data_file)
        .await
        .unwrap();
    let mut data_file = BufWriter::new(data_file);

    for (cid, data) in first_pages {
        eprintln!("save 1st page for {}, len: {}", cid, data.len());
        //  write conditionally
        if let Some((disk_hash, _pos, _siz)) = temp_storage.get(&cid) {
            if *disk_hash != data.get_hash() {
                eprintln!("diff hashes");
                write_data_to_disk(
                    cid,
                    data,
                    &mut byte_pointer,
                    &mut buff_header,
                    &mut header_file,
                    &mut data_file,
                )
                .await;
            }
        } else {
            eprintln!("nothing was there");
            write_data_to_disk(
                cid,
                data,
                &mut byte_pointer,
                &mut buff_header,
                &mut header_file,
                &mut data_file,
            )
            .await;
        }
    }
    let _ = header_file.flush().await;
    let _ = data_file.flush().await;
}

pub async fn load_first_pages_from_disk(s_storage: &Path) -> HashMap<ContentID, Data> {
    let heads_header_file = s_storage.join(format!("heads.hdr"));
    let data_file = s_storage.join(format!("heads.dat"));
    let mut temp_storage = HashMap::with_capacity(1024);
    let mut byte_pointer = 0;

    // First we load headers from heads.hdr
    if heads_header_file.exists() {
        let _head_load_res = load_content_from_header_file(
            heads_header_file.clone(),
            DataType::Data(0),
            &mut temp_storage,
            &mut byte_pointer,
        )
        .await;
        eprintln!("Load heads from header: {}", _head_load_res.is_some());
    } else {
        return HashMap::new();
    }
    if !data_file.exists() {
        return HashMap::new();
    }

    // Now we have a HashMap<CID,(u64,seek,size)> and can iterate over them one by one.
    let mut buffer: [u8; 1024] = [0; 1024];
    eprintln!("Reading: {data_file:?}");
    let mut file = BufReader::new(File::open(data_file).await.unwrap());
    let mut results = HashMap::with_capacity(temp_storage.len());
    for i in 0..=u16::MAX {
        if let Some((hash, pointer, size)) = temp_storage.remove(&i) {
            if size == 0 {
                results.insert(i, Data::empty(hash));
                continue;
            }
            let size = size as usize;
            let _sr = file.seek(std::io::SeekFrom::Start(pointer as u64)).await;
            if _sr.is_err() {
                eprintln!("Failed to Seek file: {:?}", _sr.err().unwrap());
                return results;
            }
            let _re = file.read(&mut buffer).await;
            if let Ok(count) = _re {
                if count >= size {
                    let v = Vec::from(&buffer[..size]);
                    let data = Data::new(v).unwrap();
                    let d_hash = data.get_hash();
                    if d_hash == hash {
                        results.insert(i, data);
                    } else {
                        eprintln!("CID-{i} Page hash mismatch(data: {d_hash}, expected: {hash})",);
                        return results;
                    }
                } else {
                    eprintln!("Did not read enough bytes (req: {}), read: {}", size, count);
                    return results;
                }
            } else {
                eprintln!("Failed to read file: {:?}", _re.err().unwrap());
                return results;
            }
        } else {
            break;
        }
    }
    results
}

async fn load_link_from_disk(
    cid: u16,
    hash: u64,
    first_pages: &HashMap<ContentID, Data>,
) -> Option<Content> {
    // TODO: convert also remaining
    // TODO: with heads.hdr & heads.dat this will no longer work!
    // For links we probably need a dedicated fn to read from heads files
    // let link_data = c_tree.read(0).unwrap();
    // Some(data_to_link(link_data).unwrap())
    // eprintln!("For loading DataType::Link use load_link_from_disk");
    return if let Some(link_data) = first_pages.get(&cid) {
        if hash != link_data.get_hash() {
            eprintln!("Hash mismatch, when reading a Link");
        }
        Some(data_to_link(link_data.clone()).unwrap())
    } else {
        None
    };
}

pub async fn load_content_from_disk(
    s_storage: PathBuf,
    cid: u16,
    dtype: DataType,
    hash: u64,
    first_pages: &HashMap<ContentID, Data>,
) -> Option<Content> {
    if dtype.is_link() {
        // let mut first_pages = load_first_pages_from_disk(&s_storage).await;
        // return if let Some(link_data) = first_pages.remove(&cid) {
        //     if hash != link_data.get_hash() {
        //         eprintln!("Hash mismatch, when reading a Link");
        //     }
        //     Some(data_to_link(link_data).unwrap())
        // } else {
        // return None;
        // };
        return load_link_from_disk(cid, hash, first_pages).await;
    }
    let header_file = s_storage.join(format!("{}.hdr", cid));
    let data_file = s_storage.join(format!("{}.dat", cid));
    // eprintln!("Load content from disk: {:?} {}", s_storage, cid);
    let mut temp_storage = HashMap::new();
    let mut byte_pointer: u32 = 0;
    if let Some(content_on_disk) = load_content_from_header_file(
        header_file.clone(),
        dtype,
        &mut temp_storage,
        &mut byte_pointer,
    )
    .await
    {
        // eprintln!(
        //     "Loaded CID-{} from hdr, {:?}",
        //     cid,
        //     content_on_disk.data_hashes()
        // );
        //TODO: all matches
        //TODO: read each Page from data_file
        let dhash = content_on_disk.hash();
        if dhash == hash {
            let mut c_tree = ContentTree::empty(0);
            let mut mem_size = 0;
            let mut buffer: [u8; 1024] = [0; 1024];
            // eprintln!("2Reading: {data_file:?}");
            let mut file = BufReader::new(File::open(data_file).await.unwrap());
            for i in 0..=u16::MAX {
                if let Some((hash, pointer, size)) = temp_storage.remove(&i) {
                    if size == 0 {
                        // eprintln!("Size is zero for {i}");
                        if i == 0 {
                            // eprintln!("but first page");
                            if let Some(first_page) = first_pages.get(&cid) {
                                // eprintln!("and we have it!");
                                if hash == first_page.get_hash() {
                                    let _res = c_tree.append(first_page.clone());
                                    continue;
                                } else {
                                    // eprintln!(
                                    //     "but hash mismatch exp: {hash}, but: {}",
                                    //     first_page.get_hash()
                                    // );
                                    let _res = c_tree.append(Data::empty(hash));
                                    eprintln!("Append 0 for DID: {},res: {:?}", i, _res);
                                    continue;
                                }
                            } else {
                                eprintln!("But no first pages");
                            }
                        } else {
                            let _res = c_tree.append(Data::empty(hash));
                            eprintln!("Append 0 for DID: {},res: {:?}", i, _res);
                            continue;
                        }
                    }
                    let size = size as usize;
                    //TODO: maybe read entire file once and work on those bytes instead?
                    let _sr = file.seek(std::io::SeekFrom::Start(pointer as u64)).await;
                    if _sr.is_err() {
                        eprintln!("Failed to Seek file: {:?}", _sr.err().unwrap());
                        return None;
                    }
                    let _re = file.read(&mut buffer).await;
                    if let Ok(count) = _re {
                        if count >= size {
                            let v = Vec::from(&buffer[..size]);
                            let data = Data::new(v).unwrap();
                            let d_hash = data.get_hash();
                            if d_hash == hash {
                                let _ar = c_tree.append(data);
                                eprintln!("Append non-zero for DID: {},res: {:?}", i, _ar);
                                mem_size += 1;
                                // eprintln!("Append result: {:?}", _ar);
                            } else {
                                eprintln!(
                                    "CID-{} Page hash mismatch(data: {d_hash}, expected: {hash})",
                                    cid
                                );
                                return None;
                            }
                        } else {
                            eprintln!("Did not read enough bytes (req: {}), read: {}", size, count);
                            return None;
                        }
                    } else {
                        eprintln!("Failed to read file: {:?}", _re.err().unwrap());
                        return None;
                    }
                } else {
                    break;
                }
            }
            // eprintln!("Loaded content from disk: {:?} {}", s_storage, cid);
            // if dtype.is_link() {
            //     // TODO: convert also remaining
            //     // TODO: with heads.hdr & heads.dat this will no longer work!
            //     // For links we probably need a dedicated fn to read from heads files
            //     // let link_data = c_tree.read(0).unwrap();
            //     // Some(data_to_link(link_data).unwrap())
            //     eprintln!("Loading DataType::Link not supported");
            //     None
            // } else {
            Some(Content::Data(dtype, mem_size, c_tree))
            // }
        } else {
            eprintln!("Content from hdr file hash {} mismatch {}", dhash, hash,);
            None
        }
    } else {
        eprintln!("Unable to load content from {:?} file", header_file);
        None
    }
}

async fn load_content_from_header_file(
    file_path: PathBuf,
    dtype: DataType,
    temp_storage: &mut HashMap<u16, (u64, u32, u16)>,
    byte_pointer: &mut u32,
) -> Option<Content> {
    if !file_path.exists() {
        eprintln!("File {:?} does not exist!", file_path);
        return None;
    }
    eprintln!("Reading {:?} file", file_path);
    // hdr file format:
    // PID(2B)    PageHash(8B)    Offset(4B)    Size(2B)
    let mut buffer: [u8; 16] = [0; 16];
    let mut file = BufReader::new(File::open(file_path).await.unwrap());
    while let Ok(count) = file.read(&mut buffer).await {
        if count == 0 {
            break;
        }
        let page_id = u16::from_be_bytes([buffer[0], buffer[1]]);
        let page_hash = u64::from_be_bytes([
            buffer[2], buffer[3], buffer[4], buffer[5], buffer[6], buffer[7], buffer[8], buffer[9],
        ]);
        let offset = u32::from_be_bytes([buffer[10], buffer[11], buffer[12], buffer[13]]);
        let page_size = u16::from_be_bytes([buffer[14], buffer[15]]);
        let new_pointer = offset + (page_size as u32);
        if new_pointer > *byte_pointer {
            *byte_pointer = new_pointer;
        }
        eprintln!(
            "Insert {} - {}, off: {}, size: {}",
            page_id, page_hash, offset, page_size
        );
        temp_storage.insert(page_id, (page_hash, offset, page_size));
    }
    // eprintln!("Read {} entries", temp_storage.len());
    if let Some((hash, _offset, _size)) = temp_storage.get(&0) {
        eprintln!("Has root entry!");
        // let mut content = Content::from(dtype, Data::empty(*hash)).unwrap();
        let mut content = Content::Data(dtype, 0, ContentTree::Empty(*hash));
        let mut page_id = 1;
        while let Some((hash, _offset, _size)) = temp_storage.get(&page_id) {
            let _ = content.push_data(Data::empty(*hash));
            page_id += 1;
        }
        Some(content)
    } else {
        eprintln!("No root entry!");
        None
    }
}

pub async fn write_datastore_to_disk(file_path: PathBuf, app_data: &ApplicationData) -> bool {
    //
    // TODO: We want to introduce a pair of files: heads.hdr heads.dat.
    //
    // Those files will store first pages only for all of given swarm's Contents.
    // None of those first pages should be stored in x.dat files.
    // This way if storage policy for given Swarm is to store FirstPagesOnly,
    // we only have to write to this pair of files.
    // Also when user issues a search request and given Swarm is to be searched,
    // but is not currently loaded into RAM, then we only have to read from those two
    // files (and maybe datastore) in order to satisfy users request.
    //
    // Now reading from disk should be faster, since for a preview we only need to load Data from
    // heads file-pair.
    // Only if we want to get into details of given CID should we read CID.hdr & CID.dat.
    //
    // Similarly if an app is created that is used for syncing existing files into a swarm,
    // then description of that file will be stored in heads.dat file, and entire, untouched file
    // will be stored under x.dat. This way if we want to open given file with external application,
    // such as Blender or Gimp or VLC, we simply copy that file,
    // eg. 'cp path/to/storage/GID-31f4be3eb94d15c9/2.dat ~/Pictures/earthHiRes.jpg'.
    //
    // TODO: Above might need some additional logic for cases when we change parts of a file,
    // and introduced fragmentation. Then a call to 'Rewrite to disk' function from UI
    // should trigger writing a fresh file in orderly fashion.
    let mut content_changed = false;
    let mut temp_store = HashMap::new();
    let mut root_hash = 0;
    let _ = parse_datastore_file(file_path.clone(), &mut temp_store, &mut root_hash).await;
    let root_hash_in_memory = app_data.root_hash();
    if root_hash_in_memory == root_hash {
        eprintln!("datastore.sync is up to date - {}", root_hash);
        return content_changed;
    }
    content_changed = true;
    eprintln!("Writing Datastore to {:?}…", file_path);
    let file = OpenOptions::new()
        .write(true)
        .append(true)
        .open(file_path)
        .await
        .unwrap();
    let mut file = BufWriter::new(file);

    // TODO: we need to build logic that decides what parts of existing file should be overwritten.
    // The simplest approach: read existing file contents and only write the difference
    let mut buffer: [u8; 19] = [0; 19];
    // let root_hash = app_data.root_hash();
    let mut i = 11;
    for byte in root_hash_in_memory.to_be_bytes() {
        buffer[i] = byte;
        i += 1;
    }
    for j in 0..=u16::MAX {
        // here we check if content root hash has changed,
        // and if so, we update datastore.sync file
        if let Ok((dt, crh)) = app_data.content_root_hash(j) {
            if let Some((disk_dt, disk_hash)) = temp_store.remove(&j) {
                if disk_dt == dt && disk_hash == crh {
                    eprintln!("Skipping CID-{} as it is already stored on disk", j);
                    continue;
                }
            }
            let [i0, i1] = j.to_be_bytes();
            buffer[0] = i0;
            buffer[1] = i1;
            buffer[2] = dt.byte();
            i = 3;
            for byte in crh.to_be_bytes() {
                buffer[i] = byte;
                i += 1;
            }
            let _ = file.write(&buffer).await;
            eprintln!("Write to file: {}, {}", j, crh);
        }
    }
    eprintln!("Root hash: {}", root_hash);

    let _ = file.flush().await;
    content_changed
    // Maybe we provide this function what ordered changes should be appended?
}
