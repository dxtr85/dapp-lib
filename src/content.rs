use gnome::prelude::*;
use std::cmp::Ordering;
use std::hash::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
pub type ContentID = u16;
pub type DataType = u8;

pub enum Content {
    Link(GnomeId, String, ContentID),
    Data(DataType, Option<ContentTree>),
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

pub enum ContentTree {
    // Phantom(u64) // when we only store hashes, not Data for some reason
    Empty,
    Filled(Data),
    Hashed(Box<Subtree>),
}

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

    pub fn empty() -> Self {
        ContentTree::Empty
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    pub fn len(&self) -> u16 {
        match self {
            Self::Empty => 0,
            Self::Filled(_d) => 1,
            Self::Hashed(sub_tree) => sub_tree.len(),
        }
    }

    pub fn hash(&self) -> u64 {
        match self {
            Self::Empty => 0,
            Self::Filled(data) => data.hash(),
            Self::Hashed(sub_tree) => sub_tree.hash,
        }
    }

    pub fn read(&self, idx: u16) -> Result<Data, ()> {
        match self {
            Self::Filled(data) => {
                if idx == 0 {
                    Ok(data.clone())
                } else {
                    Err(())
                }
            }
            Self::Hashed(sub_tree) => sub_tree.read(idx),
            Self::Empty => Err(()),
        }
    }

    pub fn replace(&mut self, idx: u16, new_data: Data) -> Result<Data, ()> {
        match self {
            Self::Filled(data) => {
                if idx == 0 {
                    let old_data: Data = std::mem::replace(data, new_data);
                    Ok(old_data)
                } else {
                    Err(())
                }
            }
            Self::Hashed(sub_tree) => sub_tree.read(idx),
            Self::Empty => Err(()),
        }
    }
    pub fn append(&mut self, data: Data) -> Result<u64, ()> {
        let hash = data.hash();
        // we only balance a tree every time we are inserting data
        // appending has it's own logic regarding tree structure
        match self {
            Self::Filled(existing_data) => {
                let first_hash = existing_data.hash();
                let d_hash = double_hash(first_hash, hash);
                let prev_tree = std::mem::replace(self, Self::Empty);
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
                        let prev_tree = std::mem::replace(self, Self::Empty);
                        let new_tree = ContentTree::Hashed(Box::new(Subtree {
                            data_count,
                            hash: d_hash,
                            left: prev_tree,
                            right: ContentTree::Filled(data),
                        }));
                        let _ = std::mem::replace(self, new_tree);
                        Ok(d_hash)
                    } else {
                        Ok(subtree.append(data))
                    }
                } else {
                    Err(())
                }
            }
            Self::Empty => {
                *self = ContentTree::Filled(data);
                Ok(hash)
            }
        }
    }

    pub fn insert(&mut self, idx: u16, data: Data, overwrite: bool) -> Result<u64, ()> {
        // Use append in this case
        if idx >= self.len() {
            return Err(());
        }
        let remove_last = !overwrite && self.len() == u16::MAX;
        let result = match self {
            Self::Empty => Err(()),
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
                    Err(())
                }
            }
            Self::Hashed(subtree) => subtree.insert(idx, data, overwrite),
        };
        if remove_last && result.is_ok() {
            let _ = self.remove(u16::MAX);
        }
        self.balance_tree()
    }

    pub fn remove(&mut self, idx: u16) -> Result<u64, ()> {
        match self {
            Self::Empty => Err(()),
            Self::Hashed(subtree) => {
                let rem_result = subtree.remove(idx);
                match rem_result {
                    Ok(hash) => Ok(hash),
                    Err(Some(data)) => {
                        if data.len() == 0 {
                            *self = ContentTree::empty();
                            Ok(0)
                        } else {
                            let hash = data.hash();
                            *self = Self::Filled(data);
                            Ok(hash)
                        }
                    }
                    Err(None) => Err(()),
                }
            }
            Self::Filled(_d) => {
                if idx == 0 {
                    *self = ContentTree::empty();
                    Ok(0)
                } else {
                    Err(())
                }
            }
        }
    }

    fn balance_tree(&mut self) -> Result<u64, ()> {
        match self {
            Self::Empty => Ok(0),
            Self::Filled(data) => Ok(data.hash()),
            Self::Hashed(subtree) => subtree.balance_tree(),
        }
    }

    fn take_first_n(&mut self, count: u16) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Filled(_data) => std::mem::replace(self, Self::Empty),
            Self::Hashed(subtree) => {
                let taken = subtree.take_first_n(count);
                if subtree.len() == 0 {
                    *self = Self::Empty;
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
            Self::Empty => Self::Empty,
            Self::Filled(_data) => std::mem::replace(self, Self::Empty),
            Self::Hashed(subtree) => {
                let taken = subtree.take_last_n(count);
                if subtree.len() == 0 {
                    *self = Self::Empty;
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

    fn append_tree(&mut self, mut tree: ContentTree) -> Result<u64, ()> {
        match self {
            Self::Empty => {
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
            Self::Empty => {
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
    pub fn read(&self, idx: u16) -> Result<Data, ()> {
        if idx >= self.data_count {
            Err(())
        } else {
            let left_count = self.left.len();
            if idx >= left_count {
                self.right.read(idx - left_count)
            } else {
                self.left.read(idx)
            }
        }
    }

    pub fn replace(&mut self, idx: u16, new_data: Data) -> Result<Data, ()> {
        if idx >= self.data_count {
            Err(())
        } else {
            let left_count = self.left.len();
            if idx >= left_count {
                self.right.replace(idx - left_count, new_data)
            } else {
                self.left.replace(idx, new_data)
            }
        }
    }
    pub fn append(&mut self, data: Data) -> u64 {
        self.data_count += 1;
        let right_hash = data.hash();
        let left_hash = self.left.hash();
        let _ = self.right.append(data);
        double_hash(left_hash, right_hash)
    }

    pub fn insert(&mut self, idx: u16, data: Data, overwrite: bool) -> Result<u64, ()> {
        if idx >= self.data_count {
            return Err(());
        }
        let left_count = self.left.len();
        if idx >= left_count {
            self.right.insert(idx - left_count, data, overwrite)
        } else {
            self.left.insert(idx, data, overwrite)
        }
    }

    /// We remove element from a Subtree
    /// if entire structure does not change we return Ok(hash)
    /// however if we need to change the structure
    /// we return Err(Some(data)) - upper structure needs to take care of it
    /// if we failed to remove an item we return Err(None)
    pub fn remove(&mut self, idx: u16) -> Result<u64, Option<Data>> {
        if idx >= self.data_count {
            println!(
                "(index to remove)  {} >= {} (data len) ",
                idx, self.data_count
            );
            Err(None)
        } else {
            let left_count = self.left.len();
            self.data_count -= 1;
            if idx >= left_count {
                let _ = self.right.remove(idx - left_count);
                if self.right.is_empty() {
                    let curr_left = std::mem::replace(&mut self.left, ContentTree::empty());
                    match curr_left {
                        ContentTree::Empty => {
                            // this case is not expected to happen
                            // but if it does nothing should happen ;P
                            Err(Some(Data::empty()))
                        }
                        ContentTree::Filled(data) => Err(Some(data)),
                        ContentTree::Hashed(boxed_subtree) => {
                            let _ = std::mem::replace(self, *boxed_subtree);
                            Ok(self.hash)
                        }
                    }
                } else {
                    Ok(self.hash)
                }
            } else {
                let _ = self.left.remove(idx);
                if self.left.is_empty() {
                    let curr_right = std::mem::replace(&mut self.right, ContentTree::empty());
                    match curr_right {
                        ContentTree::Empty => {
                            // this case is not expected to happen
                            Err(Some(Data::empty()))
                        }
                        ContentTree::Filled(data) => Err(Some(data)),
                        ContentTree::Hashed(boxed_subtree) => {
                            *self = *boxed_subtree;
                            Ok(self.hash)
                        }
                    }
                } else {
                    Ok(self.hash)
                }
            }
        }
    }

    fn balance_tree(&mut self) -> Result<u64, ()> {
        let left_len = self.left.len();
        let right_len = self.right.len();
        let diff = u16::abs_diff(left_len, right_len);
        if diff >= 2 {
            let take_count = if diff % 2 != 0 {
                1 + (diff >> 1)
            } else {
                diff >> 1
            };
            if right_len > left_len {
                let taken = self.right.take_first_n(take_count);
                // TODO
                let _ = self.left.append_tree(taken);
            } else {
                let taken = self.left.take_last_n(take_count);
                // TODO
                let _ = self.right.prepend_tree(taken);
            }
            // We could use results from following expressions...
            let _ = self.left.balance_tree();
            let _ = self.right.balance_tree();
            // but for now we simply recalculate hash
            Ok(self.hash())
        } else {
            // Do not recalculate hash
            Ok(self.hash)
        }
    }
    fn take_first_n(&mut self, count: u16) -> ContentTree {
        let left_count = self.left.len();
        match left_count.cmp(&count) {
            Ordering::Greater => self.left.take_first_n(count),
            Ordering::Less => {
                let mut tmp = std::mem::replace(&mut self.left, ContentTree::Empty);
                let more = self.right.take_first_n(count - left_count);
                let curr_right = std::mem::replace(&mut self.right, ContentTree::empty());
                match curr_right {
                    ContentTree::Empty => {
                        self.data_count = 0;
                    }
                    ContentTree::Filled(_data) => {
                        self.data_count = 1;
                    }
                    ContentTree::Hashed(subtree) => *self = *subtree,
                };
                let _ = tmp.append_tree(more);
                tmp
            }
            Ordering::Equal => {
                let tmp = std::mem::replace(&mut self.left, ContentTree::Empty);
                let curr_right = std::mem::replace(&mut self.right, ContentTree::empty());
                match curr_right {
                    ContentTree::Empty => {
                        self.data_count = 0;
                    }
                    ContentTree::Filled(_data) => {
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
            Ordering::Greater => self.right.take_last_n(count),
            Ordering::Less => {
                let tmp = std::mem::replace(&mut self.right, ContentTree::Empty);
                let mut more = self.left.take_last_n(count - right_count);
                // *self = self.left;
                let curr_left = std::mem::replace(&mut self.left, ContentTree::empty());
                match curr_left {
                    ContentTree::Empty => {
                        self.data_count = 0;
                    }
                    ContentTree::Filled(_data) => {
                        self.data_count = 1;
                    }
                    ContentTree::Hashed(subtree) => *self = *subtree,
                };
                let _ = more.append_tree(tmp);
                more
            }
            Ordering::Equal => {
                let tmp = std::mem::replace(&mut self.right, ContentTree::Empty);
                // *self = self.left;
                let curr_left = std::mem::replace(&mut self.left, ContentTree::empty());
                match curr_left {
                    ContentTree::Empty => {
                        self.data_count = 0;
                    }
                    ContentTree::Filled(_data) => {
                        self.data_count = 1;
                    }
                    ContentTree::Hashed(subtree) => *self = *subtree,
                };
                tmp
            }
        }
    }

    fn append_tree(&mut self, tree: ContentTree) -> Result<u64, ()> {
        self.right.append_tree(tree)
    }
    fn prepend_tree(&mut self, tree: ContentTree) -> Result<u64, ()> {
        self.left.prepend_tree(tree)
    }
}
fn double_hash(num_one: u64, num_two: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    [num_one, num_two].hash(&mut hasher);
    hasher.finish()
}
