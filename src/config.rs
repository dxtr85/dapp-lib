use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::{Chars, FromStr};

use gnome::prelude::{GnomeId, Nat, NetworkSettings, PortAllocationRule, SwarmName, Transport};

use crate::storage::{StorageCondition, StoragePolicy};

pub struct Configuration {
    pub autosave: bool,
    pub work_dir: PathBuf,
    pub storage: PathBuf,
    pub search: PathBuf,
    pub neighbors: Option<Vec<NetworkSettings>>,
    pub max_connected_swarms: u8,
    pub upload_bandwidth: u64,
    pub listen_port: Option<u16>,
    pub listen_port_ipv6: Option<u16>,
    pub storage_rules: Vec<(StorageCondition, StoragePolicy)>,
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
        let mut storage = dir.join("storage");
        let mut search = dir.join("search");
        let mut autosave = false;
        let mut max_connected_swarms = 8;
        let mut upload_bandwidth = 8192;
        let mut store_data_on_disk = vec![(StorageCondition::Default, StoragePolicy::All)];
        let mut listen_port = None;
        let mut listen_port_ipv6 = None;
        if !conf_path.exists() {
            return Configuration {
                autosave: false,
                work_dir: dir.clone(),
                storage,
                search,
                neighbors,
                max_connected_swarms,
                upload_bandwidth,
                listen_port,
                listen_port_ipv6,
                storage_rules: store_data_on_disk,
            };
        }
        let storage_rules_file_path = dir.join("storage.rules");
        if storage_rules_file_path.exists() {
            store_data_on_disk = read_storage_rules_from_file(storage_rules_file_path);
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
                    "LISTEN_PORT" => {
                        if let Some(number_str) = split.next() {
                            if let Ok(number) = u16::from_str_radix(number_str, 10) {
                                eprintln!("Updating LISTEN_PORT to {}", number);
                                listen_port = Some(number);
                            }
                        }
                    }
                    "LISTEN_PORT_IPV6" => {
                        if let Some(number_str) = split.next() {
                            if let Ok(number) = u16::from_str_radix(number_str, 10) {
                                eprintln!("Updating LISTEN_PORT_IPV6 to {}", number);
                                listen_port_ipv6 = Some(number);
                            }
                        }
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

                    "STORAGE_DIR" => {
                        if let Some(storage_str) = split.next() {
                            storage = PathBuf::from(storage_str);
                        }
                    }
                    "SEARCH_DIR" => {
                        if let Some(search_str) = split.next() {
                            search = PathBuf::from(search_str);
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
            storage,
            search,
            neighbors,
            max_connected_swarms,
            upload_bandwidth,
            listen_port,
            listen_port_ipv6,
            storage_rules: store_data_on_disk,
        }
    }
}

pub fn write_storage_rules_to_file(
    rules: &Vec<(StorageCondition, StoragePolicy)>,
    file_path: PathBuf,
) {
    //TODO
    eprintln!("in write_storage_rules_to_file");
    let mut entire_str = String::new();
    for (cond, pol) in rules {
        let line = format!("{} {}\n", cond.get_string(), pol.get_string());
        entire_str.push_str(&line);
    }
    let mut f = File::create(file_path).unwrap();
    let _ = write!(f, "{}", entire_str);
    drop(f);
    eprintln!("write is over");
}

pub fn read_storage_rules_from_file(file_path: PathBuf) -> Vec<(StorageCondition, StoragePolicy)> {
    let mut rules = vec![];
    // TODO: move storage rules to a separate file called 'storage.rules'
    let lines_iter = read_lines(file_path).unwrap().into_iter();
    for line in lines_iter {
        let ls = line.unwrap().to_string();
        if let Some(rule) = parse_for_storage_rule(ls) {
            eprintln!("Add storage rule");
            rules.push(rule);
        }
    }
    rules
}

fn parse_for_storage_rule(line: String) -> Option<(StorageCondition, StoragePolicy)> {
    if line.starts_with('#') || line.is_empty() {
        eprintln!("Ignoring Line: {}", line);
        None
    } else {
        eprintln!("Parsing Line: {}", line);
        let mut chars = line.trim().chars();
        let mut cond_chars: Vec<char> = vec![];
        let mut is_space_char = false;
        while !is_space_char {
            if let Some(char) = chars.next() {
                if char.is_whitespace() {
                    is_space_char = true;
                } else {
                    cond_chars.push(char);
                }
            } else {
                is_space_char = true;
            }
        }
        let cond_str: String = cond_chars.into_iter().collect();
        match cond_str.as_str() {
            "IamFounder" => {
                if let Some(pol) = read_policy(&mut chars) {
                    return Some((StorageCondition::IamFounder, pol));
                }
            }
            // FounderIs(GnomeId),
            "FounderIs" => {
                if let Some(g_id) = read_gnome_id(&mut chars) {
                    if let Some(pol) = read_policy(&mut chars) {
                        return Some((StorageCondition::FounderIs(g_id), pol));
                    }
                } else {
                    eprintln!("Failed reading Founder GnomeId");
                }
            }
            // SwarmName(SwarmName),
            "SwarmName" => {
                if let Some(g_id) = read_gnome_id(&mut chars) {
                    if let Some(name) = read_swarm_name(&mut chars) {
                        if let Some(pol) = read_policy(&mut chars) {
                            if let Ok(s_name) = SwarmName::new(g_id, name) {
                                return Some((StorageCondition::SwarmName(s_name), pol));
                            }
                        }
                    } else {
                        eprintln!("Failed reading SwarmName");
                    }
                } else {
                    eprintln!("Failed reading Founder GnomeId");
                }
            }
            // ,
            "CatalogApp" => {
                //TODO: read policy
                if let Some(pol) = read_policy(&mut chars) {
                    return Some((StorageCondition::CatalogApp, pol));
                }
            }
            // ,
            "ForumApp" => {
                //TODO: read policy
                if let Some(pol) = read_policy(&mut chars) {
                    return Some((StorageCondition::ForumApp, pol));
                }
            }
            // ,
            "SearchMatch" => {
                //TODO: read policy
                if let Some(pol) = read_policy(&mut chars) {
                    return Some((StorageCondition::SearchMatch, pol));
                }
            }
            // ,
            "Default" => {
                //TODO: read policy
                if let Some(pol) = read_policy(&mut chars) {
                    return Some((StorageCondition::Default, pol));
                }
            }
            _other => {
                eprintln!("Unrecognized StorageCondition: {_other}");
            }
        }
        None
    }
}

fn read_gnome_id(chars: &mut Chars) -> Option<GnomeId> {
    //skip all whitespace chars
    // read all non whitespace chars
    // try to construct GnomeId
    let mut non_whitespace_chars_started = false;
    let mut gid_chars = vec![];
    while !non_whitespace_chars_started {
        if let Some(char) = chars.next() {
            if !char.is_whitespace() {
                gid_chars.push(char);
                non_whitespace_chars_started = true;
            }
        }
    }
    while non_whitespace_chars_started {
        if let Some(char) = chars.next() {
            if char.is_whitespace() {
                non_whitespace_chars_started = false;
            } else {
                gid_chars.push(char);
            }
        }
    }
    GnomeId::from_string(gid_chars.into_iter().collect())
}

fn read_swarm_name(chars: &mut Chars) -> Option<String> {
    //skip all whitespace chars
    // read first non-whitespace char as name delimiter
    // read all non whitespace chars until delimiter
    // try to construct SwarmName (len <=32 bytes)
    let mut delimiter_determined = false;
    let mut delimiter = '"';
    while !delimiter_determined {
        if let Some(char) = chars.next() {
            if !char.is_whitespace() {
                delimiter = char;
                delimiter_determined = true;
            }
        }
    }
    let mut name_chars = vec![];
    while delimiter_determined {
        if let Some(char) = chars.next() {
            if char == delimiter {
                delimiter_determined = false;
            } else {
                name_chars.push(char);
            }
        }
    }
    let name: String = name_chars.into_iter().collect();
    if name.len() <= 32 {
        Some(name)
    } else {
        None
    }
}

fn read_policy(chars: &mut Chars) -> Option<StoragePolicy> {
    //skip all whitespace chars
    // read all non whitespace chars
    // try to construct GnomeId
    let mut non_whitespace_chars_started = false;
    let mut pol_chars = vec![];
    while !non_whitespace_chars_started {
        if let Some(char) = chars.next() {
            if !char.is_whitespace() {
                pol_chars.push(char);
                non_whitespace_chars_started = true;
            }
        } else {
            return None;
        }
    }
    while non_whitespace_chars_started {
        if let Some(char) = chars.next() {
            if char.is_whitespace() {
                non_whitespace_chars_started = false;
            } else {
                pol_chars.push(char);
            }
        } else {
            break;
        }
    }
    let pol_string: String = pol_chars.into_iter().collect();
    match pol_string.as_str() {
        "All" => Some(StoragePolicy::All),
        "Datastore" => Some(StoragePolicy::Datastore),
        "Manifest" => Some(StoragePolicy::Manifest),
        "FirstPages" => Some(StoragePolicy::FirstPages),
        "MatchOrFirstPages" => Some(StoragePolicy::MatchOrFirstPages),
        "MatchOrForget" => Some(StoragePolicy::MatchOrForget),
        "MatchAndManifestOrFirstPages" => Some(StoragePolicy::MatchAndManifestOrFirstPages),
        "MatchAndManifestOrForget" => Some(StoragePolicy::MatchAndManifestOrForget),
        "Forget" => Some(StoragePolicy::Forget),
        _other => {
            eprintln!("Unrecognized StoragePolicy: {_other}");
            None
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
