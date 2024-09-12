use super::content::DataType;
use crate::Data;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hasher};
use std::{fmt, hash::Hash};
pub struct ApplicationManifest([u8; 495], HashMap<DataType, u16>);

// ApplicationManifest defines application type, data structures, and message headers:
// It consist of two elements: a 495-byte long header, and a HashMap<u8,u16>.
// Header contains a general application description.
// Mapping stores partial ContentIDs of locally stored Content that holds further
// definitions required for application to function.
// Those ContentIDs should all have the same Datatype value = 255.
// Header may contain instructions on how to decrypt those Contents.
// There can be up to 256 top level data structures defined in a single application.
// There can be up to 256 top level synchronization messages defined.
// There can be also some (less than 256) top level reconfiguration messages defined.
// (We already have some built-in Reconfigs.)

impl ApplicationManifest {
    pub fn empty() -> Self {
        ApplicationManifest([0; 495], HashMap::new())
    }
    pub fn new(value: [u8; 495], mapping: HashMap<u8, u16>) -> Self {
        ApplicationManifest(value, mapping)
    }
    pub fn set_header(&mut self, value: [u8; 495]) {
        self.0 = value;
    }

    pub fn insert(&mut self, key: u8, value: u16) {
        self.1.insert(key, value);
    }

    pub fn hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        let mut bytes = Vec::from(self.0.clone());
        // println!("mani hash len1: {}", bytes.len());
        for i in 0..=255 {
            // bytes.push(i);
            if let Some(value) = self.1.get(&i) {
                let [first, second] = value.to_be_bytes();
                bytes.push(first);
                bytes.push(second);
            } else {
                bytes.push(0);
                bytes.push(0);
            }
        }
        // println!("mani hash len: {}", bytes.len());
        bytes.hash(&mut hasher);
        hasher.finish()
    }

    pub fn from(data: Data) -> Self {
        println!("Constructing manifest from: {} bytes", data.len());
        let mut iter = data.bytes().into_iter();
        // if iter.len() == 1024 {
        //     let _ = iter.next();
        // }
        let mut header: [u8; 495] = [0; 495];
        for i in 0..495 {
            header[i] = iter.next().unwrap();
        }
        let mut mapping = HashMap::new();
        for i in 0..=255 {
            // if i > 245 {
            //     println!("Manifest: {}", i);
            // }
            let first_byte = iter.next().unwrap();
            let second_byte = iter.next().unwrap();
            mapping.insert(i, u16::from_be_bytes([first_byte, second_byte]));
        }
        Self(header, mapping)
    }

    pub fn to_data(&self, prepend_bytes: Option<Vec<u8>>) -> Data {
        let mut res = Vec::with_capacity(1024);
        if let Some(mut bytes) = prepend_bytes {
            res.append(&mut bytes);
        }
        for byte in self.0 {
            res.push(byte);
        }
        for i in 0..=255 {
            if let Some(value) = self.1.get(&i) {
                let bts = u16::to_be_bytes(*value);
                res.push(bts[0]);
                res.push(bts[1]);
            } else {
                res.push(0);
                res.push(0);
            }
        }

        Data::new(res).unwrap()
    }
}
