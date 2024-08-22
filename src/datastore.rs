use crate::content::double_hash;

use super::content::Content;
use super::content::ContentID;
use super::content::ContentTree;
use super::manifest::ApplicationManifest;
use super::prelude::AppError;
use gnome::prelude::Data;

// A Datastore is an append-only data structure built as a binary tree,
// that should never be actively balanced.
#[derive(Debug)]
pub enum Datastore {
    Empty,
    Hashed(u64, Box<(ContentSlot, ContentSlot)>),
}

impl Datastore {
    pub fn new(manifest: ApplicationManifest) -> Datastore {
        let mut content_tree = ContentTree::empty(0);
        let _ = content_tree.append(manifest.to_data(None));
        let content = Content::Data(0, content_tree);
        let hash = content.hash();
        let dslot = ContentSlot {
            content,
            data_count: 1,
            hash,
            left: Datastore::Empty,
            right: Datastore::Empty,
        };
        Datastore::Hashed(hash, Box::new((dslot, ContentSlot::empty())))
    }

    // this fn should be used for adding new Content to Datastore,
    // failing when all possible slots are already taken
    pub fn append(&mut self, content: Content) -> Result<u64, AppError> {
        let index_to_add = self.len();
        if index_to_add == u16::MAX {
            return Err(AppError::DatastoreFull);
        }
        let update_res = self.update(index_to_add, content);
        if let Ok(_prev) = update_res {
            Ok(self.update_hash())
        } else {
            Err(update_res.err().unwrap())
        }
    }

    //  This fn should be used for taking given content out of Datastore,
    //       replacing it with it's shell representation.
    //       This is useful when we frequently use given Content in our App.
    //       This way we only have to update Empty(root_hash) for given CID
    //       in Datastore, and modify Content directly within App.
    pub fn take(&mut self, c_id: ContentID) -> Result<Content, AppError> {
        let myself = std::mem::replace(self, Datastore::Empty);
        match myself {
            Self::Empty => Err(AppError::IndexingError),
            Self::Hashed(hash, mut boxed_slots) => {
                let result = if c_id % 2 == 0 {
                    boxed_slots.0.take(c_id >> 1)
                } else {
                    boxed_slots.1.take(c_id >> 1)
                };
                *self = if result.is_ok() {
                    Self::Hashed(
                        double_hash(boxed_slots.0.hash, boxed_slots.0.hash),
                        boxed_slots,
                    )
                } else {
                    Self::Hashed(hash, boxed_slots)
                };
                result
            }
        }
    }

    // This fn should be used for updating content of existing item.
    // This can fail only when we are trying to change to Content with
    // different Datatype from old Content
    pub fn update(&mut self, c_id: ContentID, content: Content) -> Result<Content, AppError> {
        let myself = std::mem::replace(self, Datastore::Empty);
        match myself {
            Self::Empty => {
                if c_id == 0 {
                    let hash = content.hash();
                    let c_slot = ContentSlot::new(content);
                    *self = Datastore::Hashed(hash, Box::new((c_slot, ContentSlot::empty())));
                    Ok(Content::Data(0, ContentTree::Empty(0)))
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(old_hash, mut boxed_slots) => {
                let result = if c_id % 2 == 0 {
                    boxed_slots.0.update(c_id >> 1, content)
                } else {
                    boxed_slots.1.update(c_id >> 1, content)
                };
                *self = if result.is_ok() {
                    if boxed_slots.1.data_count == 0 {
                        Self::Hashed(boxed_slots.0.hash, boxed_slots)
                    } else {
                        Self::Hashed(
                            double_hash(boxed_slots.0.hash, boxed_slots.1.hash),
                            boxed_slots,
                        )
                    }
                } else {
                    Self::Hashed(old_hash, boxed_slots)
                };
                result
            }
        }
    }

    // This fn should be used for updating a datachunk of a given CID
    // It can fail when either of ids does not exist
    pub fn update_data(
        &mut self,
        (c_id, data_id): (ContentID, u16),
        data: Data,
    ) -> Result<Data, AppError> {
        let myself = std::mem::replace(self, Datastore::Empty);
        match myself {
            Self::Empty => Err(AppError::IndexingError),
            Self::Hashed(old_hash, mut boxed_slots) => {
                let result = if c_id % 2 == 0 {
                    boxed_slots.0.update_data((c_id >> 1, data_id), data)
                } else {
                    boxed_slots.1.update_data((c_id >> 1, data_id), data)
                };
                *self = if result.is_ok() {
                    if boxed_slots.1.data_count == 0 {
                        Self::Hashed(boxed_slots.0.hash, boxed_slots)
                    } else {
                        Self::Hashed(
                            double_hash(boxed_slots.0.hash, boxed_slots.1.hash),
                            boxed_slots,
                        )
                    }
                } else {
                    Self::Hashed(old_hash, boxed_slots)
                };
                result
            }
        }
    }

    // This fn should be used for reading selected datachunk
    pub fn read_data(&self, (c_id, data_id): (ContentID, u16)) -> Result<Data, AppError> {
        match self {
            Self::Empty => Err(AppError::IndexingError),
            Self::Hashed(_hash, boxed_slots) => {
                if c_id % 2 == 0 {
                    boxed_slots.0.read_data((c_id >> 1, data_id))
                } else {
                    boxed_slots.1.read_data((c_id >> 1, data_id))
                }
            }
        }
    }

    // This fn should return a Vec<u64> of all of Datastore's
    // bottom layer hashes from left to right.
    // Those are also called Content root hashes.
    pub fn all_root_hashes(&self) -> Vec<u64> {
        let mut v = vec![];
        for c_id in 0..u16::MAX {
            if let Ok(hash) = self.get_root_content_hash(c_id) {
                v.push(hash)
            } else {
                break;
            }
        }
        v
    }

    // This fn should return a Vec<u64> of all of given CID's data hashes
    // So only bottom layer hashes (Data hashes).
    pub fn content_data_hashes(&self, c_id: ContentID) -> Result<Vec<u64>, AppError> {
        match self {
            Self::Empty => Err(AppError::Smthg),
            Self::Hashed(_hash, boxed_slots) => {
                if c_id % 2 == 0 {
                    boxed_slots.0.content_data_hashes(c_id >> 1)
                } else {
                    boxed_slots.1.content_data_hashes(c_id >> 1)
                }
            }
        }
    }

    pub fn get_hash(&self) -> u64 {
        match self {
            Self::Empty => 0,
            Self::Hashed(hash, _slots) => *hash,
        }
    }

    pub fn update_hash(&mut self) -> u64 {
        let myself = std::mem::replace(self, Datastore::Empty);
        if let Datastore::Hashed(_hash, boxed_slots) = myself {
            let new_hash = if boxed_slots.1.data_count == 0 {
                boxed_slots.0.hash
            } else {
                double_hash(boxed_slots.0.hash, boxed_slots.1.hash)
            };
            *self = Datastore::Hashed(new_hash, boxed_slots);
            new_hash
        } else {
            0
        }
    }

    fn get_root_content_hash(&self, c_id: ContentID) -> Result<u64, ()> {
        match self {
            Self::Empty => Err(()),
            Self::Hashed(_hash, boxed_slots) => {
                if c_id % 2 == 0 {
                    boxed_slots.0.get_root_content_hash(c_id >> 1)
                } else {
                    boxed_slots.1.get_root_content_hash(c_id >> 1)
                }
            }
        }
    }

    pub fn len(&self) -> u16 {
        match self {
            Self::Empty => 0,
            Self::Hashed(_hash, boxed_slots) => boxed_slots
                .0
                .data_count
                .saturating_add(boxed_slots.1.data_count),
        }
    }
}

#[derive(Debug)]
pub struct ContentSlot {
    content: Content,
    data_count: u16,
    hash: u64,
    left: Datastore,
    right: Datastore,
}

impl ContentSlot {
    pub fn empty() -> Self {
        ContentSlot {
            content: Content::Data(0, ContentTree::Empty(0)),
            data_count: 0,
            hash: 0,
            left: Datastore::Empty,
            right: Datastore::Empty,
        }
    }
    pub fn new(content: Content) -> Self {
        let hash = content.hash();
        ContentSlot {
            content,
            data_count: 1,
            hash,
            left: Datastore::Empty,
            right: Datastore::Empty,
        }
    }

    pub fn take(&mut self, c_id: ContentID) -> Result<Content, AppError> {
        if c_id == 0 {
            if self.data_count == 0 {
                return Err(AppError::ContentEmpty);
            } else {
                let shell = self.content.shell();
                let content = std::mem::replace(&mut self.content, shell);
                return Ok(content);
            }
        }
        // Not sure if following is correct/needed
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        if c_id % 2 == 0 {
            self.left.take(c_id)
        } else {
            self.right.take(c_id)
        }
    }
    pub fn read_data(&self, (c_id, data_id): (ContentID, u16)) -> Result<Data, AppError> {
        if c_id == 0 {
            if self.data_count == 0 {
                return Err(AppError::ContentEmpty);
            } else {
                return self.content.read_data(data_id);
            }
        }
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        if c_id % 2 == 0 {
            self.left.read_data((c_id, data_id))
        } else {
            self.right.read_data((c_id, data_id))
        }
    }

    pub fn update(&mut self, c_id: ContentID, content: Content) -> Result<Content, AppError> {
        if c_id == 0 {
            if self.data_count == 0 {
                return Err(AppError::ContentEmpty);
            } else {
                let old_content = self.content.update(content);
                self.update_hash();
                return Ok(old_content);
            }
        }
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        if c_id % 2 == 0 {
            self.left.update(c_id, content)
        } else {
            self.right.update(c_id, content)
        }
    }

    pub fn update_data(
        &mut self,
        (c_id, data_id): (ContentID, u16),
        data: Data,
    ) -> Result<Data, AppError> {
        if c_id == 0 {
            if self.data_count == 0 {
                return Err(AppError::ContentEmpty);
            } else {
                let upd_result = self.content.update_data(data_id, data);
                if upd_result.is_ok() {
                    self.update_hash();
                }
                return upd_result;
            }
        }
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        if c_id % 2 == 0 {
            self.left.update_data((c_id, data_id), data)
        } else {
            self.right.update_data((c_id, data_id), data)
        }
    }

    pub fn update_hash(&mut self) -> u64 {
        self.hash = match (&self.left, &self.right) {
            (Datastore::Empty, Datastore::Empty) => self.content.hash(),
            (Datastore::Hashed(hash, _b), Datastore::Empty) => {
                double_hash(self.content.hash(), *hash)
            }
            (Datastore::Empty, Datastore::Hashed(hash, _b)) => {
                double_hash(self.content.hash(), *hash)
            }
            (Datastore::Hashed(hash_one, _b), Datastore::Hashed(hash_two, _bt)) => {
                double_hash(self.content.hash(), double_hash(*hash_one, *hash_two))
            }
        };
        self.hash
    }

    pub fn content_data_hashes(&self, c_id: ContentID) -> Result<Vec<u64>, AppError> {
        if c_id == 0 {
            if self.data_count == 0 {
                return Err(AppError::Smthg);
            } else {
                return Ok(self.content.data_hashes());
            }
        }
        if c_id >= self.data_count {
            return Err(AppError::Smthg);
        }
        if c_id % 2 == 0 {
            self.left.content_data_hashes(c_id)
        } else {
            self.right.content_data_hashes(c_id)
        }
    }
    fn get_root_content_hash(&self, c_id: ContentID) -> Result<u64, ()> {
        if c_id == 0 {
            if self.data_count == 0 {
                return Err(());
            } else {
                return Ok(self.content.hash());
            }
        }
        if c_id >= self.data_count {
            return Err(());
        }
        if c_id % 2 == 0 {
            self.left.get_root_content_hash(c_id)
        } else {
            self.right.get_root_content_hash(c_id)
        }
    }
}
