use gnome::prelude::*;
use std::cmp::Ordering;
use std::hash::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;

use crate::prelude::AppError;
pub type ContentID = u16;
pub type DataType = u8;

#[derive(Debug)]
pub enum Content {
    // TODO: Link could also contain a non-hashed description
    Link(GnomeId, String, ContentID),
    Data(DataType, ContentTree),
}

impl Content {
    pub fn from(data: Data) -> Result<Self, ()> {
        let mut bytes_iter = data.bytes().into_iter();
        match bytes_iter.next() {
            None => Err(()),
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
                // for now, remaining bytes is SwarmName
                let swarm_name: String = String::from_utf8(bytes_iter.collect()).unwrap();
                Ok(Content::Link(g_id, swarm_name, c_id))
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
            Self::Link(_g, _sn, _c) => 255,
            Self::Data(d_type, _ct) => *d_type,
        }
    }
    pub fn read_data(&self, data_id: u16) -> Result<Data, AppError> {
        match self {
            Self::Link(g_id, s_name, c_id) => {
                if data_id == 0 {
                    Ok(link_to_data(*g_id, s_name.clone(), *c_id))
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
            Self::Link(_g, _s, _c) => Err(AppError::DatatypeMismatch),
            Self::Data(_dt, tree) => tree.append(data),
        }
    }
    pub fn pop_data(&mut self) -> Result<Data, AppError> {
        match self {
            Self::Link(_g, _s, _c) => Err(AppError::DatatypeMismatch),
            Self::Data(_dt, tree) => {
                if tree.is_empty() {
                    Err(AppError::ContentEmpty)
                } else {
                    Ok(tree.pop())
                }
            }
        }
    }

    pub fn insert(&mut self, d_id: u16, data: Data, overwrite: bool) -> Result<u64, AppError> {
        match self {
            Self::Link(_g, _s, _c) => Err(AppError::DatatypeMismatch),
            Self::Data(_dt, tree) => tree.insert(d_id, data, overwrite),
        }
    }
    pub fn update_data(&mut self, data_id: u16, data: Data) -> Result<Data, AppError> {
        let myself = std::mem::replace(self, Content::Data(0, ContentTree::Empty(0)));
        match myself {
            Self::Link(g_id, s_name, c_id) => {
                if data_id != 0 {
                    *self = Self::Link(g_id, s_name, c_id);
                    Err(AppError::IndexingError)
                } else {
                    let link_result = data_to_link(data);
                    if let Ok(link) = link_result {
                        *self = link;
                        Ok(link_to_data(g_id, s_name, c_id))
                    } else {
                        *self = Self::Link(g_id, s_name, c_id);
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
            Self::Link(_g, _s, _c) => Err(AppError::DatatypeMismatch),
            Self::Data(_dt, tree) => tree.remove_data(d_id),
        }
    }

    pub fn shell(&self) -> Self {
        match self {
            Self::Link(g_id, s_name, c_id) => Self::Link(*g_id, s_name.clone(), *c_id),
            Self::Data(d_type, c_tree) => Self::Data(*d_type, c_tree.shell()),
        }
    }
    pub fn hash(&self) -> u64 {
        match self {
            Self::Link(g_id, string, c_id) => {
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
    fn get_data_hash(&self, d_id: u16) -> Result<u64, ()> {
        match self {
            Self::Link(_g, _s, _c) => {
                if d_id == 0 {
                    Ok(link_to_data(*_g, _s.clone(), *_c).hash())
                } else {
                    Err(())
                }
            }
            Self::Data(_type, c_tree) => c_tree.get_data_hash(d_id),
        }
    }
}

fn data_to_link(data: Data) -> Result<Content, AppError> {
    let mut bytes = data.bytes();
    let len = bytes.len();
    if len < 11 {
        return Err(AppError::Smthg);
    }
    let first_eight = [
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ];
    let next_two = [bytes[8], bytes[9]];

    let _ = bytes.iter_mut().skip(10);
    let g_id = GnomeId::from(first_eight);
    let s_res = String::from_utf8(bytes);
    if s_res.is_err() {
        return Err(AppError::Smthg);
    }
    let s_name = s_res.unwrap();
    let c_id = u16::from_be_bytes(next_two);
    Ok(Content::Link(g_id, s_name, c_id))
}

fn link_to_data(g_id: GnomeId, s_name: String, c_id: ContentID) -> Data {
    let mut v = vec![];
    for b in g_id.bytes() {
        v.push(b);
    }
    for b in c_id.to_be_bytes() {
        v.push(b);
    }
    for b in s_name.into_bytes() {
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
            tree.append(data);
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
    pub fn get_data_hash(&self, data_id: u16) -> Result<u64, ()> {
        match self {
            Self::Empty(hash) => {
                if data_id == 0 {
                    Ok(*hash)
                } else {
                    Err(())
                }
            }
            Self::Filled(data) => {
                if data_id == 0 {
                    Ok(data.hash())
                } else {
                    Err(())
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

    pub fn insert(&mut self, idx: u16, data: Data, overwrite: bool) -> Result<u64, AppError> {
        // Use append in this case
        if idx >= self.len() {
            return Err(AppError::IndexingError);
        }
        let remove_last = !overwrite && self.len() == u16::MAX;
        let result = match self {
            Self::Empty(_hash) => Err(AppError::Smthg),
            Self::Filled(old_data) => {
                if idx == 0 {
                    if overwrite {
                        let hash = data.hash();
                        *self = Self::Filled(data);
                        Ok(hash)
                    } else {
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
                    }
                } else {
                    // This should not happen, but anyway...
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(subtree) => subtree.insert(idx, data, overwrite),
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

    // fn balance_tree(&mut self) -> Result<u64, ()> {
    //     match self {
    //         Self::Empty(_hash) => Ok(0),
    //         Self::Filled(data) => Ok(data.hash()),
    //         Self::Hashed(subtree) => subtree.balance_tree(),
    //     }
    // }

    fn take_first_n(&mut self, count: u16) -> Self {
        match self {
            Self::Empty(hash) => Self::Empty(*hash),
            Self::Filled(_data) => std::mem::replace(self, Self::Empty(0)),
            Self::Hashed(subtree) => {
                let taken = subtree.take_first_n(count);
                if subtree.len() == 0 {
                    *self = Self::Empty(0);
                } else if subtree.len() == 1 {
                    if let Ok(data) = subtree.replace(0, Data::empty()) {
                        *self = Self::Filled(data);
                    } else {
                        println!("Something went wrong in take_first_n")
                    }
                }
                taken
            }
        }
    }
    fn take_last_n(&mut self, count: u16) -> Self {
        match self {
            Self::Empty(hash) => Self::Empty(*hash),
            Self::Filled(_data) => std::mem::replace(self, Self::Empty(0)),
            Self::Hashed(subtree) => {
                let taken = subtree.take_last_n(count);
                if subtree.len() == 0 {
                    *self = Self::Empty(0);
                } else if subtree.len() == 1 {
                    if let Ok(data) = subtree.replace(0, Data::empty()) {
                        *self = Self::Filled(data);
                    } else {
                        println!("Something went wrong in take_last_n")
                    }
                }
                taken
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

    fn append_tree(&mut self, mut tree: ContentTree) -> Result<u64, ()> {
        match self {
            Self::Empty(_hash) => {
                *self = tree;
                Ok(self.hash())
            }
            Self::Filled(data) => {
                let _ = tree.insert(0, data.clone(), true);
                *self = tree;
                Ok(self.hash())
            }
            Self::Hashed(subtree) => subtree.append_tree(tree),
        }
    }

    fn prepend_tree(&mut self, mut tree: Self) -> Result<u64, ()> {
        match self {
            Self::Empty(_hash) => {
                *self = tree;
                Ok(self.hash())
            }
            Self::Filled(data) => {
                let _ = tree.append(data.clone());
                *self = tree;
                Ok(self.hash())
            }
            Self::Hashed(subtree) => subtree.prepend_tree(tree),
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
    pub fn get_data_hash(&self, idx: u16) -> Result<u64, ()> {
        if idx >= self.data_count {
            Err(())
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
        } else if self.right.len() > 0 {
            self.data_count -= 1;
            self.right.pop()
        } else {
            // TODO: we need to make sure that after pop this Subtree
            // is converted into ContentTree, since it's right side is empty
            self.data_count -= 1;
            self.left.pop()
        }
    }

    pub fn insert(&mut self, idx: u16, data: Data, overwrite: bool) -> Result<u64, AppError> {
        if idx >= self.data_count {
            return Err(AppError::IndexingError);
        }
        let left_count = self.left.len();
        if idx >= left_count {
            let ins_res = self.right.insert(idx - left_count, data, overwrite);
            if let Ok(_hash) = ins_res {
                Ok(self.hash())
            } else {
                ins_res
            }
        } else {
            let ins_res = self.left.insert(idx, data, overwrite);
            if let Ok(_hash) = ins_res {
                Ok(self.hash())
            } else {
                ins_res
            }
        }
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

    // OLD REMOVE
    // pub fn remove(&mut self, idx: u16) -> Result<Data, AppError> {
    //     if idx >= self.data_count {
    //         println!(
    //             "(index to remove)  {} >= {} (data len) ",
    //             idx, self.data_count
    //         );
    //         Err(AppError::IndexingError)
    //     } else {
    //         let left_count = self.left.len();
    //         self.data_count -= 1;
    //         if idx >= left_count {
    //             let rem_result = self.right.remove(idx - left_count);
    //             if self.right.is_empty() {
    //                 let curr_left = std::mem::replace(&mut self.left, ContentTree::empty(0));
    //                 match curr_left {
    //                     ContentTree::Empty(_hash) => {
    //                         // this case is not expected to happen
    //                         // but if it does nothing should happen ;P
    //                         Err(AppError::ContentEmpty)
    //                     }
    //                     ContentTree::Filled(data) => Err(Some(data)),
    //                     ContentTree::Hashed(boxed_subtree) => {
    //                         let _ = std::mem::replace(self, *boxed_subtree);
    //                         rem_result
    //                     }
    //                 }
    //             } else if let Ok(data) = rem_result {
    //                 self.hash();
    //                 Ok(data)
    //             } else {
    //                 Err(None)
    //             }
    //         } else {
    //             let rem_result = self.left.remove(idx);
    //             if self.left.is_empty() {
    //                 let curr_right = std::mem::replace(&mut self.right, ContentTree::empty(0));
    //                 match curr_right {
    //                     ContentTree::Empty(_hash) => {
    //                         // this case is not expected to happen
    //                         Err(Some(Data::empty()))
    //                     }
    //                     ContentTree::Filled(data) => Err(Some(data)),
    //                     ContentTree::Hashed(boxed_subtree) => {
    //                         *self = *boxed_subtree;
    //                         rem_result
    //                     }
    //                 }
    //             } else if let Ok(data) = rem_result {
    //                 self.hash();
    //                 Ok(data)
    //             } else {
    //                 Err(None)
    //             }
    //         }
    //     }
    // }

    // In order to make hashing work we can never balance a tree!
    // fn balance_tree(&mut self) -> Result<u64, ()> {
    //     let left_len = self.left.len();
    //     let right_len = self.right.len();
    //     let diff = u16::abs_diff(left_len, right_len);
    //     if diff >= 2 {
    //         let take_count = if diff % 2 != 0 {
    //             1 + (diff >> 1)
    //         } else {
    //             diff >> 1
    //         };
    //         if right_len > left_len {
    //             let taken = self.right.take_first_n(take_count);
    //             let _ = self.left.append_tree(taken);
    //         } else {
    //             let taken = self.left.take_last_n(take_count);
    //             let _ = self.right.prepend_tree(taken);
    //         }
    //         // We could use results from following expressions...
    //         let _ = self.left.balance_tree();
    //         let _ = self.right.balance_tree();
    //         // but for now we simply recalculate hash
    //         Ok(self.hash())
    //     } else {
    //         // Do not recalculate hash
    //         Ok(self.hash)
    //     }
    // }
    fn take_first_n(&mut self, count: u16) -> ContentTree {
        let left_count = self.left.len();
        match left_count.cmp(&count) {
            Ordering::Greater => {
                let taken = self.left.take_first_n(count);
                self.hash();
                taken
            }
            Ordering::Less => {
                let mut tmp = std::mem::replace(&mut self.left, ContentTree::empty(0));
                let more = self.right.take_first_n(count - left_count);
                let curr_right = std::mem::replace(&mut self.right, ContentTree::empty(0));
                match curr_right {
                    ContentTree::Empty(_hash) => {
                        // self.hash = hash;
                        self.data_count = 0;
                    }
                    ContentTree::Filled(_data) => {
                        // self.hash=data.hash();
                        self.data_count = 1;
                    }
                    ContentTree::Hashed(subtree) => *self = *subtree,
                };
                let _ = tmp.append_tree(more);
                tmp
            }
            Ordering::Equal => {
                let tmp = std::mem::replace(&mut self.left, ContentTree::empty(0));
                let curr_right = std::mem::replace(&mut self.right, ContentTree::empty(0));
                match curr_right {
                    ContentTree::Empty(_hash) => {
                        // Not sure about those hashes
                        // self.hash = hash;
                        self.data_count = 0;
                    }
                    ContentTree::Filled(_data) => {
                        // Not sure about those hashes
                        // self.hash = data.hash();
                        self.data_count = 1;
                    }
                    ContentTree::Hashed(subtree) => *self = *subtree,
                };
                tmp
            }
        }
    }

    fn take_last_n(&mut self, count: u16) -> ContentTree {
        let right_count = self.right.len();
        match right_count.cmp(&count) {
            Ordering::Greater => {
                let taken = self.right.take_last_n(count);
                self.hash();
                taken
            }
            Ordering::Less => {
                let tmp = std::mem::replace(&mut self.right, ContentTree::empty(0));
                let mut more = self.left.take_last_n(count - right_count);
                let curr_left = std::mem::replace(&mut self.left, ContentTree::empty(0));
                match curr_left {
                    ContentTree::Empty(_hash) => {
                        // self.hash = hash;
                        self.data_count = 0;
                    }
                    ContentTree::Filled(_data) => {
                        // self.hash = data.hash();
                        self.data_count = 1;
                    }
                    ContentTree::Hashed(subtree) => *self = *subtree,
                };
                let _ = more.append_tree(tmp);
                more
            }
            Ordering::Equal => {
                let tmp = std::mem::replace(&mut self.right, ContentTree::empty(0));
                let curr_left = std::mem::replace(&mut self.left, ContentTree::empty(0));
                match curr_left {
                    ContentTree::Empty(_hash) => {
                        // self.hash = hash;
                        self.data_count = 0;
                    }
                    ContentTree::Filled(_data) => {
                        // self.hash = data.hash();
                        self.data_count = 1;
                    }
                    ContentTree::Hashed(subtree) => *self = *subtree,
                };
                tmp
            }
        }
    }

    fn append_tree(&mut self, tree: ContentTree) -> Result<u64, ()> {
        let result = self.right.append_tree(tree);
        if let Ok(_h) = result {
            Ok(self.hash())
        } else {
            result
        }
    }
    fn prepend_tree(&mut self, tree: ContentTree) -> Result<u64, ()> {
        let result = self.left.prepend_tree(tree);
        if let Ok(_h) = result {
            Ok(self.hash())
        } else {
            result
        }
    }
}

pub fn double_hash(num_one: u64, num_two: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    [num_one, num_two].hash(&mut hasher);
    hasher.finish()
}
