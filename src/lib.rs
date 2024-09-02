mod content;
mod datastore;
mod error;
mod manifest;
mod message;
mod registry;
use std::collections::HashMap;

use content::{Content, ContentID};
use datastore::Datastore;
use error::AppError;
use gnome::prelude::Data;
use manifest::ApplicationManifest;
use message::{SyncMessage, SyncMessageType, SyncRequirements};
use registry::ChangeRegistry;

pub mod prelude {
    pub use crate::content::{Content, ContentID, ContentTree};
    pub use crate::error::AppError;
    pub use crate::manifest::ApplicationManifest;
    pub use crate::message::SyncMessage;
    pub use crate::message::SyncMessageType;
    pub use crate::message::SyncRequirements;
    pub use crate::Application;
    pub use gnome::prelude::*;
}

pub struct Application {
    change_reg: ChangeRegistry,
    contents: Datastore,
    hash_to_temp_idx: HashMap<u64, u16>,
    partial_data: HashMap<u16, (Vec<u64>, HashMap<u64, Data>)>,
}

impl Application {
    pub fn empty() -> Self {
        Application {
            change_reg: ChangeRegistry::new(),
            contents: Datastore::Empty,
            hash_to_temp_idx: HashMap::new(),
            partial_data: HashMap::new(),
        }
    }
    pub fn new(manifest: ApplicationManifest) -> Self {
        Application {
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
    pub fn process(&mut self, data: Data) -> Option<SyncMessage> {
        // let mut bytes_iter = data.ref_bytes().iter();
        let mut bytes = data.bytes();
        let m_type = SyncMessageType::new(&mut bytes);
        let mut drained_bytes = m_type.as_bytes();
        let part_no = bytes.drain(0..1).next().unwrap();
        let total_parts = bytes.drain(0..1).next().unwrap();
        drained_bytes.push(part_no);
        drained_bytes.push(total_parts);
        println!("[{}/{}]", part_no, total_parts);
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
                    for j in 0..8 {
                        hash[j] = bytes.drain(0..1).next().unwrap();
                        drained_bytes.push(hash[j]);
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
                if let Some((vec, mut hm)) = self.partial_data.remove(&temp_idx) {
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
    pub fn content_root_hash(&self, c_id: ContentID) -> Result<u64, ()> {
        self.contents.get_root_content_hash(c_id)
    }
    pub fn registry(&self) -> Vec<ContentID> {
        self.change_reg.read()
    }

    pub fn all_content_root_hashes(&self) -> Vec<Vec<u64>> {
        self.contents.all_root_hashes()
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

// An entire application data consists of a structure called Datastore.
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
