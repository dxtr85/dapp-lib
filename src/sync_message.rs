use crate::{content::DataType, prelude::ContentID, Data};
pub fn serialize_requests(requests: Vec<SyncRequest>) -> Vec<u8> {
    let mut bytes = vec![];
    for req in requests {
        match req {
            SyncRequest::Datastore => bytes.push(0),
            SyncRequest::AllFirstPages => bytes.push(1),
            SyncRequest::Hashes(c_id, d_type, hash_ids) => {
                bytes.push(2);
                let [c_1, c_2] = c_id.to_be_bytes();
                bytes.push(c_1);
                bytes.push(c_2);
                bytes.push(d_type.byte());
                let [hl_1, hl_2] = (hash_ids.len() as u16).to_be_bytes();
                bytes.push(hl_1);
                bytes.push(hl_2);
                for h_id in hash_ids {
                    let [h_1, h_2] = h_id.to_be_bytes();
                    bytes.push(h_1);
                    bytes.push(h_2);
                }
            }
            SyncRequest::Pages(c_id, d_type, page_ids) => {
                bytes.push(3);
                let [c_1, c_2] = c_id.to_be_bytes();
                bytes.push(c_1);
                bytes.push(c_2);
                bytes.push(d_type.byte());
                let [pl_1, pl_2] = (page_ids.len() as u16).to_be_bytes();
                bytes.push(pl_1);
                bytes.push(pl_2);
                for p_id in page_ids {
                    let [p_1, p_2] = p_id.to_be_bytes();
                    bytes.push(p_1);
                    bytes.push(p_2);
                }
            }
            SyncRequest::AllPages(c_ids) => {
                bytes.push(4);
                let [cl_1, cl_2] = (c_ids.len() as u16).to_be_bytes();
                bytes.push(cl_1);
                bytes.push(cl_2);
                for c_id in c_ids {
                    let [c_1, c_2] = c_id.to_be_bytes();
                    bytes.push(c_1);
                    bytes.push(c_2);
                }
            }
        }
    }
    bytes
}
pub fn deserialize_requests(bytes: Vec<u8>) -> Vec<SyncRequest> {
    let mut requests = vec![];
    let mut bytes_iter = bytes.into_iter();
    // let mut type_known = false;
    while let Some(req_type) = bytes_iter.next() {
        match req_type {
            0 => requests.push(SyncRequest::Datastore),
            1 => requests.push(SyncRequest::AllFirstPages),
            2 => {
                let c_1 = bytes_iter.next().unwrap();
                let c_2 = bytes_iter.next().unwrap();
                let c_id = u16::from_be_bytes([c_1, c_2]);
                let d_type = DataType::from(bytes_iter.next().unwrap());

                let hl_1 = bytes_iter.next().unwrap();
                let hl_2 = bytes_iter.next().unwrap();
                let hash_len = u16::from_be_bytes([hl_1, hl_2]);

                let mut hash_ids = vec![];
                for _i in 0..hash_len {
                    let h_1 = bytes_iter.next().unwrap();
                    let h_2 = bytes_iter.next().unwrap();
                    hash_ids.push(u16::from_be_bytes([h_1, h_2]));
                }
                requests.push(SyncRequest::Hashes(c_id, d_type, hash_ids));
            }
            3 => {
                let c_1 = bytes_iter.next().unwrap();
                let c_2 = bytes_iter.next().unwrap();
                let c_id = u16::from_be_bytes([c_1, c_2]);

                let d_type = bytes_iter.next().unwrap();

                let pl_1 = bytes_iter.next().unwrap();
                let pl_2 = bytes_iter.next().unwrap();
                let page_len = u16::from_be_bytes([pl_1, pl_2]);

                let mut page_ids = vec![];
                for _i in 0..page_len {
                    let p_1 = bytes_iter.next().unwrap();
                    let p_2 = bytes_iter.next().unwrap();
                    page_ids.push(u16::from_be_bytes([p_1, p_2]));
                }
                requests.push(SyncRequest::Pages(c_id, DataType::from(d_type), page_ids));
            }
            4 => {
                let cl_1 = bytes_iter.next().unwrap();
                let cl_2 = bytes_iter.next().unwrap();
                let c_ids_len = u16::from_be_bytes([cl_1, cl_2]);

                let mut c_ids = vec![];
                for _i in 0..c_ids_len {
                    let c_1 = bytes_iter.next().unwrap();
                    let c_2 = bytes_iter.next().unwrap();
                    c_ids.push(u16::from_be_bytes([c_1, c_2]));
                }
                requests.push(SyncRequest::AllPages(c_ids));
            }
            other => {
                println!("Unexpected byte: {}", other);
            }
        }
    }
    requests
}

pub enum SyncRequest {
    Datastore,
    AllFirstPages,
    Hashes(ContentID, DataType, Vec<u16>),
    Pages(ContentID, DataType, Vec<u16>),
    AllPages(Vec<ContentID>),
}

#[derive(Debug)]
pub enum SyncResponse {
    Datastore(u16, u16, Vec<(DataType, u64)>),
    Hashes(ContentID, u16, u16, Data),
    Page(ContentID, DataType, u16, u16, Data),
}

impl SyncResponse {
    pub fn serialize(self) -> Vec<u8> {
        match self {
            SyncResponse::Datastore(part_no, total, group) => {
                let [part_no_1, part_no_2] = (part_no as u16).to_be_bytes();
                let [total_1, total_2] = (total as u16).to_be_bytes();
                let mut byte_hashes = vec![];
                for (d_type, hash) in group.iter() {
                    byte_hashes.push(d_type.byte());
                    for byte in hash.to_be_bytes() {
                        byte_hashes.push(byte);
                    }
                }
                let mut bytes = vec![0, part_no_1, part_no_2, total_1, total_2];
                bytes.append(&mut byte_hashes);
                bytes
            }
            SyncResponse::Hashes(c_id, page_no, total, data) => {
                let mut bytes = Vec::with_capacity(1450);
                let [c_1, c_2] = c_id.to_be_bytes();
                let [page_1, page_2] = page_no.to_be_bytes();
                let [total_1, total_2] = total.to_be_bytes();
                bytes.push(1);
                bytes.push(c_1);
                bytes.push(c_2);
                bytes.push(page_1);
                bytes.push(page_2);
                bytes.push(total_1);
                bytes.push(total_2);
                bytes.append(&mut data.bytes());
                bytes
            }
            SyncResponse::Page(c_id, data_type, page_no, total, data) => {
                let mut bytes = Vec::with_capacity(1450);
                let [c_1, c_2] = c_id.to_be_bytes();
                let [page_1, page_2] = page_no.to_be_bytes();
                let [total_1, total_2] = total.to_be_bytes();
                bytes.push(2);
                bytes.push(c_1);
                bytes.push(c_2);
                bytes.push(data_type.byte());
                bytes.push(page_1);
                bytes.push(page_2);
                bytes.push(total_1);
                bytes.push(total_2);
                bytes.append(&mut data.bytes());
                bytes
            }
        }
    }

    pub fn deserialize(bytes: Vec<u8>) -> Result<Self, Vec<u8>> {
        let mut bytes_iter = bytes.into_iter();
        // let mut type_known = false;
        let resp_type = bytes_iter.next().unwrap();
        match resp_type {
            0 => {
                let p_1 = bytes_iter.next().unwrap();
                let p_2 = bytes_iter.next().unwrap();
                let part_no = u16::from_be_bytes([p_1, p_2]);
                let total_1 = bytes_iter.next().unwrap();
                let total_2 = bytes_iter.next().unwrap();
                let total = u16::from_be_bytes([total_1, total_2]);
                let mut typed_hashes = Vec::with_capacity(128);
                while let Some(d_type) = bytes_iter.next() {
                    let b1 = bytes_iter.next().unwrap();
                    let b2 = bytes_iter.next().unwrap();
                    let b3 = bytes_iter.next().unwrap();
                    let b4 = bytes_iter.next().unwrap();
                    let b5 = bytes_iter.next().unwrap();
                    let b6 = bytes_iter.next().unwrap();
                    let b7 = bytes_iter.next().unwrap();
                    let b8 = bytes_iter.next().unwrap();
                    typed_hashes.push((
                        DataType::from(d_type),
                        u64::from_be_bytes([b1, b2, b3, b4, b5, b6, b7, b8]),
                    ));
                }
                return Ok(Self::Datastore(part_no, total, typed_hashes));
            }
            1 => {
                let c_1 = bytes_iter.next().unwrap();
                let c_2 = bytes_iter.next().unwrap();
                let c_id = u16::from_be_bytes([c_1, c_2]);
                let p_1 = bytes_iter.next().unwrap();
                let p_2 = bytes_iter.next().unwrap();
                let page_no = u16::from_be_bytes([p_1, p_2]);
                let total_1 = bytes_iter.next().unwrap();
                let total_2 = bytes_iter.next().unwrap();
                let total = u16::from_be_bytes([total_1, total_2]);
                let mut bytes = Vec::with_capacity(1450);
                while let Some(byte) = bytes_iter.next() {
                    bytes.push(byte);
                }
                Ok(SyncResponse::Hashes(
                    c_id,
                    page_no,
                    total,
                    Data::new(bytes).unwrap(),
                ))
            }
            2 => {
                let c_1 = bytes_iter.next().unwrap();
                let c_2 = bytes_iter.next().unwrap();
                let c_id = u16::from_be_bytes([c_1, c_2]);
                let data_type = DataType::from(bytes_iter.next().unwrap());
                let p_1 = bytes_iter.next().unwrap();
                let p_2 = bytes_iter.next().unwrap();
                let page_no = u16::from_be_bytes([p_1, p_2]);
                let total_1 = bytes_iter.next().unwrap();
                let total_2 = bytes_iter.next().unwrap();
                let total = u16::from_be_bytes([total_1, total_2]);
                let mut bytes = Vec::with_capacity(1450);
                while let Some(byte) = bytes_iter.next() {
                    bytes.push(byte);
                }
                Ok(SyncResponse::Page(
                    c_id,
                    data_type,
                    page_no,
                    total,
                    Data::new(bytes).unwrap(),
                ))
            }
            other => {
                println!("Unexpected SyncResponse header: {}", other);
                let mut ret_bytes = vec![resp_type];
                for byte in bytes_iter {
                    ret_bytes.push(byte);
                }
                Err(ret_bytes)
            }
        }
    }
}
