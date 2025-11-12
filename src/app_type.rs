#[derive(Debug, Copy, Clone, PartialEq)]
pub enum AppType {
    Catalog,
    Forum,
    //Market,
    //Storage,
    //Ledger,
    //...
    Other(u8),
}
impl AppType {
    pub fn byte(&self) -> u8 {
        match self {
            Self::Catalog => 255,
            Self::Forum => 254,
            Self::Other(val) => *val,
        }
    }
    pub fn from(byte: u8) -> Self {
        match byte {
            255 => Self::Catalog,
            254 => Self::Forum,
            other => Self::Other(other),
        }
    }
    pub fn is_catalog(&self) -> bool {
        matches!(self, Self::Catalog)
    }
    pub fn list_of_types() -> Vec<String> {
        vec![format!("Link=>Catalog"), format!("Link=>Forum")]
    }
    pub fn type_byte_from_string(text: &str) -> Option<u8> {
        match text {
            "Link=>Catalog" => Some(255),
            "Link=>Forum" => Some(254),
            _ => None,
        }
    }
}

// TODO: move this to each application's logic, since
// each application defines a distinct Manifest,
// dapp-lib only needs to know the app type of given App/Swarm.

// use super::content::DataType;
// use crate::Data;
// use std::collections::HashMap;
// use std::hash::Hash;
// use std::hash::{DefaultHasher, Hasher};
// pub struct Manifest([u8; 495], HashMap<DataType, u16>);
// pub struct Manifest {
//     app_type: AppType,
//     tags: HashMap<u8, Tag>,
// }
//TODO: a new Manifest definition, with attributes being added as needed during development
// Manifest should apply to a Swarm, not an Application, application is defined in code
// and can support different kinds of Swarms distinguished by their AppType
// Manifest defines application type, tags, data structures, and message headers:
// It consist of two elements: a 495-byte long header, and a HashMap<u8,u16>.
// Header contains a general application description.
// Mapping stores partial ContentIDs of locally stored Content that holds further
// definitions required for application to function.
// Those ContentIDs should all have the same Datatype value = 255.
// Header may contain instructions on how to decrypt those Contents.
// There can be up to 256 tags defined for a given swarm.
// There can be up to 256 top level data structures defined in a single application.
// There can be up to 256 top level synchronization messages defined.
// There can be also some (less than 256) top level reconfiguration messages defined.
// (We already have some built-in Reconfigs.)

// pub struct Tag(String);
// impl Tag {
//     pub fn new(name: String) -> Result<Self, ()> {
//         if name.len() <= 30 {
//             Ok(Tag(name))
//         } else {
//             Err(())
//         }
//     }
// }
// impl Manifest {
//     pub fn empty() -> Self {
//         Manifest {
//             app_type: AppType::Catalog,
//             tags: HashMap::new(),
//         }
//     }
//     pub fn new(app_type: AppType, tags: HashMap<u8, Tag>) -> Self {
//         Manifest { app_type, tags }
//     }
//     // pub fn set_header(&mut self, value: [u8; 495]) {
//     //     self.0 = value;
//     // }

//     // pub fn insert(&mut self, key: u8, value: u16) {
//     //     self.1.insert(key, value);
//     // }

//     pub fn hash(&self) -> u64 {
//         let mut hasher = DefaultHasher::new();
//         let mut bytes = vec![self.app_type.byte(), self.tags.len() as u8];
//         // println!("mani hash len1: {}", bytes.len());

//         for i in 0..=255 {
//             // bytes.push(i);
//             if let Some(tag_name) = self.tags.get(&i) {
//                 let tag_len = tag_name.0.len() as u8;
//                 bytes.push(i);
//                 bytes.push(tag_len);
//                 for c in tag_name.0.as_bytes() {
//                     bytes.push(*c);
//                 }
//             }
//         }
//         // println!("mani hash len: {}", bytes.len());
//         bytes.hash(&mut hasher);
//         hasher.finish()
//     }

//     pub fn from(data: Data) -> Self {
//         println!("Constructing manifest from: {} bytes", data.len());
//         let mut iter = data.bytes().into_iter();
//         // if iter.len() == 1024 {
//         //     let _ = iter.next();
//         // }
//         let app_type = AppType::from(iter.next().unwrap());
//         let tags_len = iter.next().unwrap();
//         let mut tags = HashMap::new();
//         for _i in 0..tags_len {
//             let tag_id = iter.next().unwrap();
//             let tag_len = iter.next().unwrap();
//             let mut name_bytes = Vec::with_capacity(tag_len as usize);
//             for _j in 0..tag_len {
//                 name_bytes.push(iter.next().unwrap());
//             }
//             let tag = Tag::new(String::from_utf8(name_bytes).unwrap()).unwrap();
//             tags.insert(tag_id, tag);
//         }
//         Self { app_type, tags }
//     }

//     // TODO: check if total bytes count is <= 1024
//     pub fn to_data(&self, prepend_bytes: Option<Vec<u8>>) -> Data {
//         let mut res = Vec::with_capacity(1024);
//         if let Some(mut bytes) = prepend_bytes {
//             res.append(&mut bytes);
//         }
//         res.push(self.app_type.byte());
//         res.push(self.tags.len() as u8);
//         for i in 0..=255 {
//             if let Some(tag) = self.tags.get(&i) {
//                 res.push(i);
//                 res.push(tag.0.len() as u8);
//                 res.append(&mut tag.0.clone().into_bytes());
//             }
//         }

//         // It will crash when we have sufficient count of tags!
//         Data::new(res).unwrap()
//     }
// }
