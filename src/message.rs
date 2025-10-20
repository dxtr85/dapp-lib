use crate::prelude::DataType;
use crate::SyncData;
use std::collections::HashMap;

use crate::Data;

use crate::content::ContentID;
use crate::ApplicationData;

#[derive(Clone)]
pub struct SyncRequirements {
    pub pre: Vec<(ContentID, u64)>,
    pub post: Vec<(ContentID, u64)>,
}

impl SyncRequirements {
    pub fn bytes(self) -> Vec<u8> {
        let mut bytes = vec![];
        //TODO: make sure we only have up to 256 pre & 256 post reqs
        bytes.push(self.pre.len() as u8);
        for (c_id, hash) in self.pre {
            let [c_one, c_two] = c_id.to_be_bytes();
            bytes.push(c_one);
            bytes.push(c_two);
            for byte in hash.to_be_bytes() {
                bytes.push(byte);
            }
        }
        bytes.push(self.post.len() as u8);
        for (c_id, hash) in self.post {
            let [c_one, c_two] = c_id.to_be_bytes();
            bytes.push(c_one);
            bytes.push(c_two);
            for byte in hash.to_be_bytes() {
                bytes.push(byte);
            }
        }
        bytes
    }

    // TODO: we need to add a read: Vec<Content_ID> argument
    // to verify that only specified contents were read
    // and that all specified contents were read
    pub fn pre_validate(&self, _c_id: ContentID, app: &ApplicationData) -> bool {
        for (c_id, hash) in self.pre.iter() {
            if let Ok((_d_type, d_hash)) = app.content_root_hash(*c_id) {
                if d_hash != *hash {
                    eprintln!(
                        "PRE validate\n Stored  hash: {},\nmessage hash: {}",
                        d_hash, hash
                    );
                    return false;
                }
            } else if *hash != 0 {
                eprintln!("Datastore has no CID: {}, hash: {}", c_id, hash);
                return false;
            }
        }
        true
    }

    // TODO: we need to add a changed: Vec<Content_ID> argument
    // to verify that only specified contents were changed
    // and that all specified contents were changed
    pub fn post_validate(&self, _c_id: ContentID, app: &ApplicationData) -> bool {
        for (c_id, hash) in self.post.iter() {
            if let Ok((_d_type, d_hash)) = app.content_root_hash(*c_id) {
                if d_hash != *hash {
                    eprintln!(
                        "POST validate\n{} stored  hash: {}={:?},\nmessage hash: {}={:?}",
                        c_id,
                        d_hash,
                        d_hash.to_be_bytes(),
                        hash,
                        hash.to_be_bytes()
                    );
                    return false;
                }
            } else if *hash != 0 {
                eprintln!("Datastore has no CID: {}, hash: {}", c_id, hash);
                return false;
            }
        }
        true
    }
}
// We need a high level way to manipulate Datastore elements.
// Manipulation can be done at two levels: Content, and specific Data within Content
// On Content level we can:
// - Add Content
// - Change Content (i.e. from Link to Data(x, _) or from Data(y, _d1) to Data(y, _d2))
// - TODO: Swap Content (switch places of two existing Contents with same Datatype)

// All other changes should be done on Data level:
// - Append Data to Content,
// - Remove Data from Content,
// - Update Data,
// - Insert Data,
// - Extend Data (append to existing Data newly received Data, total can not exceed 1024).

// Update data can internally have multiple Edits like InsertBytesAt, DeleteBytesFrom,
// ReplaceBytesAt and maybe more. These edits should always stay within one Data chunk.
// But this is on App developer to implement.

// Those messages can be split into multiple parts so we need to have them numbered
// and also identify their parts. It is allowed for multiple different Gnomes
// to post parts of the same message. This is to prevent stalling a Message,
// when an originating gnome drops out of swarm.
//
#[derive(Clone, Copy, Debug)]
pub enum ChangeContentOperation {
    DirectCTreeRebuild,
    DropAndAppend(u16),
    PopAndAppendConverted(u16),
    PopAndCTreeRebuild(u16),
}
impl ChangeContentOperation {
    pub fn from(bytes: &mut Vec<u8>) -> Result<Self, ()> {
        if bytes.is_empty() {
            eprintln!("Can not build from empty vec");
            return Err(());
        }
        let header = bytes.remove(0);

        match header {
            1 => Ok(Self::DirectCTreeRebuild),
            2 => {
                let b1 = bytes.remove(0);
                let b2 = bytes.remove(0);
                Ok(Self::DropAndAppend(u16::from_be_bytes([b1, b2])))
            }
            4 => {
                let b1 = bytes.remove(0);
                let b2 = bytes.remove(0);
                Ok(Self::PopAndAppendConverted(u16::from_be_bytes([b1, b2])))
            }
            8 => {
                let b1 = bytes.remove(0);
                let b2 = bytes.remove(0);
                Ok(Self::PopAndCTreeRebuild(u16::from_be_bytes([b1, b2])))
            }
            other => {
                eprintln!("Unexpected header byte: {}", other);
                bytes.insert(0, other);
                Err(())
            }
        }
    }
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Self::DirectCTreeRebuild => {
                //TODO
                vec![1]
            }
            Self::DropAndAppend(drop_how_many) => {
                //TODO
                let [b1, b2] = drop_how_many.to_be_bytes();
                vec![2, b1, b2]
            }
            Self::PopAndAppendConverted(pop_how_many) => {
                //TODO
                let [b1, b2] = pop_how_many.to_be_bytes();
                vec![4, b1, b2]
            }
            Self::PopAndCTreeRebuild(pop_how_many) => {
                //TODO
                let [b1, b2] = pop_how_many.to_be_bytes();
                vec![8, b1, b2]
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum SyncMessageType {
    // SetManifest, // Should this be a separate type?
    AppendContent(DataType),
    ChangeContent(ContentID, DataType, ChangeContentOperation),
    AppendData(ContentID),
    AppendShelledDatas(ContentID),
    RemoveData(ContentID, u16),
    UpdateData(ContentID, u16),
    InsertData(ContentID, u16),
    ExtendData(ContentID, u16),
    UserDefined(u8, u16, u16), // req_id, CID (2bytes!), DID
                               // Policy check will only recognize two bytes of CID, none of DID!
}
impl SyncMessageType {
    pub fn new(bytes: &mut Vec<u8>) -> Self {
        let value = bytes.drain(0..1).next().unwrap();
        match value {
            255 => {
                let b1 = bytes.drain(0..1).next().unwrap();
                let b2 = bytes.drain(0..1).next().unwrap();
                let c_id = u16::from_be_bytes([b1, b2]);
                SyncMessageType::AppendShelledDatas(c_id)
            }
            254 => {
                let dt = DataType::from(bytes.drain(0..1).next().unwrap());
                SyncMessageType::AppendContent(dt)
            }
            253 => {
                let b1 = bytes.drain(0..1).next().unwrap();
                let b2 = bytes.drain(0..1).next().unwrap();
                let d_type = DataType::from(bytes.drain(0..1).next().unwrap());
                let c_id = u16::from_be_bytes([b1, b2]);
                let operation = ChangeContentOperation::from(bytes).unwrap();
                SyncMessageType::ChangeContent(c_id, d_type, operation)
            }
            252 => {
                let b1 = bytes.drain(0..1).next().unwrap();
                let b2 = bytes.drain(0..1).next().unwrap();
                let c_id = u16::from_be_bytes([b1, b2]);
                // let b1 = bytes.drain(0..1).next().unwrap();
                // let b2 = bytes.drain(0..1).next().unwrap();
                // let d_id = u16::from_be_bytes([b1, b2]);
                SyncMessageType::AppendData(c_id)
            }

            251 => {
                let b1 = bytes.drain(0..1).next().unwrap();
                let b2 = bytes.drain(0..1).next().unwrap();
                let c_id = u16::from_be_bytes([b1, b2]);
                let b1 = bytes.drain(0..1).next().unwrap();
                let b2 = bytes.drain(0..1).next().unwrap();
                let d_id = u16::from_be_bytes([b1, b2]);
                SyncMessageType::RemoveData(c_id, d_id)
            }
            250 => {
                let b1 = bytes.drain(0..1).next().unwrap();
                let b2 = bytes.drain(0..1).next().unwrap();
                let c_id = u16::from_be_bytes([b1, b2]);
                let b1 = bytes.drain(0..1).next().unwrap();
                let b2 = bytes.drain(0..1).next().unwrap();
                let d_id = u16::from_be_bytes([b1, b2]);
                SyncMessageType::UpdateData(c_id, d_id)
            }
            249 => {
                let b1 = bytes.drain(0..1).next().unwrap();
                let b2 = bytes.drain(0..1).next().unwrap();
                let c_id = u16::from_be_bytes([b1, b2]);
                let b1 = bytes.drain(0..1).next().unwrap();
                let b2 = bytes.drain(0..1).next().unwrap();
                let d_id = u16::from_be_bytes([b1, b2]);
                SyncMessageType::InsertData(c_id, d_id)
            }

            248 => {
                let b1 = bytes.drain(0..1).next().unwrap();
                let b2 = bytes.drain(0..1).next().unwrap();
                let c_id = u16::from_be_bytes([b1, b2]);
                let b1 = bytes.drain(0..1).next().unwrap();
                let b2 = bytes.drain(0..1).next().unwrap();
                let d_id = u16::from_be_bytes([b1, b2]);
                SyncMessageType::ExtendData(c_id, d_id)
            }

            other => {
                let b1 = bytes.drain(0..1).next().unwrap();
                let b2 = bytes.drain(0..1).next().unwrap();
                let c_id = u16::from_be_bytes([b1, b2]);
                let b1 = bytes.drain(0..1).next().unwrap();
                let b2 = bytes.drain(0..1).next().unwrap();
                let d_id = u16::from_be_bytes([b1, b2]);
                // eprintln!("UserDefined from bytes: {other}, {c_id}, {d_id}");
                SyncMessageType::UserDefined(other, c_id, d_id)
            }
        }
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            SyncMessageType::AppendShelledDatas(c_id) => {
                let [b1, b2] = c_id.to_be_bytes();
                vec![255, b1, b2]
            }
            SyncMessageType::AppendContent(d_type) => vec![254, d_type.byte()],
            SyncMessageType::ChangeContent(c_id, d_type, operation) => {
                let [b1, b2] = c_id.to_be_bytes();
                let mut bvec = vec![253, b1, b2, d_type.byte()];
                bvec.append(&mut operation.bytes());
                bvec
            }
            SyncMessageType::AppendData(c_id) => {
                let [b1, b2] = c_id.to_be_bytes();
                // let [b3, b4] = d_id.to_be_bytes();
                vec![252, b1, b2]
            }
            SyncMessageType::RemoveData(c_id, d_id) => {
                let [b1, b2] = c_id.to_be_bytes();
                let [b3, b4] = d_id.to_be_bytes();
                vec![251, b1, b2, b3, b4]
            }

            SyncMessageType::UpdateData(c_id, d_id) => {
                let [b1, b2] = c_id.to_be_bytes();
                let [b3, b4] = d_id.to_be_bytes();
                vec![250, b1, b2, b3, b4]
            }
            SyncMessageType::InsertData(c_id, d_id) => {
                let [b1, b2] = c_id.to_be_bytes();
                let [b3, b4] = d_id.to_be_bytes();
                vec![249, b1, b2, b3, b4]
            }

            SyncMessageType::ExtendData(c_id, d_id) => {
                let [b1, b2] = c_id.to_be_bytes();
                let [b3, b4] = d_id.to_be_bytes();
                vec![248, b1, b2, b3, b4]
            }

            SyncMessageType::UserDefined(other, c_id, d_id) => {
                // eprintln!("UserDefined bytes: {other},{c_id},{d_id}");
                let [b1, b2] = c_id.to_be_bytes();
                let [b3, b4] = d_id.to_be_bytes();
                vec![*other, b1, b2, b3, b4]
            }
        }
    }
}

pub struct SyncMessage {
    pub m_type: SyncMessageType,
    pub requirements: SyncRequirements,
    pub data: Data,
}

impl SyncMessage {
    pub fn new(m_type: SyncMessageType, requirements: SyncRequirements, data: Data) -> Self {
        SyncMessage {
            m_type,
            requirements,
            data,
        }
    }

    pub fn into_parts(self) -> Vec<SyncData> {
        //TODO: here we need to determine how many parts will given message consist of
        // based on Data.len() and requirements size.
        // We also need to count 10 byte pairs of (c_id, hash) for each
        // subsequent part in part 0.
        // First partial message should not exceed 900 bytes total, in order to
        // carry PUBKEY DER.
        // Every partial message will consist of
        // - 1 byte of message type
        // - 2 bytes of part_no and total_parts
        let data_size = self.data.len();
        let req_size = 2 + 10 * (self.requirements.pre.len() + self.requirements.post.len());
        let netto_size = data_size + req_size;
        let mut partials = vec![];
        // eprintln!("data size: {}", data_size);
        // eprintln!("req size: {}", req_size);
        // eprintln!("netto size: {}", netto_size);

        // TODO/DONE? (was 897): recalculate how many bytes to drain, since now
        // message type can take up to 5 bytes, not 1 as previously
        // println!("Netto: {}", netto_size);
        if netto_size < 893 {
            // eprintln!("netto_size < 893 ");
            let mut bytes = Vec::with_capacity(netto_size + 3);
            for byte in self.m_type.as_bytes() {
                bytes.push(byte);
            }
            bytes.push(0);
            bytes.push(0);
            bytes.append(&mut self.requirements.bytes());
            bytes.append(&mut self.data.bytes());
            let data = SyncData::new(bytes).unwrap();
            partials.push(data);
            // partials.push(PartialMessage { m_type, part_no: 0, total_parts:0, data })
        } else {
            // eprintln!("netto_size ({}) >= 893 ", netto_size);
            let mut non_header_parts = 1;
            let mut done = false;
            while !done {
                let non_header_bytes_consumed = non_header_parts * 1021;
                // eprintln!("netto_size: {}", netto_size);
                // eprintln!("non_header_bytes_consumed : {}", non_header_bytes_consumed);
                // eprintln!("(893-) non_header_parts*8: {}", non_header_parts * 8);
                if netto_size < non_header_bytes_consumed
                    || netto_size - non_header_bytes_consumed <= 893 - (8 * non_header_parts)
                {
                    done = true;
                } else {
                    non_header_parts += 1;
                }
            }
            // eprintln!("Non header parts: {}", non_header_parts);
            // First we assume worst case scenario - 7 parts
            // Maximum size of SyncMessage is:
            // 1 byte for m_type
            // + 1 byte for pre req size
            // + 2560 bytes for pre reqs
            // + 1 byte for post req size
            // + 2560 bytes for postreqs
            // + 1024 bytes for Data
            // = 6147 bytes
            // (and we can add those 18 bytes 6165)
            // (and those 60 bytes 6225)
            // So there should never be more than 7 parts
            // We can reserve six 8-byte slots in first part for following parts' hashes
            // Partial message should only contain type, Data, part_no and total_parts
            // no other logical structures.
            // We only assemble SyncMessage from parts and then discover
            // hashes, Requirements and everything else.
            let hashes_size = 8 * non_header_parts;
            let mut header_bytes: Vec<u8> = Vec::with_capacity(900);
            // TODO/DONE? (was 897): recalculate how many bytes to drain, since now
            // message type can take up to 5 bytes, not 1 as previously
            for byte in self.m_type.as_bytes() {
                header_bytes.push(byte);
            }
            header_bytes.push(0);
            header_bytes.push(non_header_parts as u8);
            let first_chunk_size = 893 - hashes_size;
            // eprintln!("first_chunk_size: {}", first_chunk_size);
            // later chunks have up to 1021 bytes (3 bytes for type, part_no, total_parts)
            // First we put hashes (after we calculate them...)
            // Then we put requirements
            let mut req_and_data_bytes = self.requirements.bytes();
            // eprintln!("req_bytes: {}", req_and_data_bytes.len());
            // And at the end we put data_bytes
            let mut data_bytes = self.data.bytes();
            req_and_data_bytes.append(&mut data_bytes);
            // eprintln!("req_and_data_bytes: {}", req_and_data_bytes.len());
            let mut first_chunk_bytes: Vec<u8> =
                req_and_data_bytes.drain(0..first_chunk_size).collect();
            let mut subsequent_chunks: Vec<SyncData> = vec![];
            // TODO/DONE?(was 1021): recalculate how many bytes to drain, since now
            // message type can take up to 5 bytes, not 1 as previously
            for i in 0..non_header_parts {
                let mut vec = self.m_type.as_bytes();
                vec.push((i + 1) as u8);
                vec.push(non_header_parts as u8);
                let bytes_count = req_and_data_bytes.len();
                let drain_count = if bytes_count >= 1017 {
                    1017
                } else {
                    bytes_count
                };
                // eprintln!(
                //     "before drain, rem {} bytes from {} remaining",
                //     drain_count,
                //     req_and_data_bytes.len()
                // );
                vec.append(&mut req_and_data_bytes.drain(0..drain_count).collect());
                let data = SyncData::new(vec).unwrap();
                let hash = data.hash();
                eprintln!("into part hash: {}(len: {},\n{})", hash, data.len(), &data);
                for byte in hash.to_be_bytes() {
                    header_bytes.push(byte);
                }
                // eprintln!("Pushing {} bytes", data.len());
                subsequent_chunks.push(data);
            }
            header_bytes.append(&mut first_chunk_bytes);
            let header_data = SyncData::new(header_bytes).unwrap();
            // eprintln!("Pushing {} header bytes", header_data.len());
            partials.push(header_data);
            partials.append(&mut subsequent_chunks);
        }

        partials
    }

    // TODO: this needs rework
    pub fn from_data(idx: Vec<u64>, mut vec_data: HashMap<u64, Data>) -> Result<Self, ()> {
        // for (idx, data) in &vec_data {
        //     eprintln!("{} size {} bytes", idx, data.len());
        // }
        let idx_len = idx.len();
        if idx.len() != vec_data.len() {
            return Err(());
        }
        let mut total_bytes = Vec::with_capacity(idx_len * 1021);
        let mut idx_iter = idx.into_iter();
        let key = idx_iter.next().unwrap();
        let p_data = vec_data.remove(&key).unwrap();
        let mut header_bytes = p_data.bytes();
        // eprintln!("after remove header len: {}", header_bytes.len());
        let m_type = SyncMessageType::new(&mut header_bytes);
        // drop part_no & total_parts
        let _ = header_bytes.drain(0..2);
        // eprintln!("after remove part/total len: {}", header_bytes.len());

        let mut non_header_bytes = Vec::with_capacity((idx_len - 1) * 1021);
        for hash in idx_iter {
            let p_data = vec_data.remove(&hash).unwrap();
            let mut p_bytes = p_data.bytes();
            let _m_type = SyncMessageType::new(&mut p_bytes);
            let _ = p_bytes.drain(0..2);
            let _ = header_bytes.drain(0..8);
            non_header_bytes.append(&mut p_bytes);
        }
        total_bytes.append(&mut header_bytes);
        total_bytes.append(&mut non_header_bytes);
        let mut bytes_iter = total_bytes.into_iter();
        // let part_no = bytes_iter.next().unwrap();
        // let total_parts = bytes_iter.next().unwrap();
        // let mut part_hashes = None;
        let requirements = {
            // Requirements are defined in following way:
            // - first byte indicates number of pre requirements
            // - then there is a list of two byte CID followed by eight byte hash pairs
            // - after that there is again above procedure but for post requirements
            let pre_count = bytes_iter.next().unwrap();
            let mut pre = Vec::with_capacity(pre_count as usize);
            for _i in 0..pre_count {
                let b1 = bytes_iter.next().unwrap();
                let b2 = bytes_iter.next().unwrap();
                let c_id = u16::from_be_bytes([b1, b2]);
                let b1 = bytes_iter.next().unwrap();
                let b2 = bytes_iter.next().unwrap();
                let b3 = bytes_iter.next().unwrap();
                let b4 = bytes_iter.next().unwrap();
                let b5 = bytes_iter.next().unwrap();
                let b6 = bytes_iter.next().unwrap();
                let b7 = bytes_iter.next().unwrap();
                let b8 = bytes_iter.next().unwrap();
                let hash = u64::from_be_bytes([b1, b2, b3, b4, b5, b6, b7, b8]);
                pre.push((c_id, hash));
            }
            let post_count = bytes_iter.next().unwrap();
            let mut post = Vec::with_capacity(post_count as usize);
            for _i in 0..post_count {
                let b1 = bytes_iter.next().unwrap();
                let b2 = bytes_iter.next().unwrap();
                let c_id = u16::from_be_bytes([b1, b2]);
                let b1 = bytes_iter.next().unwrap();
                let b2 = bytes_iter.next().unwrap();
                let b3 = bytes_iter.next().unwrap();
                let b4 = bytes_iter.next().unwrap();
                let b5 = bytes_iter.next().unwrap();
                let b6 = bytes_iter.next().unwrap();
                let b7 = bytes_iter.next().unwrap();
                let b8 = bytes_iter.next().unwrap();
                let hash = u64::from_be_bytes([b1, b2, b3, b4, b5, b6, b7, b8]);
                post.push((c_id, hash));
            }
            SyncRequirements { pre, post }
        };
        // if part_no == 0 && total_parts>0 {
        //     let mut hashes_vec = Vec::with_capacity(total_parts as usize);
        //     for _i in 0..total_parts{
        //         let b1 = bytes_iter.next().unwrap();
        //         let b2 = bytes_iter.next().unwrap();
        //         let b3 = bytes_iter.next().unwrap();
        //         let b4 = bytes_iter.next().unwrap();
        //         let b5 = bytes_iter.next().unwrap();
        //         let b6 = bytes_iter.next().unwrap();
        //         let b7 = bytes_iter.next().unwrap();
        //         let b8 = bytes_iter.next().unwrap();
        //         let hash = u64::from_be_bytes([b1,b2,b3,b4,b5,b6,b7,b8]);
        //         hashes_vec.push(hash);
        //     }
        //     part_hashes = Some(hashes_vec);

        // }
        let bytes: Vec<u8> = bytes_iter.collect();
        // eprintln!("m_type: {:?}", m_type);
        // eprintln!("pre_req: {:?}", requirements.pre);
        // eprintln!("post_req: {:?}", requirements.post);
        // eprintln!("Bytes size: {}", bytes.len());
        let data = Data::new(bytes).unwrap();
        Ok(SyncMessage {
            m_type,
            requirements,
            data,
        })
    }
}
