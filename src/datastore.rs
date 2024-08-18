use crate::content::double_hash;

use super::content::Content;
use super::content::ContentID;
use super::content::ContentTree;
use super::manifest::manifest_to_data;
use super::manifest::ApplicationManifest;
use super::prelude::AppError;
use gnome::prelude::Data;

#[derive(Debug)]
pub enum Datastore {
    Empty,
    Filled(Content),
    Hashed(Box<Substore>),
}

impl Datastore {
    pub fn new(manifest: ApplicationManifest) -> Datastore {
        let mut content_tree = ContentTree::empty(0);
        for data in manifest_to_data(manifest) {
            let _ = content_tree.append(data);
        }
        let content = Content::Data(0, content_tree);
        Datastore::Filled(content)
    }

    // TODO: this fn should be used for adding new Content to Datastore
    // failing when all possible slots are already taken
    pub fn insert(&mut self, content: Content) -> Result<(), AppError> {
        let myself = std::mem::replace(self, Datastore::Empty);
        match myself {
            Self::Empty => {
                // This is not expected to happen,
                // Empty is only a temp constructor.
                // In future we may use Empty(hash) as a placeholder
                // when syncing...
                *self = Datastore::Filled(content);
                Ok(())
            }
            Self::Filled(prev_content) => {
                // TODO: compute hash
                let hash = double_hash(prev_content.hash(), content.hash());
                let substore = Substore {
                    data_count: 2,
                    hash,
                    left: Datastore::Filled(prev_content),
                    right: Datastore::Filled(content),
                };
                *self = Datastore::Hashed(Box::new(substore));
                Ok(())
            }
            Self::Hashed(mut substore) => {
                if substore.data_count < u16::MAX {
                    let result = substore.insert(content);
                    *self = Datastore::Hashed(substore);
                    result
                } else {
                    *self = Datastore::Hashed(substore);
                    Err(AppError::DatastoreFull)
                }
            }
        }
    }

    // TODO: this fn should be used for updating content of existing item.
    // This can fail only when we are trying to change to Content with
    // different Datatype
    pub fn update(&mut self, c_id: ContentID, content: Content) -> Result<(), AppError> {
        Ok(())
    }

    // TODO: this fn should be used for updating a datachunk of a given CID
    // It can fail when either of ids does not exist
    pub fn update_data(&mut self, ids: (ContentID, u16), data: Data) -> Result<(), AppError> {
        Ok(())
    }

    // TODO: this fn should be used for reading entire Content from Datastore
    pub fn read(&self, c_id: ContentID) -> Result<(), AppError> {
        Ok(())
    }

    // TODO: this fn should be used for reading selected datachunk
    pub fn read_data(&self, ids: (ContentID, u16)) -> Result<(), AppError> {
        Ok(())
    }

    // TODO: this fn should return a Vec<u64> of all of Datastore's
    // non-bottom layer hashes from top to almost bottom, left to right.
    pub fn hashes(&self) {}

    // TODO: this fn should return a Vec<u64> of all of Datastore's
    // bottom layer hashes from left to right.
    // Those are also called Content root hashes.
    pub fn bottom_hashes(&self) {}

    // TODO: this fn should return a Vec<u64> of all of given CID's hashes
    // So only non-bottom layer hashes.
    pub fn content_hashes(&self, c_id: ContentID) {}

    // TODO: this fn should return a Vec<u64> of all of given CID's data hashes
    // So only bottom layer hashes (Data hashes).
    pub fn content_bottom_hashes(&self, c_id: ContentID) {}
}

#[derive(Debug)]
struct Substore {
    data_count: u16,
    hash: u64,
    left: Datastore,
    right: Datastore,
}

impl Substore {
    pub fn insert(&mut self, content: Content) -> Result<(), AppError> {
        if self.data_count < u16::MAX {
            self.right.insert(content)
        } else {
            Err(AppError::DatastoreFull)
        }
    }
}
