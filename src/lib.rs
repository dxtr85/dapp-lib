mod content;
mod datastore;
mod manifest;
mod registry;
use datastore::Datastore;
use manifest::ApplicationManifest;
use registry::ChangeRegistry;

pub mod prelude {
    pub use crate::manifest::ApplicationManifest;
    pub use crate::Application;
    pub use gnome::prelude::*;
}

pub struct Application {
    change_reg: ChangeRegistry,
    contents: Datastore,
}

impl Application {
    pub fn new(manifest: ApplicationManifest) -> Self {
        Application {
            change_reg: ChangeRegistry::new(),
            contents: Datastore::new(manifest),
        }
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
// in a form of HashMap<u8,String(max len 63bytes)>.
// TODO: in a form of HashMap<u8,Vec<u8: max 63 bytes>> - more general.

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

// Datastore is a binary tree storing in it's leafs references to content trees.
// Non-leaf nodes contain hash of both it's left and right children.
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

// In worst case scenario a single Swarm can contain up to 4Tibibytes of Data,
// so immediate complete syncing would take forever.

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
    use super::*;

    #[test]
    fn it_works() {
        // let result = add(2, 2);
        // assert_eq!(result, 4);
    }
}
