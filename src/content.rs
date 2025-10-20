use gnome::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
// use std::hash::DefaultHasher;
use std::hash::Hash;
// use std::hash::Hasher;

use crate::error::SubtreeError;
use crate::prelude::AppError;
use crate::prelude::Data;
pub type ContentID = u16;

// impl fmt::Display for ContentID {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         write!(f, "C={}", self)
//     }
// }
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataType {
    Data(u8),
    Link,
}
impl DataType {
    pub fn from(val: u8) -> Self {
        if val == 255 {
            DataType::Link
        } else {
            DataType::Data(val)
        }
    }
    pub fn byte(&self) -> u8 {
        match self {
            DataType::Link => 255,
            DataType::Data(val) => *val,
        }
    }
    pub fn is_link(&self) -> bool {
        match self {
            DataType::Link => true,
            DataType::Data(_val) => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Description(String);
impl Description {
    pub fn new(text: String) -> Result<Self, ()> {
        if text.len() > 128 {
            return Err(());
        }
        Ok(Self(text))
    }
    pub fn text(&self) -> String {
        self.0.clone()
    }
    pub fn as_bytes(&self) -> Vec<u8> {
        let len = self.0.len();
        let mut bytes = Vec::with_capacity(1 + len);
        bytes.push(len as u8);
        for b in self.0.bytes() {
            bytes.push(b);
        }
        bytes
    }
}
#[derive(Debug, Clone)]
pub enum Content {
    // Link's Data contains application level header data (if Catalog -> Tags)
    // When we copy a Link we only have to take first two values,
    // rest is optional
    // When we hash() a Link we always only take first two values
    // this way links with different description but poining to the same
    // content will have the same hash
    Link(
        SwarmName,
        ContentID,
        Description,
        Data,
        Option<TransformInfo>,
    ),
    Data(DataType, u16, ContentTree),
}

//TODO: We need to implement a mechanism that will allow for synchronization
// of larger data structures.
// One way to do it is to use regular AppendData SyncMessage, but it has many
// downsides:
// 1 - not everybody is interested in given Content, and is totally happy with
//     only storing a single hash of it
// 2 - during synchronization bandwith is drained so other messages have it
//     difficult to get synced
//
// As a solution we could extend Content::Link with some additional info
// that will inform everyone about given link's nature - that this is just
// a seed to be converted into actual Data.

// TODO: Tags should be synced into Datastore
// type Tag = HashMap<u8, String>;

#[derive(Debug, Clone)]
pub struct TransformInfo {
    pub d_type: DataType,
    pub size: u16,
    pub root_hash: u64,
    pub broadcast_id: CastID,
    // pub description: String,
    pub missing_hashes: HashSet<u16>,
    pub data_hashes: Vec<Data>,
    pub data: HashMap<u16, Data>,
}

// To find how many Datahashes there is:
//     size >> 7 + if (size<<9).count_ones() > 0 {
//         1
//     }else{
//         0
//     }
// Of course if we start indexing from zero we have to substract one from above.

// If size <= 128 then we can send all Data Hashes within signle Data block
//         <= 256             2 Datablocks
//         <= 512             4 Datablocks
//         <= 1024            8 Datablocks
//         <= 2048           16 Datablocks
//         <= 4096           32 Datablocks
//         <= 8192           64 Datablocks
//         <= 16384         128 Datablocks
//         <= 32768         256 Datablocks
//         <= 65536         512 Datablocks
// We should also store a local u16 number pointing to first hash data index that is
// missing. This way if we receive hashes with index different from what we were
// expecting we immediately ask a Neighbor to send us this missing block.
// It is possible that this procedure will fail and a SyncMessage calling to Transform
// comes. Now we have no choice but to execute it. If we have all hashes - no problem.
// But if we are still missing some, we simply put a Data::Empty(root_hash) into
// Datastore.
// In cache we keep all datablocks with hashes and keep requesting for missing ones.
// Also in cache we should store Content's Data that's incoming for later.
// Once we have all hashes, we construct BinaryTree from them, and move what Data
// we have stored in cache into place.
// Now we can replace that placeholder containing only root_hash with entire tree,
// and keep adding whatever Data comes from broadcast.

impl TransformInfo {
    pub fn from(bytes: Vec<u8>) -> Option<Self> {
        // eprintln!("TI from: {:?} bytes", bytes);
        let mut bytes_iter = bytes.into_iter();
        if let Some(d_type) = bytes_iter.next() {
            let b1 = bytes_iter.next().unwrap();
            let b2 = bytes_iter.next().unwrap();
            let size = u16::from_be_bytes([b1, b2]);
            // let tags_len = bytes_iter.next().unwrap();
            // eprintln!("tag len: {}", tags_len);
            // let mut tags = Vec::with_capacity(tags_len as usize);
            // for _i in 0..tags_len {
            //     tags.push(bytes_iter.next().unwrap());
            // }
            // now goes root hash
            let b1 = bytes_iter.next().unwrap();
            let b2 = bytes_iter.next().unwrap();
            let b3 = bytes_iter.next().unwrap();
            let b4 = bytes_iter.next().unwrap();
            let b5 = bytes_iter.next().unwrap();
            let b6 = bytes_iter.next().unwrap();
            let b7 = bytes_iter.next().unwrap();
            let b8 = bytes_iter.next().unwrap();
            let root_hash = u64::from_be_bytes([b1, b2, b3, b4, b5, b6, b7, b8]);
            let broadcast_id = CastID(bytes_iter.next().unwrap());
            // let b1 = bytes_iter.next().unwrap();
            // let b2 = bytes_iter.next().unwrap();
            // let descr_len = u16::from_be_bytes([b1, b2]);
            // let mut descr_vec = Vec::with_capacity(descr_len as usize);
            // for _i in 0..descr_len {
            //     descr_vec.push(bytes_iter.next().unwrap());
            // }
            // let description = String::from_utf8(descr_vec).unwrap();
            let b1 = bytes_iter.next().unwrap();
            let b2 = bytes_iter.next().unwrap();
            let missing_len = u16::from_be_bytes([b1, b2]);
            let mut missing_hashes = HashSet::with_capacity(missing_len as usize);
            for _i in 0..missing_len {
                let b1 = bytes_iter.next().unwrap();
                let b2 = bytes_iter.next().unwrap();
                missing_hashes.insert(u16::from_be_bytes([b1, b2]));
            }
            let b1 = bytes_iter.next().unwrap();
            let b2 = bytes_iter.next().unwrap();
            let hashes_len = u16::from_be_bytes([b1, b2]);
            let b1 = bytes_iter.next().unwrap();
            let b2 = bytes_iter.next().unwrap();
            let data_len = u16::from_be_bytes([b1, b2]);

            Some(TransformInfo {
                d_type: DataType::from(d_type),
                // tags,
                size,
                root_hash,
                broadcast_id,
                // description,
                missing_hashes,
                data_hashes: Vec::with_capacity(hashes_len as usize),
                data: HashMap::with_capacity(data_len as usize),
            })
        } else {
            None
        }
    }
    pub fn into_tree(mut self) -> ContentTree {
        if !self.missing_hashes.is_empty() || self.data_hashes.is_empty() {
            return ContentTree::Empty(self.root_hash);
        }
        // First we need to build a Vec<u64> of hashes,
        let mut hashes = Vec::with_capacity(128 * self.data_hashes.len());
        for data_hash in self.data_hashes {
            let mut bytes_iter = data_hash.bytes().into_iter();
            while let Some(b1) = bytes_iter.next() {
                let b2 = bytes_iter.next().unwrap();
                let b3 = bytes_iter.next().unwrap();
                let b4 = bytes_iter.next().unwrap();
                let b5 = bytes_iter.next().unwrap();
                let b6 = bytes_iter.next().unwrap();
                let b7 = bytes_iter.next().unwrap();
                let b8 = bytes_iter.next().unwrap();
                hashes.push(u64::from_be_bytes([b1, b2, b3, b4, b5, b6, b7, b8]));
            }
        }
        // Then for each hash we take corresponding Data and compare hashes
        // If they are the same we push Data, otherwise we push empty Data shell with hash only
        let mut c_tree = ContentTree::Empty(self.root_hash);
        for (i, hash) in hashes.into_iter().enumerate() {
            if let Some(mut data) = self.data.remove(&(i as u16)) {
                if data.hash() == hash {
                    let _ = c_tree.append(data);
                } else {
                    eprintln!("Hash mismatch data's: {}, announced: {}", data.hash(), hash);
                    let _ = c_tree.append(Data::empty(hash));
                }
            } else {
                let _ = c_tree.append(Data::empty(hash));
            }
        }
        c_tree
    }
    pub fn bytes(self) -> Vec<u8> {
        // pub d_type: DataType,
        let mut bytes = vec![self.d_type.byte()];
        // pub size: u16,
        for byte in self.size.to_be_bytes() {
            bytes.push(byte);
        }
        // pub tags: Vec<u8>,
        // bytes.push(self.tags.len() as u8);
        // for tag in self.tags {
        //     bytes.push(tag);
        // }
        // pub root_hash: u64,
        for byte in self.root_hash.to_be_bytes() {
            bytes.push(byte);
        }
        // pub broadcast_id: CastID,
        bytes.push(self.broadcast_id.0);
        // pub description: String,
        // for byte in (self.description.len() as u16).to_be_bytes() {
        //     bytes.push(byte);
        // }
        // for byte in self.description.bytes() {
        //     bytes.push(byte);
        // }
        // pub missing_hashes: Vec<u16>,

        // We can not exceed 512 bytes total for TransformInfo
        // since it has to fit into single Data with other values
        let missing_len = self.missing_hashes.len() as u16;
        if missing_len > 61 {
            bytes.push(0);
            bytes.push(61);
            let mut pushed = 0;
            {
                for m_id in self.missing_hashes {
                    for byte in m_id.to_be_bytes() {
                        bytes.push(byte);
                    }
                    pushed += 1;
                    if pushed >= 61 {
                        break;
                    }
                }
            }
        } else {
            for byte in missing_len.to_be_bytes() {
                bytes.push(byte);
            }
            for m_id in self.missing_hashes {
                for byte in m_id.to_be_bytes() {
                    bytes.push(byte);
                }
            }
        }
        // pub data_hashes: Vec<Data>,
        for byte in (self.data_hashes.len() as u16).to_be_bytes() {
            bytes.push(byte);
        }
        // pub data: Vec<Data>,
        for byte in (self.data.len() as u16).to_be_bytes() {
            bytes.push(byte);
        }
        bytes
    }
    pub fn add_hash(&mut self, part_no: u16, _total_parts: u16, data: Data) {
        //TODO
        eprintln!(
            "Adding hash: [{}/{}], len: {}",
            part_no,
            _total_parts,
            data.len()
        );
        if self.missing_hashes.contains(&part_no) {
            self.missing_hashes.remove(&part_no);
            let dh_len = self.data_hashes.len() as u16;
            if dh_len > part_no {
                self.data_hashes[part_no as usize] = data;
            } else if dh_len == part_no {
                self.data_hashes.push(data)
            } else {
                eprintln!(
                    "Received hashes [{}/{}], we have only {} parts",
                    part_no, _total_parts, dh_len
                );
            }
        } else {
            eprintln!("not missing");
            let current_len = self.data_hashes.len() as u16;
            if current_len < part_no {
                for i in current_len..part_no {
                    self.missing_hashes.insert(i);
                    self.data_hashes.push(Data::empty(0));
                }
            } else if current_len > part_no {
                // Overwrite unconditionally
                self.data_hashes[part_no as usize] = data;
            } else {
                self.data_hashes.push(data);
            }
        }
        // panic!("just to see");
    }

    pub fn add_data(&mut self, part_no: u16, total_parts: u16, data: Data) {
        // println!(
        //     "add_data: [{}/{}] {}",
        //     part_no,
        //     _total_parts,
        //     self.data.len()
        // );
        // TODO: check if this is correct
        // let hash_data_id = if part_no < 128 { 0 } else { part_no >> 7 };
        self.size = total_parts;
        let hash_data_id = part_no >> 7;
        // println!("hID: {}, DHlen: {}", hash_data_id, self.data_hashes.len());
        if self.missing_hashes.contains(&hash_data_id) {
            eprintln!("Missing hash, adding unconditionally");
            let _res = self.data.insert(part_no, data);
        } else if hash_data_id as usize >= self.data_hashes.len() {
            for i in self.data_hashes.len() as u16..=hash_data_id {
                self.missing_hashes.insert(i);
            }
        } else {
            // TODO: check if this is correct
            let hidx = ((part_no % 128) * 8) as usize;
            // println!("Hidx: {}, DHlen: {}", hidx, self.data_hashes.len());
            let b = self.data_hashes[hash_data_id as usize].ref_bytes();
            let hash = u64::from_be_bytes([
                b[hidx],
                b[hidx + 1],
                b[hidx + 2],
                b[hidx + 3],
                b[hidx + 4],
                b[hidx + 5],
                b[hidx + 6],
                b[hidx + 7],
            ]);
            if hash == data.get_hash() {
                let _res = self.data.insert(part_no, data);
            } else {
                eprintln!("Hashes do not match, not adding");
            }
        }
        // println!("Data len before: {}", self.data.len());
        // let res = self.data.insert(part_no, Data::empty());
        // self.data.len()
        eprint!("L:{}\t", self.data.len());
    }
    pub fn what_hashes_are_missing(&self) -> Vec<u16> {
        let mut res = Vec::with_capacity(self.missing_hashes.len());
        for hash in &self.missing_hashes {
            res.push(*hash);
        }
        res
    }
    pub fn what_data_is_missing(&self, up_to_index: u16) -> Vec<u16> {
        let mut res = vec![];
        for i in 0..up_to_index {
            if self.data.get(&i).is_none() {
                res.push(i);
            }
        }
        res
    }
}
// That information will contain DataType, Tags, Description, Size, Root hash
// and some other stuff like BroadcastID.
// Now everyone sees what is about to happen - in future this Link will get
// converted into Data and it will contain Data with specific DataType and Tags.
// Now everyone can decide whether or not he is interested in syncing that Data
// to his Datastore, or maybe just syncing all Data hashes, or nothing at all.
// Once everyone is on the same page about nature of given link a Broadcast is
// initiated for everyone interested to join.
// That broadcast is used in two tiers:
// 1 - first all Data hashes are being transmitted so that everyone knows what
//     Data goes where and confirm that Data is a match to overall tree and it's
//     root hash.
//     All this Data is stored as an Appendix to given Link.
//     After this first stage is completed a SyncMessage is sent for everyone
//     to transform given Link into Data and use that stored hashes to create
//     a shell of actual Data tree.
// 2 - now Data is being transmitted over a broadcast channel and everyone
//     subscribed can update his Content without syncing with the Swarm, since
//     all the hashes stay intact. It's just Data::Empty(hash) being
//     converted to Data::Filled with the same exact hash.
//
// Everything described above happens in parallel to SyncMessages being
// transmitted, so application operates as always.
// We can have up to 256 such synchronizations occur simultaneously per swarm
// and still stay in Sync and update Datastore.
impl Content {
    pub fn from(d_type: DataType, data: Data) -> Result<Self, AppError> {
        let d_hash = data.get_hash();
        let data_empty = data.is_empty();
        // eprintln!("Content from bytes: {:?}", bytes);
        // let d_type = bytes_iter.next();
        // println!("First byte: {:?}", first_byte);
        match d_type {
            // None => Err(AppError::Smthg),
            DataType::Link => {
                data_to_link(data)
                // let bytes = data.bytes();
                // // TODO: first goes variable len SwarmName
                // // and then CID
                // let mut bytes_iter = bytes.into_iter();
                // let mut name_len = bytes_iter.next().unwrap();
                // let mut name_bytes = vec![name_len];
                // name_len = if name_len < 128 {
                //     name_len
                // } else {
                //     name_len - 128
                // };
                // for _i in 0..name_len {
                //     name_bytes.push(bytes_iter.next().unwrap())
                // }
                // let s_name = SwarmName::from(&name_bytes);
                // let tid1 = bytes_iter.next().unwrap();
                // let tid2 = bytes_iter.next().unwrap();
                // let c_id = ContentID::from_be_bytes([tid1, tid2]);
                // if let Ok(n) = &s_name {
                //     eprintln!("Link to swarm: {}", n);
                // } else {
                //     eprintln!("Failed to read SN from bytes: {:?}", s_name);
                // }
                // // let b1 = bytes_iter.next().unwrap();
                // // let b2 = bytes_iter.next().unwrap();
                // // let b3 = bytes_iter.next().unwrap();
                // // let b4 = bytes_iter.next().unwrap();
                // // let b5 = bytes_iter.next().unwrap();
                // // let b6 = bytes_iter.next().unwrap();
                // // let b7 = bytes_iter.next().unwrap();
                // // let b8 = bytes_iter.next().unwrap();
                // // let g_id = GnomeId(u64::from_be_bytes([b1, b2, b3, b4, b5, b6, b7, b8]));
                // // // next 2 bytes is ContentID
                // // let b1 = bytes_iter.next().unwrap();
                // // let b2 = bytes_iter.next().unwrap();
                // // let c_id = u16::from_be_bytes([b1, b2]);
                // let tags_len = bytes_iter.next().unwrap();
                // eprintln!("Tags len: {}", tags_len);
                // let mut tag_vec = Vec::with_capacity(tags_len as usize + 1);
                // tag_vec.push(tags_len);
                // for _i in 0..tags_len {
                //     tag_vec.push(bytes_iter.next().unwrap());
                // }
                // // let swarm_name: String = String::from_utf8(name_vec).unwrap();
                // // let swarm_name: SwarmName = SwarmName::new(g_id, swarm_name).unwrap();
                // // let descr_len = bytes_iter.next().unwrap();
                // // let mut descr_vec = Vec::with_capacity(descr_len as usize);
                // // for _i in 0..descr_len {
                // //     descr_vec.push(bytes_iter.next().unwrap());
                // // }
                // let description: Description =
                //     Description::new(String::from_utf8(bytes_iter.collect()).unwrap()).unwrap();
                // // let data_len = bytes_iter.next().unwrap();
                // // let mut data_vec = Vec::with_capacity(data_len as usize);
                // // for _i in 0..data_len {
                // //     data_vec.push(bytes_iter.next().unwrap());
                // // }
                // // TODO: we have to encode Link properly with Data and TI

                // // let ti = TransformInfo::from(bytes_iter.collect());
                // Ok(Content::Link(
                //     s_name.unwrap(),
                //     c_id,
                //     description,
                //     Data::empty(0),
                //     // Data::new(data_vec).unwrap(),
                //     None,
                // ))
            }
            // TODO: we can define instructions to create hollow Content
            // containing only hashes of either Data or non-data hashes
            // at certain hash pyramid floor (counted from top to bottom)
            // TODO: we can also define additional instruction to substitute
            // selected bottom non-data leaf with another hash pyramid that contains
            // at it's bottom root data hashes
            // TODO: once we have all root data hashes we can start
            // replacing them one-by-one
            DataType::Data(other) => {
                if data_empty {
                    Ok(Content::Data(
                        DataType::from(other),
                        0,
                        ContentTree::empty(d_hash),
                    ))
                } else {
                    Ok(Content::Data(
                        DataType::from(other),
                        1,
                        ContentTree::new(data),
                    ))
                }
            }
        }
    }
    pub fn tag_ids(&self) -> Vec<u8> {
        match self {
            Self::Link(_sn, _c, _descr, d_tags, _ti) => {
                if d_tags.is_empty() {
                    return vec![];
                }
                let mut t_bytes = d_tags.clone().bytes();
                let _len = t_bytes.remove(0);
                t_bytes
            }
            Self::Data(_d_type, _mem, _ct) => {
                //TODO
                vec![]
            }
        }
    }
    pub fn description(&self) -> String {
        match self {
            Self::Link(_sn, _c, descr, _tags, _ti) => descr.0.clone(),
            Self::Data(_d_type, _mem, _ct) => String::new(),
        }
    }
    pub fn data_type(&self) -> DataType {
        match self {
            Self::Link(_sn, _c, _descr, _tags, _ti) => DataType::Link,
            Self::Data(d_type, _mem, _ct) => *d_type,
        }
    }
    pub fn len(&self) -> u16 {
        match self {
            Self::Link(_sn, _c, _descr, _data, ti) => {
                if let Some(ti) = ti {
                    ti.size
                } else {
                    0
                }
            }
            Self::Data(_d_type, _mem, content_tree) => content_tree.len(),
        }
    }
    pub fn to_data(self) -> Result<Data, Self> {
        match self {
            Content::Link(s_name, c_id, descr, data, ti_opt) => {
                Ok(link_to_data(s_name, c_id, descr, data, ti_opt))
            }
            // TODO: here we drop existing Content when it is not a Link!
            other => Err(other),
        }
    }
    pub fn read_data(&self, data_id: u16) -> Result<Data, AppError> {
        // eprintln!("Internal read_data {}", data_id);
        match self {
            Self::Link(s_name, c_id, descr, data, ti) => {
                // if let Some(ti) = ti {
                //     if let Some(data) = ti.data.get(&data_id) {
                //         Ok(data.clone())
                //     } else {
                //         Err(AppError::IndexingError)
                //     }
                // } else {
                if data_id == 0 {
                    // eprintln!("Converting link to data, {} TI: {:?}", s_name, ti.is_some());
                    Ok(link_to_data(
                        s_name.clone(),
                        *c_id,
                        descr.clone(),
                        data.clone(),
                        ti.clone(),
                    ))
                } else {
                    eprintln!(
                        "TODO: Link indexing error (idx {} > 0 not yet supported)",
                        data_id
                    );
                    Err(AppError::IndexingError)
                }
                // }
            }
            Self::Data(_type, _mem, c_tree) => c_tree.read(data_id),
        }
    }
    pub fn read_link_data(&self, d_type: DataType, data_id: u16) -> Result<(Data, u16), AppError> {
        match self {
            Self::Link(_s_name, _c_id, _descr, _data, ti) => {
                if let Some(ti) = ti {
                    if d_type == ti.d_type {
                        if let Some(data) = ti.data.get(&data_id) {
                            Ok((data.clone(), ti.size))
                        } else {
                            Err(AppError::IndexingError)
                        }
                    } else {
                        Err(AppError::DatatypeMismatch)
                    }
                } else {
                    Err(AppError::LinkNonTransformative)
                }
            }
            _other => Err(AppError::DatatypeMismatch),
        }
    }
    // memusechange 1of5
    pub fn update(&mut self, content: Content) -> Content {
        std::mem::replace(self, content)
    }
    // memusechange 2of5
    pub fn push_data(&mut self, data: Data) -> Result<u64, AppError> {
        match self {
            Self::Link(_s, _c, _descr, _data, _ti) => Err(AppError::DatatypeMismatch),
            Self::Data(_dt, _mem, tree) => {
                if data.is_empty() {
                    tree.append(data)
                } else {
                    let res = tree.append(data);
                    if res.is_ok() {
                        *self = Self::Data(*_dt, *_mem + 1, tree.to_owned());
                    }
                    res
                }
            }
        }
    }
    // memusechange 3of5
    pub fn pop_data(&mut self) -> Result<Data, AppError> {
        // eprintln!("pop_data");
        match self {
            Self::Link(_s, _c, _descr, _data, _ti) => Err(AppError::DatatypeMismatch),
            Self::Data(_dt, _mem, tree) => {
                if tree.is_empty() {
                    // eprintln!("tree empty");
                    Err(AppError::ContentEmpty)
                } else {
                    // eprintln!("tree pop");
                    let res = tree.pop();
                    if let Ok(data) = &res {
                        if !data.is_empty() {
                            *self = Self::Data(*_dt, *_mem - 1, tree.to_owned());
                        }
                    }
                    res
                }
            }
        }
    }

    pub fn insert(&mut self, d_id: u16, data: Data) -> Result<u64, AppError> {
        match self {
            Self::Link(_s, _c, _descr, _data, _ti) => Err(AppError::DatatypeMismatch),
            Self::Data(_dt, _mem, tree) => {
                let tree_len_max = tree.len() == u16::MAX;
                if tree_len_max {
                    let last_empty = tree.read(u16::MAX).unwrap().is_empty();
                    if data.is_empty() {
                        // if last elem is not empty mem - 1
                        if !last_empty {
                            let res = tree.insert(d_id, data);
                            if res.is_ok() {
                                *self = Self::Data(*_dt, *_mem - 1, tree.to_owned());
                            }
                            res
                        } else {
                            tree.insert(d_id, data)
                        }
                    } else {
                        // if last elem IS empty + 1
                        if last_empty {
                            let res = tree.insert(d_id, data);
                            if res.is_ok() {
                                *self = Self::Data(*_dt, *_mem + 1, tree.to_owned());
                            }
                            res
                        } else {
                            tree.insert(d_id, data)
                        }
                    }
                } else if data.is_empty() {
                    tree.insert(d_id, data)
                } else {
                    let res = tree.insert(d_id, data);
                    if res.is_ok() {
                        *self = Self::Data(*_dt, *_mem + 1, tree.to_owned());
                    }
                    res
                }
            }
        }
    }
    pub fn link_params(
        &self,
    ) -> Option<(
        SwarmName,
        ContentID,
        Description,
        Data,
        Option<TransformInfo>,
    )> {
        match self {
            Self::Link(s_name, c_id, descr, data, ti_opt) => Some((
                s_name.clone(),
                *c_id,
                descr.clone(),
                data.clone(),
                ti_opt.clone(),
            )),
            _ => None,
        }
    }
    // memusechange 4of5
    pub fn update_data(&mut self, data_id: u16, data: Data) -> Result<Data, AppError> {
        let myself = std::mem::replace(
            self,
            Content::Data(DataType::Data(0), 0, ContentTree::Empty(0)),
        );
        let new_data_empty = data.is_empty();
        match myself {
            Self::Link(s_name, c_id, descr, t_data, ti) => {
                if data_id == 0 {
                    let link_result = data_to_link(data);
                    if let Ok(link) = link_result {
                        *self = link;
                        Ok(link_to_data(s_name, c_id, descr, t_data, ti))
                    } else {
                        *self = Self::Link(s_name, c_id, descr, t_data, ti);
                        Err(link_result.err().unwrap())
                    }
                    //If data_id == 1 then we only replace t_data
                } else if data_id == 1 {
                    *self = Self::Link(s_name.clone(), c_id, descr.clone(), data, ti.clone());
                    Ok(link_to_data(s_name, c_id, descr, t_data, ti))
                } else {
                    *self = Self::Link(s_name, c_id, descr, data, ti);
                    Err(AppError::IndexingError)
                }
            }
            Self::Data(d_type, mem, mut c_tree) => {
                let result = c_tree.replace(data_id, data);
                if let Ok(old_data) = result {
                    if old_data.is_empty() {
                        if new_data_empty {
                            *self = Self::Data(d_type, mem, c_tree);
                        } else {
                            *self = Self::Data(d_type, mem + 1, c_tree);
                        }
                    } else {
                        if new_data_empty {
                            *self = Self::Data(d_type, mem - 1, c_tree);
                        } else {
                            *self = Self::Data(d_type, mem, c_tree);
                        }
                    }
                    Ok(old_data)
                } else {
                    *self = Self::Data(d_type, mem, c_tree);
                    Err(result.err().unwrap())
                }
                // result
            }
        }
    }
    // memusechange 5of5
    pub fn remove_data(&mut self, d_id: u16) -> Result<Data, AppError> {
        match self {
            Self::Link(_s, _c, _descr, _data, _ti) => Err(AppError::DatatypeMismatch),
            Self::Data(_dt, _mem, tree) => {
                let result = tree.remove_data(d_id);
                if let Ok(data) = result {
                    if !data.is_empty() {
                        *self = Self::Data(*_dt, *_mem - 1, tree.to_owned());
                    }
                    return Ok(data);
                }
                result
            }
        }
    }

    pub fn shell(&self) -> Self {
        match self {
            Self::Link(s_name, c_id, descr, data, ti) => Self::Link(
                s_name.clone(),
                *c_id,
                descr.clone(),
                data.clone(),
                ti.clone(),
            ),
            Self::Data(d_type, mem, c_tree) => Self::Data(*d_type, *mem, c_tree.shell()),
        }
    }
    pub fn hash(&self) -> u64 {
        match self {
            Self::Link(s_name, c_id, description, data, ti) => {
                // eprintln!("dAta: {:?}", data);
                let mut l_data = link_to_data(
                    s_name.clone(),
                    *c_id,
                    description.clone(),
                    data.clone(),
                    ti.clone(),
                );
                // eprintln!("data: {:?}, {}", data, data.get_hash());
                l_data.hash()
                // let mut b_vec = vec![];
                // for byte in s_name.founder.bytes() {
                //     b_vec.push(byte);
                // }
                // for byte in s_name.name.bytes() {
                //     b_vec.push(byte);
                // }
                // for byte in c_id.to_be_bytes() {
                //     b_vec.push(byte);
                // }
                // let mut hasher = DefaultHasher::new();
                // b_vec.hash(&mut hasher);
                // hasher.finish()
            }
            Self::Data(_type, _mem, tree) => tree.hash(),
        }
    }
    pub fn link_ti_hashes(&self) -> Result<Vec<Data>, AppError> {
        if let Self::Link(_s_name, _, _descr, _data, ti_opt) = self {
            if let Some(ti) = ti_opt {
                Ok(ti.data_hashes.clone())
            } else {
                Err(AppError::LinkNonTransformative)
            }
        } else {
            Err(AppError::DatatypeMismatch)
        }
    }
    pub fn data_hashes(&self) -> Vec<u64> {
        let mut v = vec![];
        match self {
            Self::Link(_s_name, _c_id, _descr, _data, ti_opt) => {
                v.push(self.hash());
                if let Some(transform_info) = ti_opt {
                    for hash in &transform_info.data_hashes {
                        let mut bytes = hash.clone().bytes().into_iter();
                        while let Some(b1) = bytes.next() {
                            let b2 = bytes.next().unwrap();
                            let b3 = bytes.next().unwrap();
                            let b4 = bytes.next().unwrap();
                            let b5 = bytes.next().unwrap();
                            let b6 = bytes.next().unwrap();
                            let b7 = bytes.next().unwrap();
                            let b8 = bytes.next().unwrap();
                            let hash = u64::from_be_bytes([b1, b2, b3, b4, b5, b6, b7, b8]);
                            v.push(hash);
                        }
                    }
                    // } else {
                }
            }
            _other => {
                for d_id in 0..u16::MAX {
                    if let Ok(hash) = self.get_data_hash(d_id) {
                        v.push(hash)
                    } else {
                        break;
                    }
                }
            }
        }
        v
    }
    pub fn used_memory_pages(&self) -> usize {
        match self {
            Self::Link(_s, _c, _descr, _data, ti_opt) => {
                if let Some(ti) = ti_opt {
                    ti.data.len()
                } else {
                    1
                }
            }
            Self::Data(_type, mem, _tree) => *mem as usize,
        }
    }
    fn get_data_hash(&self, d_id: u16) -> Result<u64, AppError> {
        match self {
            Self::Link(_s, _c, _descr, _data, _ti) => {
                if d_id == 0 {
                    // Ok(link_to_data(*_g, _s.clone(), *_c, &_ti).hash())
                    Ok(self.hash())
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Data(_type, _mem, c_tree) => c_tree.get_data_hash(d_id),
        }
    }
}

pub fn data_to_link(data: Data) -> Result<Content, AppError> {
    eprintln!("data_to_link: {:?}", data.get_hash());
    let mut bytes_iter = data.bytes().into_iter();
    let len = bytes_iter.len();
    // eprintln!("creating link from {} bytes", len);
    if len < 11 {
        // eprintln!("data_to_link too short data {}", len);
        return Err(AppError::Smthg);
    }
    let d_len = bytes_iter.next().unwrap();
    // eprintln!("d_len: {}", d_len);
    let mut d_bytes = Vec::with_capacity(d_len as usize + 1);
    // d_bytes.push(d_len);
    for _i in 0..d_len {
        d_bytes.push(bytes_iter.next().unwrap());
    }
    let data = Data::new(d_bytes).unwrap();
    let olds_len = bytes_iter.next().unwrap();
    let s_len = if olds_len < 128 {
        olds_len
    } else {
        olds_len - 128
    };
    let mut s_bytes = Vec::with_capacity(s_len as usize + 1);
    s_bytes.push(olds_len);
    for _i in 0..s_len {
        s_bytes.push(bytes_iter.next().unwrap());
    }
    let s_name = SwarmName::from(&s_bytes).unwrap();
    let next_two = [bytes_iter.next().unwrap(), bytes_iter.next().unwrap()];
    let c_id = u16::from_be_bytes(next_two);

    let descr_len = bytes_iter.next().unwrap();
    // eprintln!("Descr len: {}", descr_len);
    let mut descr_vec = Vec::with_capacity(descr_len as usize);
    for _i in 0..descr_len {
        descr_vec.push(bytes_iter.next().unwrap());
    }
    // data_len byte
    // data bytes
    // swarm_name len
    // swarm_name
    // c_id
    // description len
    // description
    // TransformInfo len
    // TransformInfo (if above > 0)
    //
    let ti_len = bytes_iter.next().unwrap();
    // eprintln!("TI len: {}", ti_len);
    let ti = if ti_len == 0 {
        None
    } else {
        // let mut ti_bytes = Vec::with_capacity(ti_len as usize);
        let mut ti_bytes = vec![ti_len];

        // for _i in 0..ti_len {
        while let Some(byte) = bytes_iter.next() {
            // ti_bytes.push(bytes_iter.next().unwrap());
            ti_bytes.push(byte);
        }
        TransformInfo::from(ti_bytes)
    };
    // let mut data_vec = Vec::with_capacity(data_len as usize);
    // for _i in 0..data_len {
    //     data_vec.push(bytes_iter.next().unwrap());
    // }
    // // let g_id = GnomeId::from(first_eight);
    // // let s_name = String::from_utf8(name_vec).unwrap();
    // let swarm_name = SwarmName::new(g_id, s_name).unwrap();
    let description = Description::new(String::from_utf8(descr_vec).unwrap()).unwrap();
    // eprintln!("Link {} {}, data: {:?}", s_name, c_id, data);

    Ok(Content::Link(s_name, c_id, description, data, ti))
}

// TODO: make sure that we do not exceed 1024 bytes
fn link_to_data(
    s_name: SwarmName,
    c_id: ContentID,
    description: Description,
    data: Data,
    ti: Option<TransformInfo>,
) -> Data {
    // data_len byte
    // let mut v = data.bytes();
    // eprintln!("link_to_data: d_len: {}({:?})", data.len(), data);
    let mut v = vec![data.len() as u8];
    // data bytes
    v.append(&mut data.bytes());
    // v.push(data_bytes.len() as u8);
    // for b in data_bytes {
    //     v.push(b);
    // }
    // eprintln!("S name: {},", s_name.name,);
    // swarm_name len
    // swarm_name
    v.append(&mut s_name.as_bytes());
    // eprintln!(" len: {}", s_bytes.len());
    // v.push(s_bytes.len() as u8);// covered by s_name.as_bytes
    // eprintln!("link to data data: {:?}", data);
    // for b in s_bytes {
    //     v.push(b);
    // }
    //
    // c_id
    for b in c_id.to_be_bytes() {
        v.push(b);
    }
    // description len
    // description
    v.append(&mut description.as_bytes());
    // eprintln!("descr len: {}", d_bytes.len());
    // v.push(d_bytes.len() as u8);
    // for b in d_bytes {
    //     v.push(b);
    // }
    //
    // TransformInfo len
    // TransformInfo (if above > 0)
    //
    // eprintln!("link_to_data len before TI: {}", v.len());
    if let Some(ti) = ti {
        let mut ti_bytes = ti.bytes();
        // eprintln!("TI bytes: {:?}", ti_bytes);
        v.append(&mut ti_bytes);
    } else {
        v.push(0);
    }
    // eprintln!("link_to_data len: {}\n{:?}", v.len(), v);
    Data::new(v).unwrap()
}

// ContentTree should be a Binary Tree with Leafs containing up to
// 1024 bytes of data each, and maximum of u16::MAX Leafs in a tree
// This tree should always stay balanced, non-leaf nodes contain
// hash of it's sibling's data hash([left, right]).

// We should define a simple interface for ContentTree:
// - append new Data instance at end
// - read data at index
// - replace old_data at index with new_data, and retun old_data
// - insert data at index with:
//   a) replacing existing data at that index
//   b) increasing existing data and all subsequent data indeces by one
// - remove data at index with decreasing all subsequent data indeces by one

#[derive(Debug, Clone)]
pub enum ContentTree {
    Empty(u64),
    Filled(Data),
    Hashed(Box<Subtree>),
}

#[derive(Debug, Clone)]
pub struct Subtree {
    data_count: u16,
    hash: u64,
    left: ContentTree,
    right: ContentTree,
}

impl ContentTree {
    pub fn new(data: Data) -> Self {
        ContentTree::Filled(data)
    }

    pub fn from(data_vec: Vec<Data>) -> Self {
        let mut tree = ContentTree::empty(0);
        for data in data_vec {
            let _ = tree.append(data);
        }
        tree
    }

    pub fn empty(hash: u64) -> Self {
        ContentTree::Empty(hash)
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty(0))
    }

    pub fn len(&self) -> u16 {
        match self {
            Self::Empty(h) => {
                if *h == 0 {
                    0
                } else {
                    1
                }
            }
            Self::Filled(_d) => 1,
            Self::Hashed(sub_tree) => sub_tree.len(),
        }
    }

    // TODO: shell should preserve data structure, only replace Data with it's hash
    pub fn shell(&self) -> Self {
        match self {
            Self::Empty(hash) => Self::Empty(*hash),
            Self::Filled(data) => Self::Filled(Data::empty(data.get_hash())),
            Self::Hashed(sub_tree) => Self::Hashed(Box::new(Subtree {
                data_count: sub_tree.data_count,
                hash: sub_tree.hash,
                left: sub_tree.left.shell(),
                right: sub_tree.right.shell(),
            })),
        }
    }
    pub fn hash(&self) -> u64 {
        match self {
            Self::Empty(hash) => {
                // eprintln!("cont empty {}", hash);
                *hash
            }
            Self::Filled(data) => data.get_hash(),
            Self::Hashed(sub_tree) => {
                // eprintln!("subtree.hash");
                sub_tree.hash
            }
        }
    }
    pub fn get_data_hash(&self, data_id: u16) -> Result<u64, AppError> {
        // eprintln!("Getting data hash {}", data_id);
        match self {
            Self::Empty(hash) => {
                if data_id == 0 {
                    // eprintln!("empty {} {:?}", hash, hash.to_be_bytes());
                    Ok(*hash)
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Filled(data) => {
                // eprintln!("filled");
                if data_id == 0 {
                    // eprintln!(
                    //     "filled {} {:?}",
                    //     data.get_hash(),
                    //     data.get_hash().to_be_bytes()
                    // );
                    Ok(data.get_hash())
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(sub_tree) => {
                // eprintln!("hashed…");
                sub_tree.get_data_hash(data_id)
            }
        }
    }

    pub fn read(&self, idx: u16) -> Result<Data, AppError> {
        match self {
            Self::Filled(data) => {
                if idx == 0 {
                    Ok(data.clone())
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(sub_tree) => sub_tree.read(idx),
            Self::Empty(_h) => {
                if idx == 0 {
                    Ok(Data::empty(*_h))
                } else {
                    Err(AppError::IndexingError)
                }
            }
        }
    }

    pub fn replace(&mut self, idx: u16, mut new_data: Data) -> Result<Data, AppError> {
        // eprintln!("replace {}", idx);
        match self {
            Self::Filled(data) => {
                if idx == 0 {
                    // let n_hash = data.hash();
                    let old_data: Data = std::mem::replace(data, new_data);
                    Ok(old_data)
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(sub_tree) => sub_tree.replace(idx, new_data),
            Self::Empty(hash) => {
                // eprintln!("Existing hash: {}", hash);
                // eprintln!("New data hash: {} {}", new_data.get_hash(), new_data.hash());
                let new_hash = new_data.hash();
                if idx == 0 && *hash == new_hash {
                    *self = Self::Filled(new_data);
                    Ok(Data::empty(0))
                } else if idx != 0 {
                    Err(AppError::IndexingError)
                } else {
                    Err(AppError::HashMismatch)
                }
            }
        }
    }
    pub fn append(&mut self, data: Data) -> Result<u64, AppError> {
        let hash = data.get_hash();
        match self {
            Self::Filled(existing_data) => {
                // eprintln!(
                //     "I am filled {:?} {},\n{:?}",
                //     existing_data.clone().bytes(),
                //     existing_data.len(),
                //     existing_data.get_hash().to_be_bytes(),
                // );
                let first_hash = existing_data.get_hash();
                // eprintln!("first hash: {:?}", first_hash.to_be_bytes());
                let d_hash = double_hash(first_hash, hash);
                // eprintln!("double hash: {:?}", d_hash.to_be_bytes());
                let prev_tree = std::mem::replace(self, Self::Empty(0));
                let new_tree = ContentTree::Hashed(Box::new(Subtree {
                    data_count: 2,
                    hash: d_hash,
                    left: prev_tree,
                    right: ContentTree::Filled(data),
                }));
                // let _ = std::mem::replace(self, new_tree);
                *self = new_tree;
                // eprintln!("1 Len after append: {}", self.len());
                Ok(d_hash)
            }
            Self::Hashed(subtree) => {
                // eprintln!("I am hashed");
                //  Here we choose between two paths
                //   of growing this tree by another level
                //   by reassigning *self to new Hashed
                //   and storing in left previous self,
                //   and setting right to Filled,
                //   but this way the tree will become unbalanced
                if subtree.can_grow() {
                    if subtree.should_extend() {
                        // eprintln!("should extend");
                        let data_count = subtree.len() + 1;
                        let subtree_hash = subtree.hash;
                        let d_hash = double_hash(subtree_hash, hash);
                        let prev_tree = std::mem::replace(self, Self::Empty(0));
                        let right = if data.is_empty() {
                            ContentTree::Empty(data.get_hash())
                        } else {
                            ContentTree::Filled(data)
                        };
                        let new_tree = ContentTree::Hashed(Box::new(Subtree {
                            data_count,
                            hash: d_hash,
                            left: prev_tree,
                            right,
                        }));
                        let _ = std::mem::replace(self, new_tree);
                        // eprintln!("2 Len after append: {}", self.len());
                        Ok(d_hash)
                    } else {
                        // eprintln!("no extend");
                        let app_res = subtree.append(data);
                        // eprintln!("3 Len after append: {}", self.len());
                        if let Ok(_hash) = app_res {
                            Ok(self.hash())
                        } else {
                            app_res
                        }
                    }
                } else {
                    Err(AppError::ContentFull)
                }
            }
            Self::Empty(_hash) => {
                //TODO: what to do if existing hash = 0?
                // eprintln!("append I am empty {}", _hash);
                if data.is_empty() {
                    if *_hash == 0 {
                        let new_hash = data.get_hash();
                        *self = ContentTree::Empty(new_hash);
                        Ok(new_hash)
                    } else {
                        let double_hash = double_hash(*_hash, data.get_hash());
                        // eprintln!("data empty {}", double_hash);
                        let s_tree = Subtree {
                            data_count: 2,
                            hash: double_hash,
                            left: ContentTree::Empty(*_hash),
                            right: ContentTree::Empty(data.get_hash()),
                        };
                        *self = ContentTree::Hashed(Box::new(s_tree));
                        // eprintln!("4 Len after append: {}", self.len());
                        Ok(double_hash)
                    }
                } else {
                    // eprintln!("append data not empty");
                    *self = ContentTree::Filled(data);
                    // eprintln!("5 Len after append: {}", self.len());
                    Ok(hash)
                }
            }
        }
    }

    pub fn insert(&mut self, idx: u16, data: Data) -> Result<u64, AppError> {
        // Use append in this case
        // eprintln!("insert {:?} @ {}", data, idx);
        if idx > 0 && idx >= self.len() {
            // eprintln!("IE: idx {} >= {} self.len()", idx, self.len());
            return Err(AppError::IndexingError);
        }
        let remove_last = self.len() == u16::MAX;
        let result = match self {
            Self::Empty(_hash) => {
                // eprintln!("Insert Empty {}", _hash);
                if idx == 0 {
                    let hash = data.get_hash();
                    // eprintln!(
                    //     "Empty, replacing {} with {}, dlen: {}",
                    //     _hash,
                    //     hash,
                    //     data.len()
                    // );
                    *self = Self::Filled(data);
                    Ok(hash)
                } else {
                    // eprintln!("idx is not 0");
                    Err(AppError::IndexingError)
                }
            }
            Self::Filled(old_data) => {
                // eprintln!("Insert Filled {}", old_data.get_hash());
                if idx == 0 {
                    let left_hash = data.get_hash();
                    let right_hash = old_data.hash();
                    let hash = double_hash(left_hash, right_hash);
                    *self = Self::Hashed(Box::new(Subtree {
                        data_count: 2,
                        hash,
                        left: ContentTree::Filled(data),
                        right: ContentTree::Filled(old_data.clone()),
                    }));
                    Ok(hash)
                } else {
                    // This should not happen, but anyway...
                    // eprintln!("filled idx is not 0");
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(subtree) => {
                // eprintln!("Insert hashed");
                subtree.insert(idx, data)
            }
        };
        if remove_last && result.is_ok() {
            let _ = self.pop();
        }
        result
    }

    pub fn remove_data(&mut self, idx: u16) -> Result<Data, AppError> {
        let myself = std::mem::replace(self, Self::empty(0));
        match myself {
            Self::Empty(_hash) => Err(AppError::ContentEmpty),
            Self::Filled(d) => {
                if idx == 0 {
                    Ok(d)
                } else {
                    *self = Self::Filled(d);
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(mut subtree) => {
                let chunks_count = subtree.len();
                // eprintln!("remove_data({}), count: {}", idx, chunks_count);
                if idx >= chunks_count {
                    *self = Self::Hashed(subtree);
                    return Err(AppError::IndexingError);
                }
                let shifted_data = subtree.pop();
                match shifted_data {
                    Ok(mut data) => {
                        for i in (idx..chunks_count - 1).rev() {
                            data = subtree.replace(i, data).unwrap();
                        }
                        *self = Self::Hashed(subtree);
                        Ok(data)
                    }
                    Err(st_err) => match st_err {
                        SubtreeError::Empty => {
                            *self = Self::Hashed(subtree);
                            Err(AppError::ContentEmpty)
                        }
                        SubtreeError::DatatypeMismatch => {
                            *self = Self::Hashed(subtree);
                            Err(AppError::DatatypeMismatch)
                        }
                        SubtreeError::BothLeavesEmpty(data) => {
                            *self = ContentTree::empty(0);
                            Ok(data)
                        }
                        SubtreeError::RightLeafEmpty(mut data) => {
                            *self = match subtree.left {
                                ContentTree::Empty(hash) => ContentTree::Empty(hash),
                                ContentTree::Filled(e_data) => ContentTree::Filled(e_data),
                                ContentTree::Hashed(mut boxed_st) => {
                                    boxed_st.hash();
                                    ContentTree::Hashed(boxed_st)
                                }
                            };
                            for i in (idx..chunks_count - 1).rev() {
                                data = self.replace(i, data).unwrap();
                            }
                            Ok(data)
                        }
                    },
                }
            }
        }
    }

    fn pop(&mut self) -> Result<Data, AppError> {
        let myself = std::mem::replace(self, Self::Empty(0));
        match myself {
            Self::Empty(hash) => Ok(Data::empty(hash)),
            Self::Filled(data) => {
                // eprintln!("t pop 0 self: {:?}", self);
                Ok(data)
            }
            Self::Hashed(mut subtree) => {
                // eprintln!("t pop 1 stlen before: {}", subtree.len());
                let pop_result = subtree.pop();
                match pop_result {
                    Ok(data) => {
                        // eprintln!("ok {}", subtree.data_count);
                        *self = Self::Hashed(subtree);
                        Ok(data)
                    }
                    Err(st_err) => {
                        // eprintln!("err: {} ,dcount:{}", st_err, subtree.data_count);
                        match st_err {
                            SubtreeError::Empty => Err(AppError::ContentEmpty),
                            SubtreeError::DatatypeMismatch => {
                                *self = Self::Hashed(subtree);
                                // eprintln!("dt mismatch");
                                Err(AppError::DatatypeMismatch)
                            }
                            SubtreeError::BothLeavesEmpty(data) => {
                                *self = Self::Empty(0);
                                // eprintln!("both empty");
                                Ok(data)
                            }
                            SubtreeError::RightLeafEmpty(data) => {
                                // eprintln!("RLE2 {:?}", subtree.left);
                                *self = match subtree.left {
                                    ContentTree::Empty(hash) => ContentTree::Empty(hash),
                                    ContentTree::Filled(data) => ContentTree::Filled(data),
                                    ContentTree::Hashed(mut boxed_st) => {
                                        boxed_st.hash();
                                        // eprintln!("New st hash: {}", boxed_st.hash);
                                        ContentTree::Hashed(boxed_st)
                                    }
                                };
                                // eprintln!("right empty {:?}", self);
                                Ok(data)
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Subtree {
    pub fn can_grow(&self) -> bool {
        self.data_count < u16::MAX
    }
    pub fn should_extend(&self) -> bool {
        // self.data_count.count_ones() == 1 && self.right.is_empty() && self.data_count > 1
        // Above is probably wrong, right subtree will be empty after we extend, not before
        self.data_count.count_ones() == 1 && self.data_count > 1
    }

    pub fn len(&self) -> u16 {
        self.data_count
    }
    pub fn is_empty(&self) -> bool {
        self.data_count == 0
    }

    pub fn hash(&mut self) -> u64 {
        // let mut hasher = DefaultHasher::new();
        // [self.left.hash(), self.right.hash()].hash(&mut hasher);
        // self.hash = hasher.finish();
        self.hash = double_hash(self.left.hash(), self.right.hash());
        self.hash
    }
    pub fn get_data_hash(&self, idx: u16) -> Result<u64, AppError> {
        if idx >= self.data_count {
            Err(AppError::IndexingError)
        } else {
            let left_count = self.left.len();
            // eprintln!("left count {}", left_count);
            if idx >= left_count {
                // eprintln!("get right hash @{}", idx - left_count);
                self.right.get_data_hash(idx - left_count)
            } else {
                // eprintln!("get left hash @{}", idx);
                self.left.get_data_hash(idx)
            }
        }
    }
    pub fn read(&self, idx: u16) -> Result<Data, AppError> {
        if idx >= self.data_count {
            eprintln!("Req read {}, when data count: {}", idx, self.data_count);
            Err(AppError::IndexingError)
        } else {
            let left_count = self.left.len();
            if idx >= left_count {
                self.right.read(idx - left_count)
            } else {
                self.left.read(idx)
            }
        }
    }

    pub fn replace(&mut self, idx: u16, new_data: Data) -> Result<Data, AppError> {
        if idx >= self.data_count {
            Err(AppError::IndexingError)
        } else {
            let left_count = self.left.len();
            if idx >= left_count {
                let result = self.right.replace(idx - left_count, new_data);
                if let Ok(data) = result {
                    // We need to return our hash, not child's
                    self.hash();
                    Ok(data)
                } else {
                    result
                }
            } else {
                let result = self.left.replace(idx, new_data);
                if let Ok(data) = result {
                    // We need to return our hash, not child's
                    self.hash();
                    Ok(data)
                } else {
                    result
                }
            }
        }
    }
    pub fn append(&mut self, data: Data) -> Result<u64, AppError> {
        self.data_count += 1;
        let right_hash_res = self.right.append(data);
        if let Ok(_hash) = right_hash_res {
            Ok(self.hash())
        } else {
            right_hash_res
        }
    }

    pub fn pop(&mut self) -> Result<Data, SubtreeError> {
        if self.data_count == 0 {
            // eprintln!("st pop 0");
            Err(SubtreeError::Empty)
        } else if !self.right.is_empty() {
            self.data_count -= 1;
            // eprintln!("st pop 1, new data count: {}", self.data_count);
            let pop_result = self.right.pop();
            // eprintln!("right dcount: {}", self.right.len());
            match pop_result {
                Ok(data) => {
                    if self.right.is_empty() {
                        Err(SubtreeError::RightLeafEmpty(data))
                    } else {
                        // eprintln!("nhash: {}", self.hash);
                        self.hash();
                        // eprintln!("nhash: {}", self.hash);
                        Ok(data)
                    }
                }
                Err(app_error) => match app_error {
                    AppError::ContentEmpty => Err(SubtreeError::Empty),
                    AppError::DatatypeMismatch => Err(SubtreeError::DatatypeMismatch),
                    other => {
                        panic!("This should not happen!: {}", other);
                    }
                },
            }
        } else {
            // right is empty
            //
            // TODO: we need to make sure that after pop this Subtree
            // is converted into ContentTree, since it's right side is empty
            self.data_count -= 1;
            // eprintln!("st pop 2, new data count: {}", self.data_count);
            let pop_result = self.left.pop();
            // eprintln!("right dcount: {}", self.right.len());
            match pop_result {
                Ok(data) => {
                    if self.left.is_empty() {
                        Err(SubtreeError::BothLeavesEmpty(data))
                    } else {
                        Err(SubtreeError::RightLeafEmpty(data))
                    }
                }
                Err(app_error) => match app_error {
                    AppError::ContentEmpty => Err(SubtreeError::Empty),
                    AppError::DatatypeMismatch => Err(SubtreeError::DatatypeMismatch),
                    other => {
                        panic!("This should not happen: {:?}!", other);
                    }
                },
            }
        }
    }

    pub fn insert(&mut self, idx: u16, mut data: Data) -> Result<u64, AppError> {
        if idx >= self.data_count {
            return Err(AppError::IndexingError);
        }
        let left_count = self.left.len();
        let right_count = self.left.len();

        if idx < left_count {
            for i in idx..left_count {
                data = self.replace(i, data).unwrap();
            }
            for i in 0..right_count {
                data = self.replace(i, data).unwrap();
            }
        } else {
            for i in idx..right_count {
                data = self.replace(i, data).unwrap();
            }
        }
        let result = self.append(data);
        self.hash();
        result
    }
}

pub fn double_hash(num_one: u64, num_two: u64) -> u64 {
    // let mut hasher = DefaultHasher::new();
    // [num_one, num_two].hash(&mut hasher);
    // hasher.finish()
    let mut bytes = Vec::with_capacity(16);
    for byte in num_one.to_be_bytes() {
        bytes.push(byte);
    }
    for byte in num_two.to_be_bytes() {
        bytes.push(byte);
    }
    sha_hash(&bytes)
}
