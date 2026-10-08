use crate::content::ContentTree;
pub use crate::data::Data;
use smol::channel as achannel;
use smol::channel::Receiver as AReceiver;
use smol::channel::Sender as ASender;
use smol::fs::File;
use smol::io::{AsyncReadExt as ReadExt, BufReader};
use std::path::PathBuf;

// This is where a Chopper tool will emerge.
// It's aim is to allow user loading files of various sizes
// and chopping them up into portions required by sync mechanism.
// Files of size of up to 113 * 64512 bytes (~ 7.2GB) can be synced
// using one message: AppendMultipleContentns.
// Larger files will require more of those messages.
//
// As an input Chopper will take an absolute path to a file on disk,
// and it will output a bunch of Data blocks.
//
// Every 7 GB segment will consist of:
// - 1 Data block with up to 113 CID root hashes;
// - for each of those root hashes there will be up to 512 Data
//   blocks with leaf hashes. Inside each leaf hash Data block there
//   is 128 8-byte hashes, so 512 * 128 = 65536, max CID capacity.

pub struct Chopper(PathBuf);

// In order to support varying sizes of resulting CIDs,
// we need to define a structure that will make sure those
// Data blocks are always in order.

#[derive(Debug)]
pub struct Chops {
    pub root_hashes: Data,
    pub leaf_hashes: Vec<Vec<Data>>,
}

impl Chopper {
    pub fn new(file_path: PathBuf) -> Option<Chopper> {
        if file_path.exists() {
            Some(Chopper(file_path))
        } else {
            None
        }
    }

    pub async fn chop_it(&self, data_sender: ASender<Data>) -> Vec<Chops> {
        let max_contents_in_chop = 113;
        let mut chop_no = 0;
        let mut content_no: u8 = 0;
        let mut page_no = 0;
        // let mut data_no = 0;
        // let mut index_discount = 0;
        let mut c_no_hash_pair_container: Vec<u8> = vec![];
        let mut hash_container: Vec<u8> = Vec::with_capacity(1024);
        let mut no_more_bytes_to_read = false;
        let mut c_tree: ContentTree = ContentTree::Empty(0);
        let mut leaf_hashes: Vec<Vec<Data>> = vec![vec![]];
        let mut c_leaf_hashes: Vec<Data> = Vec::with_capacity(512);
        let mut output: Vec<Chops> = vec![];

        let mut file = BufReader::new(File::open(self.0.clone()).await.unwrap());
        let _ = data_sender.send(Data::empty(3)).await;
        let _ = data_sender.send(Data::empty(2)).await;
        let _ = data_sender.send(Data::empty(1)).await;
        let mut counter = 0;
        let mut force_hash_conversion = false;
        while !no_more_bytes_to_read {
            if page_no == u16::MAX {
                // TODO: since we are indexing Data/Pages with u16
                // ContentTree can store only up to u16::MAX pages.
                eprintln!("CT full: {}", c_tree.len() == u16::MAX);
                force_hash_conversion = true;
                let _ = data_sender.send(Data::empty(0)).await;
                let _ = data_sender.send(Data::empty(0)).await;
                let _ = data_sender.send(Data::empty(0)).await;
                let _ = data_sender.send(Data::empty(3)).await;
                let _ = data_sender.send(Data::empty(2)).await;
                let _ = data_sender.send(Data::empty(1)).await;
            }
            if page_no > 0 && page_no % 128 == 0 || force_hash_conversion {
                if force_hash_conversion {
                    force_hash_conversion = false;
                    page_no = 0;
                }
                eprintln!("c{} page {} % 128 tree: {}", counter, page_no, c_tree.len());
                counter += 1;
                // We have 1024 bytes in hash_container,
                // let's build a Data block of it.
                let hdta = Data::new(std::mem::replace(
                    &mut hash_container,
                    Vec::with_capacity(1024),
                ))
                .unwrap();
                c_leaf_hashes.push(hdta);
                eprintln!("c_leaf_hashes size: {}", c_leaf_hashes.len());
                if c_leaf_hashes.len() == 512 {
                    eprintln!("CID is full");
                    // TODO: CID is full
                    let root_hash = c_tree.hash().to_be_bytes();
                    c_no_hash_pair_container.push(content_no);
                    c_no_hash_pair_container.extend(root_hash);

                    leaf_hashes.push(std::mem::replace(
                        &mut c_leaf_hashes,
                        Vec::with_capacity(512),
                    ));
                    c_tree = ContentTree::Empty(0);
                    page_no = 0;
                    if content_no > 0 && content_no % max_contents_in_chop == 0 {
                        chop_no += 1;
                        // TODO: here we need to construct a Chop result and add
                        // it to output.
                        let root_hashes = Data::new(std::mem::replace(
                            &mut c_no_hash_pair_container,
                            Vec::with_capacity(1024),
                        ))
                        .unwrap();
                        let chops = Chops {
                            root_hashes,
                            leaf_hashes: std::mem::replace(&mut leaf_hashes, vec![vec![]]),
                        };
                        output.push(chops);
                    }
                    content_no += 1;
                }
            }
            let dta = if page_no == 0 {
                eprintln!("crtee : {}", c_tree.len());
                // we are starting a new CID, so first we have to place
                // a text-based manifest page into it, so that we always know
                // what we are dealing with.
                //
                // index_discount += 1;

                let mut bts = format!("{:?} {}-{}", self.0, chop_no, content_no).into_bytes();
                for _i in 0..bts.len().saturating_sub(1024) {
                    eprintln!("drop a byte");
                    bts.remove(0);
                }
                Data::new(bts).unwrap()
            } else {
                let mut buffer: [u8; 1024] = [0; 1024];
                if let Ok(count) = file.read(&mut buffer).await {
                    if count < 1024 {
                        eprintln!("read bytes: {}", count);
                        no_more_bytes_to_read = true;
                        Data::new(Vec::from_iter(buffer.into_iter().take(count))).unwrap()
                    } else {
                        // eprintln!("read full buffer: {}", count);
                        Data::new(Vec::from(buffer)).unwrap()
                    }
                } else {
                    no_more_bytes_to_read = true;
                    Data::empty(0)
                }
            };
            // TODO: handle Data::empty(0)
            //
            if dta.is_empty() {
                break;
            }
            let hsh = dta.get_hash();
            let _ = data_sender.send(dta).await;
            let apd_res = c_tree.append(Data::empty(hsh));
            if apd_res.is_ok() {
                hash_container.extend(hsh.to_be_bytes());
                page_no += 1;
            } else {
                eprint!(
                    "CRASH  {} {:?} #: {}| ctree: {}",
                    page_no,
                    apd_res.err().unwrap(),
                    hsh,
                    c_tree.len()
                );
                break;
            }
        }

        let _ = data_sender.send(Data::empty(0)).await;
        let _ = data_sender.send(Data::empty(0)).await;
        let _ = data_sender.send(Data::empty(0)).await;
        // TODO: include remainder hashes
        if !hash_container.is_empty() {
            let hdta = Data::new(hash_container).unwrap();
            c_leaf_hashes.push(hdta);
            let mut last_lh = leaf_hashes.pop().unwrap();
            if last_lh.len() < 512 {
                last_lh.append(&mut c_leaf_hashes);
                leaf_hashes.push(last_lh);
            } else {
                leaf_hashes.push(c_leaf_hashes);
            }

            c_no_hash_pair_container.push(content_no);
            let root_hash = c_tree.hash().to_be_bytes();
            c_no_hash_pair_container.extend(root_hash);

            // TODO: here we need to construct a Chop result and add remainder
            let chops = Chops {
                root_hashes: Data::new(c_no_hash_pair_container).unwrap(),
                leaf_hashes,
            };
            output.push(chops);
        }

        output
    }
}
