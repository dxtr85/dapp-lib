use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use gnome::prelude::{Nat, NetworkSettings, PortAllocationRule, Transport};

use crate::storage::StoragePolicy;

pub struct Configuration {
    pub autosave: bool,
    pub work_dir: PathBuf,
    pub storage: PathBuf,
    pub neighbors: Option<Vec<NetworkSettings>>,
    pub max_connected_swarms: u8,
    pub upload_bandwidth: u64,
    pub store_data_on_disk: StoragePolicy,
}

impl Configuration {
    pub fn new(dir: PathBuf) -> Configuration {
        let conf_path = dir.join("dapp-lib.conf");
        let n_path = dir.join("neigh.conf");
        let neighbors = if n_path.exists() {
            Some(parse_neighbors(&n_path))
        } else {
            None
        };
        let mut autosave = false;
        let mut max_connected_swarms = 8;
        let mut upload_bandwidth = 8192;
        let mut store_data_on_disk = StoragePolicy::Everything;
        if !conf_path.exists() {
            return Configuration {
                autosave: false,
                work_dir: dir.clone(),
                storage: dir.join("storage"),
                neighbors,
                max_connected_swarms,
                upload_bandwidth,
                store_data_on_disk,
            };
        }
        let lines_iter = read_lines(conf_path).unwrap().into_iter();
        for line in lines_iter {
            let ls = line.unwrap().to_string();
            if ls.starts_with('#') || ls.is_empty() {
                eprintln!("Ignoring Line: {}", ls);
            } else {
                eprintln!("Parsing Line: {}", ls);
                let mut split = ls.split_whitespace();
                let line_header = split.next().unwrap();
                match line_header {
                    "AUTOSAVE" => {
                        eprintln!("Enabling AUTOSAVE");
                        autosave = true;
                    }
                    "MAX_CONNECTED_SWARMS" => {
                        if let Some(number_str) = split.next() {
                            if let Ok(number) = u8::from_str_radix(number_str, 10) {
                                eprintln!(
                                    "Updating MAX_CONNECTED_SWARMS from {} to {}",
                                    max_connected_swarms, number
                                );
                                max_connected_swarms = number;
                            }
                        }
                    }
                    "MAX_UPLOAD_BYTES_PER_SECOND" => {
                        if let Some(number_str) = split.next() {
                            if let Ok(number) = u64::from_str_radix(number_str, 10) {
                                eprintln!(
                                    "Updating MAX_UPLOAD_BYTES_PER_SECOND from {} to {}",
                                    upload_bandwidth, number
                                );
                                upload_bandwidth = number;
                            }
                        }
                    }
                    "STORE_DATA_ON_DISK" => {
                        if let Some(number_str) = split.next() {
                            if let Ok(number) = u8::from_str_radix(number_str, 10) {
                                match number {
                                    1 => store_data_on_disk = StoragePolicy::Everything,
                                    0 => store_data_on_disk = StoragePolicy::Datastore,
                                    other => {
                                        eprintln!(
                                            "Invalid config value for STORE_DATA_ON_DISK: {}",
                                            other
                                        );
                                        eprintln!("Allowed values: 0 - no data storage\n\t\t 1 - data storage enabled");
                                    }
                                }
                            }
                        }
                    }
                    other => {
                        eprintln!("Unrecognized config line: {}", other);
                    }
                }
            }
        }
        Configuration {
            autosave,
            work_dir: dir.clone(),
            storage: dir.join("storage"),
            neighbors,
            max_connected_swarms,
            upload_bandwidth,
            store_data_on_disk,
        }
    }
}

fn parse_neighbors(file: &Path) -> Vec<NetworkSettings> {
    let mut parsed_neighbors = vec![];
    let lines_iter = read_lines(file).unwrap().into_iter();
    for line in lines_iter {
        let ls = line.unwrap().to_string();
        if ls.starts_with('#') || ls.is_empty() {
            eprintln!("Ignoring Line: {}", ls);
        } else {
            eprintln!("Parsing Line: {}", ls);
            let mut split = ls.split_whitespace();
            let pub_ip;
            if let Some(ip) = split.next() {
                if let Ok(addr) = IpAddr::from_str(ip) {
                    pub_ip = addr;
                } else {
                    eprintln!("Failed at parsing IP addr, line {}", ls);
                    continue;
                }
            } else {
                continue;
            }
            let pub_port;
            if let Some(p) = split.next() {
                if let Ok(p) = u16::from_str(p) {
                    pub_port = p;
                } else {
                    eprintln!("Failed at parsing PORT, line {}", ls);
                    continue;
                }
            } else {
                continue;
            }
            let nat_type;
            if let Some(n) = split.next() {
                if let Ok(n) = u8::from_str(n) {
                    match n {
                        0 => nat_type = Nat::Unknown,
                        1 => nat_type = Nat::None,
                        2 => nat_type = Nat::FullCone,
                        4 => nat_type = Nat::AddressRestrictedCone,
                        8 => nat_type = Nat::PortRestrictedCone,
                        16 => nat_type = Nat::SymmetricWithPortControl,
                        32 => nat_type = Nat::Symmetric,
                        other => {
                            eprintln!("Unsupported value for Nat: {}", other);
                            nat_type = Nat::Unknown;
                        }
                    }
                } else {
                    eprintln!("Failed at parsing NAT, line {}", ls);
                    continue;
                }
            } else {
                continue;
            }
            let rule;
            if let Some(r) = split.next() {
                if let Ok(r) = u8::from_str(r) {
                    match r {
                        0 => rule = PortAllocationRule::Random,
                        1 => rule = PortAllocationRule::FullCone,
                        2 => rule = PortAllocationRule::AddressSensitive,
                        4 => rule = PortAllocationRule::PortSensitive,
                        other => {
                            eprintln!("Unsupported value for Port allocation rule: {}", other);
                            rule = PortAllocationRule::Random;
                        }
                    }
                } else {
                    eprintln!("Failed at parsing Port allocation rule, line {}", ls);
                    continue;
                }
            } else {
                continue;
            }
            let transport;
            if let Some(n) = split.next() {
                if let Ok(r) = u8::from_str(n) {
                    if let Ok(t) = Transport::from(r) {
                        transport = t;
                    } else {
                        eprintln!("Unsupported value for Transport: {}", r);
                        transport = Transport::UDPoverIP4;
                    }
                } else {
                    eprintln!("Failed at parsing Transport type, line {}", ls);
                    continue;
                }
            } else {
                continue;
            }
            eprintln!(
                "IP: {}, Port: {}, NAT: {:?}, rule: {:?}",
                pub_ip, pub_port, nat_type, rule
            );
            parsed_neighbors.push(NetworkSettings {
                pub_ip,
                pub_port,
                nat_type,
                port_allocation: (rule, 1), //TODO: value from file
                transport,
            });
        }
    }
    parsed_neighbors
}

fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(BufReader::new(file).lines())
}
