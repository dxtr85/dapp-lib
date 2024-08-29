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
    Filled(Content),
    Hashed(Box<Substore>),
}

impl Datastore {
    pub fn new(manifest: ApplicationManifest) -> Datastore {
        let mut content_tree = ContentTree::empty(0);
        let _ = content_tree.append(manifest.to_data(None));
        let content = Content::Data(0, content_tree);
        Datastore::Filled(content)
    }

    // this fn should be used for inserting new Data into existing Content,
    // failing when all possible slots are already taken
    pub fn insert_data(&mut self, c_id: ContentID, d_id: u16, data: Data) -> Result<u64, AppError> {
        let take_result = self.take(c_id);
        if let Err(e) = take_result {
            return Err(e);
        }
        let mut content = take_result.unwrap();
        let insert_result = content.insert(d_id, data);
        let _ = self.update(c_id, content);
        insert_result
    }

    // this fn should be used for adding new Data to existing Content,
    // failing when all possible slots are already taken
    pub fn append_data(&mut self, c_id: ContentID, data: Data) -> Result<u64, AppError> {
        println!("Append {}", c_id);
        let take_result = self.take(c_id);
        if let Err(e) = take_result {
            return Err(e);
        }
        let mut content = take_result.unwrap();
        let append_result = content.push_data(data);
        let _ = self.update(c_id, content);
        append_result
    }

    // this fn should be used for removing last Data chunk from existing Content,
    pub fn pop_data(&mut self, c_id: ContentID) -> Result<Data, AppError> {
        let take_result = self.take(c_id);
        if let Err(e) = take_result {
            return Err(e);
        }
        let mut content = take_result.unwrap();
        let pop_result = content.pop_data();
        let _ = self.update(c_id, content);
        pop_result
    }

    // this fn should be used for removing a Data chunk from existing Content,
    pub fn remove_data(&mut self, c_id: ContentID, d_id: u16) -> Result<Data, AppError> {
        let take_result = self.take(c_id);
        if let Err(e) = take_result {
            return Err(e);
        }
        let mut content = take_result.unwrap();
        let remove_result = content.remove_data(d_id);
        let _ = self.update(c_id, content);
        remove_result
    }

    // this fn should be used for adding new Content to Datastore,
    // failing when all possible slots are already taken
    pub fn append(&mut self, content: Content) -> Result<u64, AppError> {
        let myself = std::mem::replace(self, Datastore::Empty);
        match myself {
            Self::Empty => {
                // println!("Append to empty");
                // This is not expected to happen,
                // Empty is only a temp constructor.
                let hash = content.hash();
                *self = Datastore::Filled(content);
                Ok(hash)
            }
            Self::Filled(prev_content) => {
                // println!("Append to Filled");
                let hash = double_hash(prev_content.hash(), content.hash());
                let substore = Substore {
                    data_count: 2,
                    hash,
                    left: Datastore::Filled(prev_content),
                    right: Datastore::Filled(content),
                };
                *self = Datastore::Hashed(Box::new(substore));
                Ok(hash)
            }
            Self::Hashed(mut substore) => {
                // println!("Append to Hashed");
                let result = substore.append(content);
                *self = Datastore::Hashed(substore);
                result
            }
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
            Self::Filled(old_content) => {
                if c_id == 0 {
                    let new_content = old_content.shell();
                    *self = Self::Filled(new_content);
                    Ok(old_content)
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(mut s_store) => {
                let result = s_store.take(c_id);
                *self = Self::Hashed(s_store);
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
            Self::Empty => Err(AppError::IndexingError),
            Self::Filled(old_content) => {
                if c_id == 0 {
                    *self = Self::Filled(content);
                    Ok(old_content)
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(mut s_store) => {
                let result = s_store.update(c_id, content);
                *self = Self::Hashed(s_store);
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
            Self::Filled(mut content) => {
                if c_id == 0 {
                    let result = content.update_data(data_id, data);
                    *self = Self::Filled(content);
                    result
                } else {
                    *self = Self::Filled(content);
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(mut s_store) => {
                let result = s_store.update_data((c_id, data_id), data);
                *self = Self::Hashed(s_store);
                result
            }
        }
    }

    // This fn should be used for reading selected datachunk
    pub fn read_data(&self, (c_id, data_id): (ContentID, u16)) -> Result<Data, AppError> {
        match self {
            Self::Empty => Err(AppError::IndexingError),
            Self::Filled(content) => {
                if c_id == 0 {
                    content.read_data(data_id)
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(s_store) => s_store.read_data((c_id, data_id)),
        }
    }

    // This fn should return a Vec<u64> of all of Datastore's
    // bottom layer hashes from left to right.
    // Those are also called Content root hashes.
    pub fn all_root_hashes(&self) -> Vec<u64> {
        // println!("Next")
        let mut v = vec![];
        for c_id in 0..u16::MAX {
            if let Ok(hash) = self.get_root_content_hash(c_id) {
                v.push(hash)
            } else {
                break;
            }
        }
        println!("All root hashes count: {}", v.len());
        v
    }

    // TODO: this fn should return a Vec<u64> of all of Datastore's
    // non-bottom layer hashes from top to almost bottom, left to right.
    pub fn hashes(&self) {}

    // TODO: this fn should return a Vec<u64> of all of given CID's hashes
    // So only non-bottom layer hashes.
    pub fn content_hashes(&self, c_id: ContentID) {}

    // This fn should return a Vec<u64> of all of given CID's data hashes
    // So only bottom layer hashes (Data hashes).
    pub fn content_bottom_hashes(&self, c_id: ContentID) -> Result<Vec<u64>, AppError> {
        match self {
            Self::Empty => Err(AppError::Smthg),
            Self::Filled(content) => {
                if c_id == 0 {
                    Ok(content.data_hashes())
                } else {
                    Err(AppError::Smthg)
                }
            }
            Self::Hashed(s_store) => s_store.content_bottom_hashes(c_id),
        }
    }

    pub fn hash(&self) -> u64 {
        match self {
            Self::Empty => 0, // This is unexpected to happen
            Self::Filled(content) => content.hash(),
            Self::Hashed(s_store) => s_store.hash,
        }
    }
    pub fn get_root_content_hash(&self, c_id: ContentID) -> Result<u64, ()> {
        match self {
            Self::Empty => Err(()),
            Self::Filled(content) => {
                if c_id == 0 {
                    Ok(content.hash())
                } else {
                    Err(())
                }
            }
            Self::Hashed(s_store) => s_store.get_root_content_hash(c_id),
        }
    }

    pub fn len(&self) -> u16 {
        match self {
            Self::Empty => 0,
            Self::Filled(_c) => 1,
            Self::Hashed(s_store) => s_store.data_count,
        }
    }
}

#[derive(Debug)]
struct Substore {
    data_count: u16,
    hash: u64,
    left: Datastore,
    right: Datastore,
}

impl Substore {
    pub fn append(&mut self, content: Content) -> Result<u64, AppError> {
        // println!("Substore count: {}", self.data_count);
        if self.data_count < u16::MAX {
            let result = self.right.append(content);
            if let Ok(_h) = result {
                self.data_count += 1;
                Ok(self.hash())
            } else {
                result
            }
        } else {
            Err(AppError::DatastoreFull)
        }
    }

    pub fn take(&mut self, c_id: ContentID) -> Result<Content, AppError> {
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        let left_len = self.left.len();
        if c_id >= left_len {
            self.right.take(c_id - left_len)
        } else {
            self.left.take(c_id)
        }
    }
    pub fn read_data(&self, (c_id, data_id): (ContentID, u16)) -> Result<Data, AppError> {
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        let left_len = self.left.len();
        if c_id >= left_len {
            self.right.read_data((c_id - left_len, data_id))
        } else {
            self.left.read_data((c_id, data_id))
        }
    }

    pub fn update(&mut self, c_id: ContentID, content: Content) -> Result<Content, AppError> {
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        let left_len = self.left.len();
        let result = if c_id >= left_len {
            self.right.update(c_id - left_len, content)
        } else {
            self.left.update(c_id, content)
        };
        self.hash();
        result
    }

    pub fn update_data(
        &mut self,
        (c_id, data_id): (ContentID, u16),
        data: Data,
    ) -> Result<Data, AppError> {
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        let left_len = self.left.len();
        if c_id >= left_len {
            self.right.update_data((c_id - left_len, data_id), data)
        } else {
            self.left.update_data((c_id, data_id), data)
        }
    }

    pub fn hash(&mut self) -> u64 {
        self.hash = double_hash(self.left.hash(), self.right.hash());
        self.hash
    }

    pub fn content_bottom_hashes(&self, c_id: ContentID) -> Result<Vec<u64>, AppError> {
        if c_id >= self.data_count {
            return Err(AppError::Smthg);
        }
        let left_len = self.left.len();
        if c_id >= left_len {
            self.right.content_bottom_hashes(c_id - left_len)
        } else {
            self.left.content_bottom_hashes(c_id)
        }
    }
    fn get_root_content_hash(&self, c_id: ContentID) -> Result<u64, ()> {
        if c_id >= self.data_count {
            return Err(());
        }
        let left_len = self.left.len();
        // println!("c_id: {}, left_len: {}", c_id, left_len);
        if c_id >= left_len {
            self.right.get_root_content_hash(c_id - left_len)
        } else {
            self.left.get_root_content_hash(c_id)
        }
    }
}
