use std::collections::HashMap;
use std::path::PathBuf;

// use async_std::channel::Sender;
use async_std::fs::{File, OpenOptions};
use async_std::io::prelude::SeekExt;
use async_std::io::{BufReader, BufWriter, ReadExt, WriteExt};
use gnome::prelude::{GnomeId, SwarmName};

use crate::content::{Content, ContentTree, DataType, Description};
use crate::{ApplicationData, Data};

// pub async fn test_write() {
//     let mut file = BufWriter::new(File::create("/home/dxtr/test.file").await.unwrap());

//     for i in 'a'..='z' {
//         let arr = [i as u8];
//         file.write(&arr).await;
//     }

//     file.flush().await;
// }
// pub async fn test_read() {
//     let mut file = BufReader::new(File::open("/home/dxtr/test.file").await.unwrap());

//     let mut buffer = [0; 1024];
//     let read_result = file.read(&mut buffer).await;
//     if let Ok(count) = read_result {
//         let text = String::from_utf8_lossy(&buffer[..count]);
//         eprintln!("Read from file: {}", text);
//     } else {
//         eprintln!("Error reading file: {:?}", read_result.err().unwrap());
//     }
// }

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
        temp_store.insert(id, (DataType::from(dtype), hash));
        if id > highest_inserted_id {
            highest_inserted_id = id;
        }
        // eprintln!("Read from file: {}, {}", id, hash);
    }
    highest_inserted_id
}
pub async fn read_datastore_from_disk(
    file_path: PathBuf,
    // to_app_data: Sender<ToAppData>
) -> ApplicationData {
    eprintln!("Reading Datastore from {:?}…", file_path);
    // TODO: here we read all the contents of given file and process it line-by-line.
    // Only when done we send response back and finish task.
    let mut temp_store = HashMap::new();
    let mut root_hash = 0;
    let highest_inserted_id =
        parse_datastore_file(file_path, &mut temp_store, &mut root_hash).await;

    let mut app_data = ApplicationData::new(crate::prelude::AppType::Catalog);
    for i in 0..=highest_inserted_id {
        if let Some((dtype, hash)) = temp_store.remove(&i) {
            let content = if dtype == DataType::Link {
                Content::Link(
                    SwarmName {
                        founder: GnomeId::any(),
                        name: String::new(),
                    },
                    0,
                    Description::new(String::new()).unwrap(),
                    Data::empty(hash),
                    None,
                )
            } else {
                let ctree = ContentTree::empty(hash);
                Content::Data(dtype, ctree)
            };
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
    // eprintln!(
    //     "Loaded from file: {}, expected: {}",
    //     app_data.root_hash(),
    //     root_hash
    // );
    app_data
}

pub async fn store_data_on_disk(s_storage: PathBuf, app_data: ApplicationData) {
    let dsync_store = s_storage.join("datastore.sync");
    write_datastore_to_disk(dsync_store, &app_data).await;
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
    let mut buff_header: [u8; 16] = [0; 16];
    for c_id in 0..=u16::MAX {
        // let [c0, c1] = c_id.to_be_bytes();
        if let Ok((dtype, rhash)) = app_data.content_root_hash(c_id) {
            // eprintln!("CID-{} hash {}", c_id, rhash);
            // Load existing file contents into memory
            let header_file = s_storage.join(format!("{}.hdr", c_id));
            let data_file = s_storage.join(format!("{}.dat", c_id));
            let mut temp_storage = HashMap::new();
            let mut byte_pointer: u32 = 0;
            if let Some(file_content) = load_content_from_header_file(
                header_file.clone(),
                dtype,
                &mut temp_storage,
                &mut byte_pointer,
            )
            .await
            {
                // eprintln!("Header file for CID-{} read", c_id);
                // Calculate it's root hash
                if file_content.hash() != rhash {
                    // Only if hashes are different append pages that differ
                    eprintln!(
                        "CID-{} on disk {} differs from {} in memory\n(file: {:?})",
                        c_id,
                        file_content.hash(),
                        rhash,
                        header_file,
                    );
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
                    let mem_data_hashes = app_data.content_bottom_hashes(c_id).unwrap();
                    for (i, mem_hash) in mem_data_hashes.into_iter().enumerate() {
                        if let Some((hash, _offset, _size)) = temp_storage.get(&(i as u16)) {
                            if *hash != mem_hash {
                                eprintln!("PID-{} Disk: {}, mem: {} ", i, hash, mem_hash);
                                //TODO: send updated contents to disk
                                // hdr file format:
                                // PID(2B)    PageHash(8B)    Offset(4B)    Size(2B)
                                let [d0, d1] = (i as u16).to_be_bytes();
                                let data = app_data.read_data(c_id, i as u16).unwrap();
                                buff_header[0] = d0;
                                buff_header[1] = d1;
                                let mut i = 2;
                                for byte in mem_hash.to_be_bytes() {
                                    buff_header[i] = byte;
                                    i += 1;
                                }
                                if data.is_empty() {
                                    //Only write hdr file
                                    for i in 10..16 {
                                        buff_header[i] = 0;
                                    }
                                    let _ = header_file.write(&mut buff_header).await;
                                } else {
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
                                    let _ = header_file.write(&mut buff_header).await;
                                    let _ = data_file.write(&mut data.bytes()).await;
                                    byte_pointer += data_len;
                                }
                            }
                        } else {
                            // TODO: too much copy-pasting!!!
                            let [d0, d1] = (i as u16).to_be_bytes();
                            let data = app_data.read_data(c_id, i as u16).unwrap();
                            buff_header[0] = d0;
                            buff_header[1] = d1;
                            let mut i = 2;
                            for byte in mem_hash.to_be_bytes() {
                                buff_header[i] = byte;
                                i += 1;
                            }
                            if data.is_empty() {
                                //Only write hdr file
                                for i in 10..16 {
                                    buff_header[i] = 0;
                                }
                                let _ = header_file.write(&mut buff_header).await;
                            } else {
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
                                let _ = header_file.write(&mut buff_header).await;
                                let _ = data_file.write(&mut data.bytes()).await;
                                byte_pointer += data_len;
                            }
                        }
                    }
                    let _ = header_file.flush().await;
                    let _ = data_file.flush().await;
                    // TODO: we need to update what has changed into disk
                }
            } else {
                eprintln!("Creating new header and data for CID-{}", c_id);
                let mut header_file = BufWriter::new(File::create(header_file).await.unwrap());
                let mut data_file = BufWriter::new(File::create(data_file).await.unwrap());
                let mut byte_pointer: u32 = 0;
                let mut data_id = 0;
                // TODO: write both hdr & dat from scratch
                while let Ok(data) = app_data.read_data(c_id, data_id) {
                    let [d0, d1] = data_id.to_be_bytes();
                    buff_header[0] = d0;
                    buff_header[1] = d1;
                    let mut i = 2;
                    for byte in data.get_hash().to_be_bytes() {
                        buff_header[i] = byte;
                        i += 1;
                    }
                    if data.is_empty() {
                        //Only write hdr file
                        for i in 10..16 {
                            buff_header[i] = 0;
                        }
                        let _ = header_file.write(&mut buff_header).await;
                    } else {
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
                        let _ = header_file.write(&mut buff_header).await;
                        let _ = data_file.write(&mut data.bytes()).await;
                        byte_pointer += data_len;
                    }
                    data_id += 1;
                }

                let _ = header_file.flush().await;
                let _ = data_file.flush().await;
            }
        } else {
            // eprintln!("Unable to read root hash for {}, breaking", c_id);
            break;
        }
    }
}

pub async fn load_content_from_disk(
    s_storage: PathBuf,
    cid: u16,
    dtype: DataType,
    hash: u64,
) -> Option<Content> {
    // eprintln!("Load content from disk: {:?} {}", s_storage, cid);
    let header_file = s_storage.join(format!("{}.hdr", cid));
    let data_file = s_storage.join(format!("{}.dat", cid));
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
            let mut buffer: [u8; 1024] = [0; 1024];
            let mut file = BufReader::new(File::open(data_file).await.unwrap());
            for i in 0..=u16::MAX {
                if let Some((hash, pointer, size)) = temp_storage.remove(&i) {
                    let size = size as usize;
                    //TODO
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
                            if data.get_hash() == hash {
                                let _ar = c_tree.append(data);
                                // eprintln!("Append result: {:?}", _ar);
                            } else {
                                eprintln!("CID-{} Page hash mismatch", cid);
                                return None;
                            }
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
            return Some(Content::Data(dtype, c_tree));
        } else {
            eprintln!("Content from hdr file hash {} mismatch {}", dhash, hash,);
            return None;
        }
    } else {
        eprintln!("Unable to load content from {:?} file", header_file);
        return None;
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
    // eprintln!("Reading {:?} file", file_path);
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
        // eprintln!("Insert {} - {}", page_id, page_hash);
        temp_storage.insert(page_id, (page_hash, offset, page_size));
    }
    // eprintln!("Read {} entries", temp_storage.len());
    if let Some((hash, _offset, _size)) = temp_storage.get(&0) {
        // eprintln!("Has root entry!");
        let mut content = Content::from(dtype, Data::empty(*hash)).unwrap();
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

pub async fn write_datastore_to_disk(file_path: PathBuf, app_data: &ApplicationData) {
    let mut temp_store = HashMap::new();
    let mut root_hash = 0;
    let _ = parse_datastore_file(file_path.clone(), &mut temp_store, &mut root_hash).await;
    let root_hash_in_memory = app_data.root_hash();
    if root_hash_in_memory == root_hash {
        // eprintln!("datastore.sync is up to date - {}", root_hash);
        return;
    }
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
    // Maybe we provide this function what ordered changes should be appended?
}
