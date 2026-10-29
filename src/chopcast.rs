use std::time::{Duration, Instant};

use gnome::prelude::{CastData, SwarmID};
use smol::{
    channel::{Receiver, Sender},
    Timer,
};

use crate::{chopper::extract_chop_data, ToAppData};

pub async fn handle_chopcast(
    // swarm_id: SwarmID,
    recv: Receiver<CastData>,
    to_app_data: Sender<ToAppData>,
) {
    // TODO: for larger files we might collect all pages
    // for one or more Contents before we AppendMultipleContents
    // to Datastore. dapp_lib should be able to cover this.
    // User should not have to worry about this ordering.
    eprintln!("In HANDLE_CHOPCAST");
    //
    // TODO:: send first leaf data at the end of transmission so that we know
    //        it is time to break the loop
    // let mut first_leaf_data = None;
    let mut leaf_hashes = Vec::with_capacity(512);
    let mut leaf_data = Vec::with_capacity(u16::MAX as usize);
    let mut last_hash_send = Instant::now() - Duration::from_secs(20);
    let mut hash_queue = vec![];
    loop {
        if let Ok(msg) = recv.recv().await {
            let ext_res = extract_chop_data(msg);
            // eprint!("ChopCast, ");
            if let Ok(chop) = ext_res {
                if chop.is_hashes() {
                    // eprintln!("hash, ");
                    let last_hash = chop.is_last_hash();
                    leaf_hashes.push(chop);
                    if last_hash {
                        eprintln!("last hash!");
                        hash_queue.push(ToAppData::StoreChopData(
                            // swarm_id,
                            std::mem::replace(&mut leaf_hashes, Vec::with_capacity(512)),
                        ));
                        // TODO: send what we have till now
                        // TODO: wait for about 20sec between msg sends
                        //       else push into a queue
                        // TODO: we get stuck in this loop without sending last batch of hashes
                        if last_hash_send.elapsed() > Duration::from_secs(20) {
                            if !hash_queue.is_empty() {
                                last_hash_send = Instant::now();
                                let _ = to_app_data.send(hash_queue.remove(0)).await;
                            }
                        }
                    }
                } else if chop.is_data() {
                    // eprintln!("data, ");
                    leaf_data.push(chop);
                } else if chop.is_seg_complete() {
                    // eprintln!("seg complete, ");
                    let (_ch_nr, _cnt_nr) = chop.unpack_seg_complete().unwrap();
                    if !leaf_data.is_empty() {
                        let _ = to_app_data
                            .send(ToAppData::StoreChopData(
                                // swarm_id,
                                std::mem::replace(&mut leaf_data, Vec::new()),
                            ))
                            .await;
                    }
                    if !hash_queue.is_empty() {
                        last_hash_send = Instant::now();
                        let _ = to_app_data.send(hash_queue.remove(0)).await;
                    }
                } else {
                    eprintln!("other, ");
                    //TODO: some other ChopData type?
                }
            } else {
                eprintln!("other2, ");
                //TODO: not ChopData received
            }
        } else {
            let err = recv.recv().await;
            eprintln!("Error RECV: {:?}", err.err().unwrap());
            // Do we get an error here when broadcast is terminated by origin?
            // TODO: send all we've got
            if !leaf_hashes.is_empty() {
                hash_queue.push(ToAppData::StoreChopData(
                    // swarm_id,
                    std::mem::replace(&mut leaf_hashes, Vec::new()),
                ));
            }
            if !leaf_data.is_empty() {
                let _ = to_app_data
                    .send(ToAppData::StoreChopData(
                        // swarm_id,
                        std::mem::replace(&mut leaf_data, Vec::new()),
                    ))
                    .await;
            }
            for msg in hash_queue {
                let _ = to_app_data.send(msg).await;
                Timer::after(Duration::from_secs(10)).await;
            }
            // eprintln!("break2");
            break;
        }
    }
    eprintln!("END of handle_bcast ");
}
