use super::content::Content;
use super::content::ContentID;
use super::content::ContentTree;
use super::prelude::AppError;
use crate::app_type::AppType;
use crate::content::double_hash;
use crate::content::DataType;
use crate::prelude::TransformInfo;
use crate::Data;
// use std::collections::HashSet;

// A Datastore is an append-only data structure built as a binary tree,
// that should never be actively balanced.
#[derive(Debug)]
pub enum Datastore {
    Empty,
    Filled(Content),
    Hashed(Box<Substore>),
}

impl Datastore {
    // pub fn new(manifest: Manifest) -> Datastore {
    pub fn new(app_type: AppType) -> Datastore {
        let mut content_tree = ContentTree::empty(0);
        // let _ = content_tree.append(manifest.to_data(None));
        let _ = content_tree.append(Data::new(vec![app_type.byte()]).unwrap());
        let content = Content::Data(DataType::Data(0), content_tree);
        Datastore::Filled(content)
    }

    pub fn type_and_len(&self, c_id: ContentID) -> Result<(DataType, u16), AppError> {
        match self {
            Self::Empty => Err(AppError::IndexingError),
            Self::Filled(content) => {
                if c_id == 0 {
                    Ok((content.data_type(), content.len()))
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(s_store) => s_store.type_and_len(c_id),
        }
    }

    // this fn should be used for inserting new Data into existing Content,
    // failing when all possible slots are already taken
    pub fn insert_data(&mut self, c_id: ContentID, d_id: u16, data: Data) -> Result<u64, AppError> {
        let take_result = self.take(c_id);
        eprintln!("Take result: {:?}", take_result);
        if let Err(e) = take_result {
            return Err(e);
        }
        let mut content = take_result.unwrap();
        let insert_result = content.insert(d_id, data);
        eprintln!("Insert result: {:?}", insert_result);
        let _ = self.update(c_id, content);
        insert_result
    }

    // this fn should be used for adding new Data to existing Content,
    // failing when all possible slots are already taken
    pub fn append_data(&mut self, c_id: ContentID, data: Data) -> Result<u64, AppError> {
        // eprintln!("Append {}", c_id);
        let take_result = self.take(c_id);
        if let Err(e) = take_result {
            return Err(e);
        }
        let mut content = take_result.unwrap();
        let append_result = content.push_data(data);
        let _r = self.update(c_id, content);
        let (_t, len) = self.type_and_len(c_id).unwrap();
        eprintln!("Update result: {}, len: {}", _r.is_ok(), len);
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
                    *self = Self::Filled(old_content);
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
    pub fn shell(&self, c_id: ContentID) -> Result<Content, AppError> {
        match self {
            Self::Empty => Err(AppError::IndexingError),
            Self::Filled(content) => {
                if c_id == 0 {
                    Ok(content.shell())
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(ref s_store) => s_store.shell(c_id),
        }
    }
    pub fn take_transform_info(&mut self, c_id: ContentID) -> Result<TransformInfo, AppError> {
        let myself = std::mem::replace(self, Datastore::Empty);
        match myself {
            Self::Empty => Err(AppError::IndexingError),
            Self::Filled(content) => {
                if c_id == 0 {
                    match content {
                        Content::Link(g_id, s_name, c_id, ti) => {
                            let result = if let Some(ti) = ti {
                                Ok(ti)
                            } else {
                                Err(AppError::LinkNonTransformative)
                            };
                            *self = Self::Filled(Content::Link(g_id, s_name, c_id, None));
                            result
                        }
                        other => {
                            *self = Self::Filled(other);
                            Err(AppError::DatatypeMismatch)
                        }
                    }
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(mut s_store) => {
                let result = s_store.take_transform_info(c_id);
                *self = Self::Hashed(s_store);
                result
            }
        }
    }

    pub fn restore_transform_info(
        &mut self,
        c_id: ContentID,
        ti: TransformInfo,
    ) -> Result<(), AppError> {
        //TODO
        let myself = std::mem::replace(self, Datastore::Empty);
        match myself {
            Self::Empty => Err(AppError::IndexingError),
            Self::Filled(content) => {
                if c_id == 0 {
                    match content {
                        Content::Link(g_id, s_name, c_id, _ti) => {
                            *self = Self::Filled(Content::Link(g_id, s_name, c_id, Some(ti)));
                            Ok(())
                        }
                        other => {
                            *self = Self::Filled(other);
                            Err(AppError::DatatypeMismatch)
                        }
                    }
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(mut s_store) => {
                let result = s_store.restore_transform_info(c_id, ti);
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
                    match old_content {
                        Content::Link(_g_id, ref _s_name, _c_id, ref ti_opt) => {
                            if ti_opt.is_some() {
                                *self = Self::Filled(old_content);
                                Err(AppError::Smthg)
                            } else {
                                *self = Self::Filled(content);
                                Ok(old_content)
                            }
                        }
                        other => {
                            *self = Self::Filled(content);
                            Ok(other)
                        }
                    }
                } else {
                    *self = Self::Filled(old_content);
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
                    // eprintln!("1 After update, len: {}", content.len());
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

    // This fn should be used when uploading content
    pub fn update_transformative_link(
        &mut self,
        is_hash: bool,
        content_id: ContentID,
        part_no: u16,
        total_parts: u16,
        data: Data,
    ) -> Result<(DataType, Vec<u16>, Vec<u16>), AppError> {
        // let content = self.take(content_id).unwrap();
        // println!("Update: {} is_hash: {}", content_id, is_hash);
        // if let Content::Link(g_id, s_name, lc_id, ti) = content {
        // if let Some(mut tiu) = ti {
        if let Ok(mut tiu) = self.take_transform_info(content_id) {
            // println!("Link has transform info");
            // let missing_parts;
            if is_hash {
                tiu.add_hash(part_no, total_parts, data);
            } else {
                tiu.add_data(part_no, total_parts, data);
            }
            let missing_hashes = tiu.what_hashes_are_missing();
            let missing_parts = tiu.what_data_is_missing(part_no);
            let d_type = tiu.d_type;
            let _res = self.restore_transform_info(content_id, tiu);
            // println!("Update res: {:?}", _res);
            Ok((d_type, missing_hashes, missing_parts))
        } else {
            eprintln!("Link has no TI");
            // let _ = self.update(content_id, Content::Link(g_id, s_name, lc_id, ti));
            Err(AppError::LinkNonTransformative)
        }
        // } else {
        //     let _ = self.update(content_id, content);
        //     Err(AppError::DatatypeMismatch)
        // }
    }

    // This fn should be used for reading selected datachunk
    pub fn read_data(&self, (c_id, data_id): (ContentID, u16)) -> Result<Data, AppError> {
        eprintln!("Read request: {}-{}", c_id, data_id);
        match self {
            Self::Empty => {
                eprintln!("EMPTY");
                Err(AppError::IndexingError)
            }
            Self::Filled(content) => {
                if c_id == 0 {
                    eprintln!("Read OK");
                    content.read_data(data_id)
                } else {
                    eprintln!("Read Error");
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(s_store) => s_store.read_data((c_id, data_id)),
        }
    }

    // This fn should be used for reading selected datachunk
    pub fn read_link_data(
        &self,
        (c_id, data_id): (ContentID, u16),
        d_type: DataType,
    ) -> Result<(Data, u16), AppError> {
        match self {
            Self::Empty => Err(AppError::IndexingError),
            Self::Filled(content) => {
                if c_id == 0 {
                    content.read_link_data(d_type, data_id)
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(s_store) => s_store.read_link_data((c_id, data_id), d_type),
        }
    }

    // This fn should return a Vec<u64> of all of Datastore's
    // bottom layer hashes from left to right.
    // Those are also called Content root hashes.
    pub fn all_typed_root_hashes(&self) -> Vec<Vec<(DataType, u64)>> {
        // println!("Next")

        let mut count = 0;
        let mut total = vec![];
        let mut v = vec![];
        for c_id in 0..u16::MAX {
            if let Ok(typed_hash) = self.get_root_content_typed_hash(c_id) {
                v.push(typed_hash);
                count += 1;
                if count == 128 {
                    total.push(v);
                    v = vec![];
                    count = 0;
                }
            } else {
                break;
            }
        }
        if !v.is_empty() {
            total.push(v);
        }
        // eprintln!("All root hashes count: ~{}", total.len() * 128);
        total
    }

    // TODO: this fn should return a Vec<u64> of all of Datastore's
    // non-bottom layer hashes from top to almost bottom, left to right.
    pub fn hashes(&self) {}

    // TODO: this fn should return a Vec<u64> of all of given CID's hashes
    // So only non-bottom layer hashes.
    pub fn content_hashes(&self, _c_id: ContentID) {}

    // This fn should return a Vec<u64> of all of given CID's data hashes
    // So only bottom layer hashes (Data hashes).

    pub fn link_transform_info_hashes(
        &self,
        c_id: ContentID,
        d_type: DataType,
    ) -> Result<Vec<Data>, AppError> {
        match self {
            Self::Empty => Err(AppError::ContentEmpty),
            Self::Filled(content) => {
                eprintln!("Sending bottom hashes up");
                if c_id == 0 {
                    content.link_ti_hashes()
                } else {
                    Err(AppError::IndexingError)
                }
            }
            Self::Hashed(s_store) => s_store.link_transform_info_hashes(c_id, d_type),
        }
    }

    pub fn content_bottom_hashes(&self, c_id: ContentID) -> Result<Vec<u64>, AppError> {
        match self {
            Self::Empty => Err(AppError::ContentEmpty),
            Self::Filled(content) => {
                println!("Sending bottom hashes up");
                Ok(content.data_hashes())
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
    pub fn get_root_content_typed_hash(
        &self,
        c_id: ContentID,
    ) -> Result<(DataType, u64), AppError> {
        match self {
            Self::Empty => Err(AppError::ContentEmpty),
            Self::Filled(content) => {
                if c_id == 0 {
                    Ok((content.data_type(), content.hash()))
                } else {
                    Err(AppError::IndexingError)
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
pub struct Substore {
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
    pub fn type_and_len(&self, c_id: ContentID) -> Result<(DataType, u16), AppError> {
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        let left_len = self.left.len();
        if c_id >= left_len {
            self.right.type_and_len(c_id - left_len)
        } else {
            self.left.type_and_len(c_id)
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
    pub fn shell(&self, c_id: ContentID) -> Result<Content, AppError> {
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        let left_len = self.left.len();
        if c_id >= left_len {
            self.right.shell(c_id - left_len)
        } else {
            self.left.shell(c_id)
        }
    }
    pub fn take_transform_info(&mut self, c_id: ContentID) -> Result<TransformInfo, AppError> {
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        let left_len = self.left.len();
        if c_id >= left_len {
            self.right.take_transform_info(c_id - left_len)
        } else {
            self.left.take_transform_info(c_id)
        }
    }
    pub fn restore_transform_info(
        &mut self,
        c_id: ContentID,
        ti: TransformInfo,
    ) -> Result<(), AppError> {
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        let left_len = self.left.len();
        if c_id >= left_len {
            self.right.restore_transform_info(c_id - left_len, ti)
        } else {
            self.left.restore_transform_info(c_id, ti)
        }
    }
    pub fn read_link_data(
        &self,
        (c_id, data_id): (ContentID, u16),
        d_type: DataType,
    ) -> Result<(Data, u16), AppError> {
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        let left_len = self.left.len();
        if c_id >= left_len {
            self.right
                .read_link_data((c_id - left_len, data_id), d_type)
        } else {
            self.left.read_link_data((c_id, data_id), d_type)
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

    pub fn link_transform_info_hashes(
        &self,
        c_id: ContentID,
        d_type: DataType,
    ) -> Result<Vec<Data>, AppError> {
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        let left_len = self.left.len();
        if c_id >= left_len {
            self.right
                .link_transform_info_hashes(c_id - left_len, d_type)
        } else {
            self.left.link_transform_info_hashes(c_id, d_type)
        }
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
    fn get_root_content_hash(&self, c_id: ContentID) -> Result<(DataType, u64), AppError> {
        if c_id >= self.data_count {
            return Err(AppError::IndexingError);
        }
        let left_len = self.left.len();
        // println!("c_id: {}, left_len: {}", c_id, left_len);
        if c_id >= left_len {
            self.right.get_root_content_typed_hash(c_id - left_len)
        } else {
            self.left.get_root_content_typed_hash(c_id)
        }
    }
}
