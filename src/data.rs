// use crate::BlockID;
use gnome::prelude::sha_hash;
use gnome::prelude::CastData;
use gnome::prelude::SyncData;
// use std::hash::{DefaultHasher, Hasher};
// use std::{fmt, hash::Hash};
use std::fmt;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Data(u64, Vec<u8>);
impl Data {
    pub fn new(contents: Vec<u8>) -> Result<Self, Vec<u8>> {
        // println!("new data: {:?}", contents);
        if contents.len() > 1024 {
            return Err(contents);
        }
        // // Prefix is for later storing SwarmTime value before sign/verify
        // let mut with_prefix = vec![0, 0, 0, 0];
        // with_prefix.append(&mut contents);
        // Ok(Self(with_prefix))

        // Ok(Self(hasher.finish(), contents))
        Ok(Self(sha_hash(&contents), contents))
    }

    pub fn empty(hash: u64) -> Self {
        // eprintln!("new empty {}", hash);
        Data(hash, vec![])
    }

    pub fn is_empty(&self) -> bool {
        self.1.is_empty()
    }

    pub fn to_sync(self) -> SyncData {
        SyncData::new(self.1).unwrap()
    }

    pub fn to_cast(self, mut prefix: Vec<u8>) -> CastData {
        prefix.append(&mut self.bytes());
        CastData::new(prefix).unwrap()
    }

    pub fn bytes(self) -> Vec<u8> {
        self.1
    }

    pub fn ref_bytes(&self) -> &Vec<u8> {
        &self.1
    }

    pub fn first_byte(&self) -> u8 {
        self.1[0]
    }
    pub fn second_byte(&self) -> u8 {
        self.1[1]
    }
    pub fn third_byte(&self) -> u8 {
        self.1[2]
    }
    pub fn len(&self) -> usize {
        self.1.len()
    }
    pub fn get_hash(&self) -> u64 {
        // eprintln!("data.get_hash() {} {:?}", self.0, self.1);
        self.0
    }
    pub fn hash(&mut self) -> u64 {
        // let mut hasher = DefaultHasher::new();
        // self.1.hash(&mut hasher);
        // let hash = hasher.finish();
        let hash = sha_hash(&self.1);
        self.0 = hash;
        hash
    }
}

impl fmt::Display for Data {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[len:{:?}]", self.len())
    }
}
