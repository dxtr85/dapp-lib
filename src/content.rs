use gnome::prelude::*;
use std::collections::HashMap;
use std::hash::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;

use crate::prelude::AppError;
pub type ContentID = u16;
pub type DataType = u8;

#[derive(Debug)]
pub enum Content {
    // TODO: Link could also contain a non-hashed description
    Link(GnomeId, String, ContentID, Option<TransformInfo>),
    Data(DataType, ContentTree),
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
type Tag = HashMap<u8, String>;

#[derive(Debug, Clone)]
pub struct TransformInfo {
    pub d_type: DataType,
    pub tags: Vec<u8>,
    pub size: u16,
    pub root_hash: u64,
    pub broadcast_id: CastID,
    pub description: String,
    pub missing_hashes: Vec<u16>,
    pub data_hashes: Vec<Data>,
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
        let mut bytes_iter = bytes.into_iter();
        if let Some(d_type) = bytes_iter.next() {
            let b1 = bytes_iter.next().unwrap();
            let b2 = bytes_iter.next().unwrap();
            let size = u16::from_be_bytes([b1, b2]);
            let tags_len = bytes_iter.next().unwrap();
            let mut tags = Vec::with_capacity(tags_len as usize);
            for _i in 0..tags_len {
                tags.push(bytes_iter.next().unwrap());
            }
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
            let description = String::from_utf8(bytes_iter.collect()).unwrap();
            Some(TransformInfo {
                d_type,
                tags,
                size,
                root_hash,
                broadcast_id,
                description,
                missing_hashes: vec![],
                data_hashes: vec![],
            })
        } else {
            None
        }
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
    pub fn from(data: Data) -> Result<Self, AppError> {
        let bytes = data.bytes();
        println!("Bytes: {:?}", bytes);
        let mut bytes_iter = bytes.into_iter();
        let first_byte = bytes_iter.next();
        println!("First byte: {:?}", first_byte);
        match first_byte {
            None => Err(AppError::Smthg),
            Some(255) => {
                // first 8 bytes is GnomeId
                let b1 = bytes_iter.next().unwrap();
                let b2 = bytes_iter.next().unwrap();
                let b3 = bytes_iter.next().unwrap();
                let b4 = bytes_iter.next().unwrap();
                let b5 = bytes_iter.next().unwrap();
                let b6 = bytes_iter.next().unwrap();
                let b7 = bytes_iter.next().unwrap();
                let b8 = bytes_iter.next().unwrap();
                let g_id = GnomeId(u64::from_be_bytes([b1, b2, b3, b4, b5, b6, b7, b8]));
                // next 2 bytes is ContentID
                let b1 = bytes_iter.next().unwrap();
                let b2 = bytes_iter.next().unwrap();
                let c_id = u16::from_be_bytes([b1, b2]);
                let name_len = bytes_iter.next().unwrap();
                let mut name_vec = Vec::with_capacity(name_len as usize);
                for _i in 0..name_len {
                    name_vec.push(bytes_iter.next().unwrap());
                }
                // for now, remaining bytes is SwarmName
                let swarm_name: String = String::from_utf8(name_vec).unwrap();
                let ti = TransformInfo::from(bytes_iter.collect());
                Ok(Content::Link(g_id, swarm_name, c_id, ti))
            }
            // TODO: we can define instructions to create hollow Content
            // containing only hashes of either Data or non-data hashes
            // at certain hash pyramid floor (counted from top to bottom)
            // TODO: we can also define additional instruction to substitute
            // selected bottom non-data leaf with another hash pyramid that contains
            // at it's bottom root data hashes
            // TODO: once we have all root data hashes we can start
            // replacing them one-by-one
            Some(other) => {
                let tree = ContentTree::new(Data::new(bytes_iter.collect()).unwrap());
                Ok(Content::Data(other, tree))
            }
        }
    }
    pub fn data_type(&self) -> DataType {
        match self {
            Self::Link(_g, _sn, _c, _ti) => 255,
            Self::Data(d_type, _ct) => *d_type,
        }
    }
    pub fn link_to_data(&self) -> Option<Data> {
        match self {
            Content::Link(g_id, s_name, c_id, ti_opt) => {
                Some(link_to_data(*g_id, s_name.clone(), *c_id, &ti_opt))
            }
            Content::Data(_d_type, _c_tree) => None,
        }
    }
    pub fn read_data(&self, data_id: u16) -> Result<Data, AppError> {
        match self {
            Self::Link(g_id, s_name, c_id, ti) => {
                if data_id == 0 {
                    Ok(link_to_data(*g_id, s_name.clone(), *c_id, &ti))
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Data(_type, c_tree) => {
                if data_id == 0 {
                    c_tree.read(data_id)
                } else {
                    Err(AppError::IndexingError)
                }
            }
        }
    }
    pub fn update(&mut self, content: Content) -> Content {
        std::mem::replace(self, content)
    }

    pub fn push_data(&mut self, data: Data) -> Result<u64, AppError> {
        match self {
            Self::Link(_g, _s, _c, _ti) => Err(AppError::DatatypeMismatch),
            Self::Data(_dt, tree) => tree.append(data),
        }
    }
    pub fn pop_data(&mut self) -> Result<Data, AppError> {
        match self {
            Self::Link(_g, _s, _c, _ti) => Err(AppError::DatatypeMismatch),
            Self::Data(_dt, tree) => {
                if tree.is_empty() {
                    Err(AppError::ContentEmpty)
                } else {
                    Ok(tree.pop())
                }
            }
        }
    }

    pub fn insert(&mut self, d_id: u16, data: Data) -> Result<u64, AppError> {
        match self {
            Self::Link(_g, _s, _c, _ti) => Err(AppError::DatatypeMismatch),
            Self::Data(_dt, tree) => tree.insert(d_id, data),
        }
    }
    pub fn update_data(&mut self, data_id: u16, data: Data) -> Result<Data, AppError> {
        let myself = std::mem::replace(self, Content::Data(0, ContentTree::Empty(0)));
        match myself {
            Self::Link(g_id, s_name, c_id, ti) => {
                if data_id != 0 {
                    *self = Self::Link(g_id, s_name, c_id, ti);
                    Err(AppError::IndexingError)
                } else {
                    let link_result = data_to_link(data);
                    if let Ok(link) = link_result {
                        *self = link;
                        Ok(link_to_data(g_id, s_name, c_id, &ti))
                    } else {
                        *self = Self::Link(g_id, s_name, c_id, ti);
                        Err(link_result.err().unwrap())
                    }
                }
            }
            Self::Data(d_type, mut c_tree) => {
                let result = c_tree.replace(data_id, data);
                *self = Self::Data(d_type, c_tree);
                // if let Ok(old_data) = result {
                //     Ok(old_data)
                // } else {
                //     Err(result.err().unwrap())
                // }
                result
            }
        }
    }
    pub fn remove_data(&mut self, d_id: u16) -> Result<Data, AppError> {
        match self {
            Self::Link(_g, _s, _c, _ti) => Err(AppError::DatatypeMismatch),
            Self::Data(_dt, tree) => tree.remove_data(d_id),
        }
    }

    pub fn shell(&self) -> Self {
        match self {
            Self::Link(g_id, s_name, c_id, ti) => {
                Self::Link(*g_id, s_name.clone(), *c_id, ti.clone())
            }
            Self::Data(d_type, c_tree) => Self::Data(*d_type, c_tree.shell()),
        }
    }
    pub fn hash(&self) -> u64 {
        match self {
            Self::Link(g_id, string, c_id, _ti) => {
                let mut b_vec = vec![];
                for byte in g_id.bytes() {
                    b_vec.push(byte);
                }
                for byte in string.bytes() {
                    b_vec.push(byte);
                }
                for byte in c_id.to_be_bytes() {
                    b_vec.push(byte);
                }
                let mut hasher = DefaultHasher::new();
                b_vec.hash(&mut hasher);
                hasher.finish()
            }
            Self::Data(_type, tree) => tree.hash(),
        }
    }
    pub fn data_hashes(&self) -> Vec<u64> {
        let mut v = vec![];
        for d_id in 0..u16::MAX {
            if let Ok(hash) = self.get_data_hash(d_id) {
                v.push(hash)
            } else {
                break;
            }
        }
        v
    }
    fn get_data_hash(&self, d_id: u16) -> Result<u64, AppError> {
        match self {
            Self::Link(_g, _s, _c, _ti) => {
                if d_id == 0 {
                    Ok(link_to_data(*_g, _s.clone(), *_c, &_ti).hash())
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Data(_type, c_tree) => c_tree.get_data_hash(d_id),
        }
    }
}

fn data_to_link(data: Data) -> Result<Content, AppError> {
    let mut bytes_iter = data.bytes().into_iter();
    let len = bytes_iter.len();
    if len < 11 {
        return Err(AppError::Smthg);
    }
    let first_eight = [
        bytes_iter.next().unwrap(),
        bytes_iter.next().unwrap(),
        bytes_iter.next().unwrap(),
        bytes_iter.next().unwrap(),
        bytes_iter.next().unwrap(),
        bytes_iter.next().unwrap(),
        bytes_iter.next().unwrap(),
        bytes_iter.next().unwrap(),
    ];
    let next_two = [bytes_iter.next().unwrap(), bytes_iter.next().unwrap()];

    let name_len = bytes_iter.next().unwrap();
    let mut name_vec = Vec::with_capacity(name_len as usize);
    for _i in 0..name_len {
        name_vec.push(bytes_iter.next().unwrap());
    }
    let g_id = GnomeId::from(first_eight);
    let s_name = String::from_utf8(name_vec).unwrap();
    let c_id = u16::from_be_bytes(next_two);

    let ti = TransformInfo::from(bytes_iter.collect());
    Ok(Content::Link(g_id, s_name, c_id, ti))
}

fn link_to_data(
    g_id: GnomeId,
    s_name: String,
    c_id: ContentID,
    // TODO: append this
    ti: &Option<TransformInfo>,
) -> Data {
    let mut v = vec![255];
    for b in g_id.bytes() {
        v.push(b);
    }
    for b in c_id.to_be_bytes() {
        v.push(b);
    }
    let s_bytes = s_name.into_bytes();
    v.push(s_bytes.len() as u8);
    for b in s_bytes {
        v.push(b);
    }
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

#[derive(Debug)]
pub enum ContentTree {
    Empty(u64),
    Filled(Data),
    Hashed(Box<Subtree>),
}

#[derive(Debug)]
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
        matches!(self, Self::Empty(_h))
    }

    pub fn len(&self) -> u16 {
        match self {
            Self::Empty(_h) => 0,
            Self::Filled(_d) => 1,
            Self::Hashed(sub_tree) => sub_tree.len(),
        }
    }

    pub fn shell(&self) -> Self {
        match self {
            Self::Empty(hash) => Self::Empty(*hash),
            Self::Filled(data) => Self::Empty(data.hash()),
            Self::Hashed(sub_tree) => Self::Empty(sub_tree.hash),
        }
    }
    pub fn hash(&self) -> u64 {
        match self {
            Self::Empty(hash) => *hash,
            Self::Filled(data) => data.hash(),
            Self::Hashed(sub_tree) => sub_tree.hash,
        }
    }
    pub fn get_data_hash(&self, data_id: u16) -> Result<u64, AppError> {
        match self {
            Self::Empty(hash) => {
                if data_id == 0 {
                    Ok(*hash)
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Filled(data) => {
                if data_id == 0 {
                    Ok(data.hash())
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(sub_tree) => sub_tree.get_data_hash(data_id),
        }
    }

    pub fn read(&self, idx: u16) -> Result<Data, AppError> {
        match self {
            Self::Filled(data) => {
                if idx == 0 {
                    Ok(data.clone())
                } else {
                    Err(AppError::Smthg)
                }
            }
            Self::Hashed(sub_tree) => sub_tree.read(idx),
            Self::Empty(_h) => Err(AppError::ContentEmpty),
        }
    }

    pub fn replace(&mut self, idx: u16, new_data: Data) -> Result<Data, AppError> {
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
                let new_hash = new_data.hash();
                if idx == 0 && *hash == new_hash {
                    *self = Self::Filled(new_data);
                    Ok(Data::empty())
                } else if idx != 0 {
                    Err(AppError::IndexingError)
                } else {
                    Err(AppError::HashMismatch)
                }
            }
        }
    }
    pub fn append(&mut self, data: Data) -> Result<u64, AppError> {
        let hash = data.hash();
        // we only balance a tree every time we are inserting data
        // appending has it's own logic regarding tree structure
        match self {
            Self::Filled(existing_data) => {
                let first_hash = existing_data.hash();
                let d_hash = double_hash(first_hash, hash);
                let prev_tree = std::mem::replace(self, Self::Empty(0));
                let new_tree = ContentTree::Hashed(Box::new(Subtree {
                    data_count: 2,
                    hash: d_hash,
                    left: prev_tree,
                    right: ContentTree::Filled(data),
                }));
                let _ = std::mem::replace(self, new_tree);
                Ok(d_hash)
            }
            Self::Hashed(subtree) => {
                //  Here we choose between two paths
                //   of growing this tree by another level
                //   by reassigning *self to new Hashed
                //   and storing in left previous self,
                //   and setting right to Filled,
                //   but this way the tree will become unbalanced
                if subtree.can_grow() {
                    if subtree.should_extend() {
                        let data_count = subtree.len() + 1;
                        let subtree_hash = subtree.hash;
                        let d_hash = double_hash(subtree_hash, hash);
                        let prev_tree = std::mem::replace(self, Self::Empty(0));
                        let new_tree = ContentTree::Hashed(Box::new(Subtree {
                            data_count,
                            hash: d_hash,
                            left: prev_tree,
                            right: ContentTree::Filled(data),
                        }));
                        let _ = std::mem::replace(self, new_tree);
                        Ok(d_hash)
                    } else {
                        let app_res = subtree.append(data);
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
                *self = ContentTree::Filled(data);
                Ok(hash)
            }
        }
    }

    pub fn insert(&mut self, idx: u16, data: Data) -> Result<u64, AppError> {
        // Use append in this case
        if idx >= self.len() {
            return Err(AppError::IndexingError);
        }
        let remove_last = self.len() == u16::MAX;
        let result = match self {
            Self::Empty(_hash) => {
                if idx == 0 {
                    let hash = data.hash();
                    *self = Self::Filled(data);
                    Ok(hash)
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Filled(old_data) => {
                if idx == 0 {
                    // if overwrite {
                    //     let hash = data.hash();
                    //     *self = Self::Filled(data);
                    //     Ok(hash)
                    // } else {
                    let left_hash = data.hash();
                    let right_hash = old_data.hash();
                    let hash = double_hash(left_hash, right_hash);
                    *self = Self::Hashed(Box::new(Subtree {
                        data_count: 2,
                        hash,
                        left: ContentTree::Filled(data),
                        right: ContentTree::Filled(old_data.clone()),
                    }));
                    Ok(hash)
                    // }
                } else {
                    // This should not happen, but anyway...
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(subtree) => subtree.insert(idx, data),
        };
        if remove_last && result.is_ok() {
            let _ = self.pop();
        }
        result
        // self.balance_tree()
    }

    pub fn remove_data(&mut self, idx: u16) -> Result<Data, AppError> {
        let myself = std::mem::replace(self, Self::empty(0));
        match myself {
            Self::Empty(_hash) => Err(AppError::ContentEmpty),
            Self::Hashed(mut subtree) => {
                let rem_result = subtree.remove_data(idx);
                let s_len = subtree.len();
                *self = if s_len == 0 {
                    Self::Empty(0)
                } else if s_len == 1 {
                    Self::Filled(subtree.pop())
                } else {
                    Self::Hashed(subtree)
                };
                rem_result
                // match rem_result {
                //     Ok(data) => Ok(data),
                //     Err(Some(data)) => {
                //         if data.len() == 0 {
                //             *self = ContentTree::empty(0);
                //             Ok(0)
                //         } else {
                //             let hash = data.hash();
                //             *self = Self::Filled(data);
                //             Ok(hash)
                //         }
                //     }
                //     Err(None) => Err(()),
                // }
            }
            Self::Filled(d) => {
                if idx == 0 {
                    Ok(d)
                } else {
                    *self = Self::Filled(d);
                    Err(AppError::IndexingError)
                }
            }
        }
    }

    fn pop(&mut self) -> Data {
        let myself = std::mem::replace(self, Self::Empty(0));
        match myself {
            Self::Empty(_hash) => Data::empty(),
            Self::Filled(data) => data,
            Self::Hashed(mut subtree) => {
                let taken = subtree.pop();
                if subtree.len() == 0 {
                } else if subtree.len() == 1 {
                    if let Ok(data) = subtree.replace(0, Data::empty()) {
                        *self = Self::Filled(data);
                    } else {
                        panic!("Something went wrong in pop")
                    }
                } else if subtree.len().count_ones() == 1 {
                    println!("Shrinking, right size: {}", subtree.right.len());
                    *self = subtree.left;
                } else {
                    *self = Self::Hashed(subtree);
                }
                taken
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
        let mut hasher = DefaultHasher::new();
        [self.left.hash(), self.right.hash()].hash(&mut hasher);
        self.hash = hasher.finish();
        self.hash
    }
    pub fn get_data_hash(&self, idx: u16) -> Result<u64, AppError> {
        if idx >= self.data_count {
            Err(AppError::IndexingError)
        } else {
            let left_count = self.left.len();
            if idx >= left_count {
                self.right.get_data_hash(idx - left_count)
            } else {
                self.left.get_data_hash(idx)
            }
        }
    }
    pub fn read(&self, idx: u16) -> Result<Data, AppError> {
        if idx >= self.data_count {
            Err(AppError::Smthg)
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

    pub fn pop(&mut self) -> Data {
        if self.data_count == 0 {
            Data::empty()
        } else if !self.right.is_empty() {
            self.data_count -= 1;
            self.right.pop()
        } else {
            // TODO: we need to make sure that after pop this Subtree
            // is converted into ContentTree, since it's right side is empty
            self.data_count -= 1;
            self.left.pop()
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
        self.append(data)
    }

    /// We remove element from a Subtree
    /// Every time we remove an element from subtree, the tree is being rebuilt.
    /// This process is required in order to keep Data in sync and all
    /// hash dependent functions to work properly.
    /// If possible it is recommended to use update(d_id, Data::empty()) instead.
    // How to make this work
    // We call pop_data to remove last element.
    // Then we call update_data starting from last but one until idx argument
    // This way we replace data under index n with what was under n+1
    // When we reach index idx we return result of this update_data call as
    // result of remove function
    pub fn remove_data(&mut self, idx: u16) -> Result<Data, AppError> {
        let chunks_count = self.len();
        if idx >= chunks_count {
            return Err(AppError::IndexingError);
        }
        let mut shifted_data = self.pop();
        for i in (idx..chunks_count).rev() {
            shifted_data = self.replace(i, shifted_data).unwrap();
        }
        Ok(shifted_data)
    }
}

pub fn double_hash(num_one: u64, num_two: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    [num_one, num_two].hash(&mut hasher);
    hasher.finish()
}
