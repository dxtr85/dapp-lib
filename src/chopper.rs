use crate::content::ContentTree;
pub use crate::data::Data;
use crate::{ToAppData, ToAppMgr};
use gnome::prelude::{CastData, CastID, SwarmID};
use smol::channel as achannel;
// use smol::channel::Receiver as AReceiver;
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

    pub async fn chop_it(&self, to_app_data: ASender<ToAppData>, cast_id: CastID) -> Vec<Chops> {
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
        let mut counter = 0;
        let mut force_hash_conversion = false;
        while !no_more_bytes_to_read {
            if page_no == u16::MAX {
                // TODO: since we are indexing Data/Pages with u16
                // ContentTree can store only up to u16::MAX pages.
                eprintln!("CT full: {}", c_tree.len() == u16::MAX);
                force_hash_conversion = true;
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
            let [p0, p1] = page_no.to_be_bytes();
            let mut c_vec = vec![
                'd' as u8, 'a' as u8, 't' as u8, 'a' as u8, content_no, p0, p1,
            ];
            c_vec.append(&mut dta.bytes());
            let c_data = CastData::new(c_vec).unwrap();
            let msg = ToAppData::BroadcastSend(cast_id, c_data);
            let _ = to_app_data.send(msg).await;

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
pub async fn chopping_task(
    swarm_id: SwarmID,
    filepath: PathBuf,
    to_app_mgr: achannel::Sender<ToAppMgr>,
    to_app_data: achannel::Sender<ToAppData>,
    data_cast_id: CastID,
    hash_cast_id: CastID,
) {
    eprintln!("chopping_task {:?}", filepath);
    let chopper = Chopper::new(filepath).unwrap();
    // Chopper::new(PathBuf::new().join("/home/dxtr/Downloads/testfile")).unwrap();
    // Chopper::new(PathBuf::new().join("/home/dxtr/Downloads/README.md")).unwrap();
    // Chopper::new(PathBuf::new().join("/home/dxtr/Downloads/pompka.mp4")).unwrap();
    // Chopper::new(PathBuf::new().join("/home/dxtr/Downloads/Mira-latest.AppImage")).unwrap();
    // TODO: Test it out with a file that is at least 7.3 GB large
    //       (after everything is moved to separate async tasks).
    eprint!("Now we apply chop_it(ASender):");
    // let (send, _recv) = unbounded();

    // TODO: Reading from file should also be done from a separate task.
    let mut chops = chopper.chop_it(to_app_data.clone(), data_cast_id).await;
    // TODO: handle resulting chops: put root_hashes in SyncMultipleContents.
    // TODO: decide what to do with leaf hashes - maybe we should send
    //       them in a separate BCast?
    // TODO: should leaf hashes be sent using a pair: (empty Data containing
    //       actual Data's index, actual Data)? This way Gnomes that join
    //       a BCast in the middle of transmission will be able to gather
    //       at least some of the hashes.
    //       Probably more efficient is to send a bunch of preamble
    //       Data::empty(33), Data::empty(22), Data::empty(11),
    //       Data::empyt(c_no),
    //       Data::empty(3), Data::empty(2), Data::empty(1),
    //       up to 512 Data blocks with Leaf Hashes for c_no
    //       Data::empty(0), Data::empty(0), Data::empty(0),
    //       And then repeat for next content, increasing c_no += 1.
    //       This broadcast could repeat this transmission in a loop
    //       for some time, so that everyone is eventually synced.
    eprintln!(
        "And we got: {}, {} ",
        chops.len(),
        // chops[0].root_hashes,
        chops[0].leaf_hashes.len()
    );
    let mut root_hashes: Vec<Data> = vec![];

    // <chop no < content no < page no >>>
    let mut leaf_hashes: Vec<Vec<Vec<Data>>> = vec![];
    while !chops.is_empty() {
        let chop = chops.remove(0);
        root_hashes.push(chop.root_hashes);
        leaf_hashes.push(chop.leaf_hashes);
    }
    let _ = to_app_mgr
        .send(ToAppMgr::FromDatastore(crate::LibResponse::ChoppingDone(
            swarm_id,
            data_cast_id,
            hash_cast_id,
            root_hashes,
        )))
        .await;
    if leaf_hashes.is_empty() {
        eprintln!("No leaf_hashes?!");
        return;
    }
    let chop_count = (leaf_hashes.len() - 1) as u8;
    for (chop_no, chop_hsh) in leaf_hashes.into_iter().enumerate() {
        let content_count = (chop_hsh.len() - 1) as u8;
        for (content_no, cnt_hash) in chop_hsh.into_iter().enumerate() {
            let page_count = (cnt_hash.len() - 1) as u16;
            for (page_no, bottom_hashes) in cnt_hash.into_iter().enumerate() {
                let [p0, p1] = (page_no as u16).to_be_bytes();
                let [pc0, pc1] = page_count.to_be_bytes();
                let mut h_vec = vec![
                    'h' as u8,
                    'a' as u8,
                    's' as u8,
                    'h' as u8,
                    chop_no as u8,
                    chop_count,
                    content_no as u8,
                    content_count,
                    p0,
                    p1,
                    pc0,
                    pc1,
                ];
                h_vec.append(&mut bottom_hashes.bytes());
                let h_data = CastData::new(h_vec).unwrap();
                let msg = ToAppData::BroadcastSend(hash_cast_id, h_data);
                let _ = to_app_data.send(msg).await;
            }
        }
    }

    // TODO: start a broadcasting channel and push file Data there
    //       (preferably as a separate async task)
    // TODO: Data will be broadcasted before CID placeholders are
    // synced in Datastore, so we need a way to store them somewhere
    // while let Ok(data) = _recv.try_recv() {
    //     if data.is_empty() {
    //         eprintln!("Empty data: {}", data.get_hash());
    //     } else {
    //         eprintln!("Data: {}", data.get_hash());
    //     }
    // }
    // eprintln!("And we got: {:?}", chops);
}
