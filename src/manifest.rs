use crate::app_type::AppType;
use crate::Data;
use gnome::prelude::sha_hash;
use gnome::prelude::Nat;
use gnome::prelude::NetworkSettings;
// use gnome::prelude::NetworkSettings;
use gnome::prelude::PortAllocationRule;
use std::collections::HashMap;
use std::hash::Hash;
// use std::hash::{DefaultHasher, Hasher};
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;

#[derive(Clone, Debug)]
struct CombinedNetworkSettings {
    pub udp_ip4: Option<NetworkSettings>,
    pub tcp_ip4: Option<NetworkSettings>,
    pub udp_ip6: Option<NetworkSettings>,
    pub tcp_ip6: Option<NetworkSettings>,
}
impl CombinedNetworkSettings {
    pub fn new() -> Self {
        CombinedNetworkSettings {
            udp_ip4: None,
            tcp_ip4: None,
            udp_ip6: None,
            tcp_ip6: None,
        }
    }

    pub fn get_bytes(&self) -> Vec<u8> {
        // TODO: get all network settings as bytes
        // then push zero & number of those bytes
        // and the bytes as well
        let mut total_bytes = vec![0, 0];
        if let Some(ns) = self.udp_ip4 {
            total_bytes.append(&mut ns.clone().bytes());
        }
        if let Some(ns) = self.tcp_ip4 {
            total_bytes.append(&mut ns.clone().bytes());
        }
        if let Some(ns) = self.udp_ip6 {
            total_bytes.append(&mut ns.clone().bytes());
        }
        if let Some(ns) = self.tcp_ip6 {
            total_bytes.append(&mut ns.clone().bytes());
        }
        total_bytes[1] = (total_bytes.len() - 2) as u8;
        total_bytes
    }

    pub fn get_settings(&self) -> Vec<NetworkSettings> {
        let mut total_settings = vec![];
        if let Some(ns) = self.udp_ip4 {
            total_settings.push(ns.clone());
        }
        if let Some(ns) = self.tcp_ip4 {
            total_settings.push(ns.clone());
        }
        if let Some(ns) = self.udp_ip6 {
            total_settings.push(ns.clone());
        }
        if let Some(ns) = self.tcp_ip6 {
            total_settings.push(ns.clone());
        }
        total_settings
    }

    pub fn update(&mut self, new_settings: Vec<NetworkSettings>) -> bool {
        let mut updated = false;
        for ns in new_settings {
            match ns.transport {
                gnome::prelude::Transport::UDPoverIP4 => {
                    self.udp_ip4 = Some(ns);
                    updated = true;
                }
                gnome::prelude::Transport::TCPoverIP4 => {
                    self.tcp_ip4 = Some(ns);
                    updated = true;
                }
                gnome::prelude::Transport::UDPoverIP6 => {
                    self.udp_ip6 = Some(ns);
                    updated = true;
                }
                gnome::prelude::Transport::TCPoverIP6 => {
                    self.tcp_ip6 = Some(ns);
                    updated = true;
                }
            }
        }
        updated
    }
}
#[derive(Clone, Debug)]
pub struct Manifest {
    pub app_type: AppType,
    // pub pub_ips: Vec<(IpAddr, u16, Nat, (PortAllocationRule, i8))>,
    pub_ips: CombinedNetworkSettings,
    pub description: String,
    pub tags: HashMap<u8, Tag>,
    pub d_types: HashMap<u8, Tag>,
}
// TODO: a new Manifest definition, with attributes being added as needed during development
//  Manifest should apply to a Swarm, not an Application, application is defined in code
//  and can support different kinds of Swarms distinguished by their AppType
//  Manifest defines application type, tags, data structures, and message headers:
//  It consist of two elements: a 495-byte long header, and a HashMap<u8,u16>.
//  Header contains a general application description.
//  Mapping stores partial ContentIDs of locally stored Content that holds further
//  definitions required for application to function.
//  Those ContentIDs should all have the same Datatype value = 255.
//  Header may contain instructions on how to decrypt those Contents.
//  There can be up to 256 tags defined for a given swarm.
//  There can be up to 256 top level data structures defined in a single application.
//  There can be up to 256 top level synchronization messages defined.
//  There can be also some (less than 256) top level reconfiguration messages defined.
//  (We already have some built-in Reconfigs.)

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tag(pub String);
impl Tag {
    pub fn empty() -> Self {
        Tag(String::new())
    }
    pub fn new(name: String) -> Result<Self, ()> {
        if name.len() <= 32 {
            Ok(Tag(name))
        } else {
            Err(())
        }
    }
    pub fn bytes(&self) -> Vec<u8> {
        let len = self.0.len();
        let mut bytes = Vec::with_capacity(32);
        for _i in len..32 {
            bytes.push(32); //Fill with ' ' char
        }
        for byte in self.0.bytes() {
            bytes.push(byte);
        }
        bytes
    }
}
impl Manifest {
    pub fn new(app_type: AppType, tags: HashMap<u8, Tag>) -> Self {
        Manifest {
            app_type,
            pub_ips: CombinedNetworkSettings::new(),
            description: String::new(),
            tags,
            d_types: HashMap::new(),
        }
    }

    pub fn set_description(&mut self, text: String) -> bool {
        if text.len() > 1022 {
            return false;
        }
        self.description = text;
        true
    }

    // first Data consists only of:
    // - app_type
    // - tags count
    // - description
    pub fn first_hash(&self) -> u64 {
        // let mut hasher = DefaultHasher::new();
        let mut bytes = Vec::with_capacity(1024);
        bytes.push(self.app_type.byte());
        bytes.push(self.tags.len() as u8);
        //
        for byte in self.description.bytes() {
            bytes.push(byte);
        }

        // for i in 0..=31 {
        //     // bytes.push(i);
        //     if let Some(tag_name) = self.tags.get(&i) {
        //         // let tag_len = tag_name.0.len() as u8;
        //         // bytes.push(i);
        //         // bytes.push(tag_len);
        //         // for c in tag_name.0.as_bytes() {
        //         //     bytes.push(*c);
        //         // }
        //         bytes.append(&mut tag_name.bytes());
        //     }
        // }
        // println!("mani hash len: {}", bytes.len());
        // bytes.hash(&mut hasher);
        // hasher.finish()
        sha_hash(&bytes)
    }

    //TODO: There can be up to 256 Tags defined in a manifest,
    // each 32 bytes long, so a Manifest will be at most 9 Data chunks long
    // (later there can be some additions).
    // There is only one exception to this, when there are no Tags defined,
    // Then it can be only 1 Data long
    // If we add a single Tag, then Manifest has to expand to 2 Datas.
    // Tags are ordered from 0 to 255.
    // Each Tag is stored as a sequence of 32 bytes in Data, if all of these bytes are 0,
    // then given Tag is not defined, otherwise given Tag is defined.
    pub fn from(data_vec: Vec<Data>) -> Self {
        let data_count = data_vec.len();
        if data_count == 0 {
            return Manifest {
                app_type: AppType::Other(0),
                pub_ips: CombinedNetworkSettings::new(),
                description: String::new(),
                tags: HashMap::new(),
                d_types: HashMap::new(),
            };
        }
        // eprintln!("Constructing manifest from: {} Data blocks", data_count);
        let mut data_iter = data_vec.into_iter();
        let first_data = data_iter.next().unwrap();
        let mut iter = first_data.bytes().into_iter();
        if data_count == 1 && iter.len() == 1 {
            return Manifest {
                app_type: AppType::from(iter.next().unwrap()),
                pub_ips: CombinedNetworkSettings::new(),
                description: String::new(),
                tags: HashMap::new(),
                d_types: HashMap::new(),
            };
        }
        let app_type_byte = iter.next();
        if app_type_byte.is_none() {
            return Manifest {
                app_type: AppType::Other(0),
                pub_ips: CombinedNetworkSettings::new(),
                description: String::new(),
                tags: HashMap::new(),
                d_types: HashMap::new(),
            };
        }
        let app_type = AppType::from(app_type_byte.unwrap());
        // let tags_count = if let Some(byte) = iter.next() {
        //     byte as usize
        // } else {
        //     0
        // };
        let first_tags_page = u16::from_be_bytes([iter.next().unwrap(), iter.next().unwrap()]);
        let first_dt_page = u16::from_be_bytes([iter.next().unwrap(), iter.next().unwrap()]);
        let other_defs = u16::from_be_bytes([iter.next().unwrap(), iter.next().unwrap()]);
        let tag_pages_count = if first_tags_page == 0 {
            0
        } else {
            if first_dt_page == 0 && other_defs == 0 {
                data_count - 1
            } else if first_dt_page > 0 {
                first_dt_page as usize - first_tags_page as usize
            } else {
                other_defs as usize - first_tags_page as usize
            }
        };
        let mut tags = HashMap::with_capacity(tag_pages_count << 5);
        // let d_types_count = if let Some(byte) = iter.next() {
        //     byte as usize
        // } else {
        //     0
        // };
        let dt_page_count = if other_defs == 0 {
            data_count - first_dt_page as usize
        } else {
            other_defs as usize - first_dt_page as usize
        };

        let mut d_types = HashMap::with_capacity(dt_page_count << 5);
        let mut pub_ips = CombinedNetworkSettings::new();
        // First byte in UTF-8 encoded String can not start with a 1 bit, so we are good
        // TODO: read one byte and check its value
        // if it's higher than 251 use old logic
        // otherwise it indicates how many following bytes need to be read
        // in order to retrieve pub_ips
        // read that many and update pub_ips
        if let Some(next_byte) = iter.clone().peekable().next() {
            eprintln!("Next_byte: {}", next_byte);
            if next_byte == 255 || next_byte == 254 || next_byte == 253 || next_byte == 252 {
                match iter.next().unwrap() {
                    255 => {
                        eprintln!("255 we have IPv4 & IPv6");
                        // first read IPv4 address, port,Nat,PortAllocationRule,step
                        let (pub_ip, pub_port, nat_type, port_allocation) = read_ipv4(&mut iter);
                        let ns = NetworkSettings {
                            pub_ip,
                            pub_port,
                            nat_type,
                            port_allocation,
                            transport: gnome::prelude::Transport::UDPoverIP4,
                        };
                        // pub_ips.push(read_ipv4(&mut iter));
                        pub_ips.update(vec![ns]);
                        // second read IPv6 address and port, we assume all IPv6 to be public
                        // pub_ips.push(read_ipv6(&mut iter));
                        let (pub_ip, pub_port, nat_type, port_allocation) = read_ipv6(&mut iter);
                        let ns = NetworkSettings {
                            pub_ip,
                            pub_port,
                            nat_type,
                            port_allocation,
                            transport: gnome::prelude::Transport::UDPoverIP4,
                        };
                        pub_ips.update(vec![ns]);
                    }
                    254 => {
                        eprintln!("254 IPv6 only");
                        //     254 IPv6 only
                        // pub_ips.push(read_ipv6(&mut iter));
                        let (pub_ip, pub_port, nat_type, port_allocation) = read_ipv6(&mut iter);
                        let ns = NetworkSettings {
                            pub_ip,
                            pub_port,
                            nat_type,
                            port_allocation,
                            transport: gnome::prelude::Transport::UDPoverIP4,
                        };
                        pub_ips.update(vec![ns]);
                    }
                    253 => {
                        eprintln!("253 IPv4 only");
                        //     253 IPv4 only
                        let (pub_ip, pub_port, nat_type, port_allocation) = read_ipv4(&mut iter);
                        let ns = NetworkSettings {
                            pub_ip,
                            pub_port,
                            nat_type,
                            port_allocation,
                            transport: gnome::prelude::Transport::UDPoverIP4,
                        };
                        // pub_ips.push(read_ipv4(&mut iter));
                        pub_ips.update(vec![ns]);
                    }
                    _ => {
                        eprintln!("252 No Public IPs defined");
                        //     252 No Public IPs defined
                    }
                }
            } else if next_byte == 0 {
                let next_byte = iter.next().unwrap();
                let next_byte = iter.next().unwrap();
                let mut ns_bytes = Vec::with_capacity(next_byte as usize);
                eprintln!(
                    "We are using new method to load pub_ips from storage {}b",
                    next_byte
                );
                for _i in 0..next_byte {
                    ns_bytes.push(iter.next().unwrap());
                }
                let nss = NetworkSettings::from(&ns_bytes);
                pub_ips.update(nss);
            }
        }
        eprintln!("Loading Description…");
        let description = String::from_utf8(iter.collect()).unwrap();

        let mut current_tag_id: u8 = 0;
        let mut tag_pages_read = 0;
        // let mut current_dtype_id = 0;
        let mut dtype_pages_read = 0;
        let mut adding_tags = true;
        while let Some(data) = data_iter.next() {
            let bytes = data.bytes();
            for chunk in bytes.chunks_exact(32) {
                let mut all_zeros = true;
                let mut non_space_byte_occured = false;
                let mut name_bytes = Vec::with_capacity(32);
                for byte in chunk {
                    if *byte > 0 {
                        all_zeros = false;
                    }
                    if *byte == 32 {
                        if non_space_byte_occured {
                            name_bytes.push(*byte);
                        }
                    } else {
                        non_space_byte_occured = true;
                        name_bytes.push(*byte);
                    }
                }
                if !all_zeros {
                    let tag = Tag::new(String::from_utf8(name_bytes).unwrap()).unwrap();
                    if adding_tags {
                        tags.insert(current_tag_id, tag);
                    } else {
                        d_types.insert(current_tag_id, tag);
                    }
                    // tags_added = tags_added + 1;
                }
                current_tag_id = current_tag_id.saturating_add(1);
                // if adding_tags && tags_added >= tags_count {
                //     adding_tags = false;
                //     tags_added = 0;
                //     current_tag_id = 0;
                // }
            }
            if adding_tags {
                tag_pages_read = tag_pages_read + 1;
                if tag_pages_read >= tag_pages_count {
                    adding_tags = false;
                    current_tag_id = 0;
                }
            } else {
                dtype_pages_read = dtype_pages_read + 1;
                if dtype_pages_read >= dt_page_count {
                    // We don't want to tread other data as data type defs
                    break;
                }
            }
        }
        // TODO: read other data if any!
        Self {
            app_type,
            pub_ips,
            description,
            tags,
            d_types,
        }
    }

    pub fn to_data(&self) -> Vec<Data> {
        let mut res = Vec::with_capacity(1024);
        res.push(self.app_type.byte());
        let tags_len = self.tags.len() as u8;
        res.push(0);
        if tags_len == 0 {
            res.push(0);
        } else {
            res.push(1);
        }
        let d_types_len = self.d_types.len() as u8;
        res.push(0);
        if d_types_len == 0 {
            res.push(0);
        } else {
            //TODO: Calculate d_id of first page containing user defined data types
            // it is right after Tags pages
            let tags_pages_count = tags_len >> 4 + if tags_len % 32 > 0 { 1 } else { 0 };
            res.push(tags_pages_count + 1);
        }
        // TODO: index of other data after Data type definitions, 0 if none
        res.push(0);
        res.push(0);
        res.append(&mut self.pub_ips.get_bytes());
        // match self.pub_ips.len() {
        //     0 => res.push(252),
        //     1 => {
        //         // TODO: store multiple IP addresses
        //         // TODO: store trasnport protocol for each address
        //         // let (pub_ip, port, nat, (rule, delta)) = self.pub_ips[0];
        //         let ns = self.pub_ips[0];
        //         match ns.pub_ip {
        //             IpAddr::V4(ip) => {
        //                 res.push(253);
        //                 for octet in ip.octets() {
        //                     res.push(octet);
        //                 }
        //                 for byte in ns.pub_port.to_be_bytes() {
        //                     res.push(byte);
        //                 }
        //                 res.push(ns.nat_type as u8);
        //                 res.push(ns.port_allocation.0 as u8);
        //                 res.push(ns.port_allocation.1 as u8);
        //             }
        //             IpAddr::V6(ip) => {
        //                 res.push(254);
        //                 for octet in ip.octets() {
        //                     res.push(octet);
        //                 }
        //                 for byte in ns.pub_port.to_be_bytes() {
        //                     res.push(byte);
        //                 }
        //                 res.push(ns.nat_type as u8);
        //                 res.push(ns.port_allocation.0 as u8);
        //                 res.push(ns.port_allocation.1 as u8);
        //             }
        //         }
        //     }
        //     2 => {
        //         res.push(255);
        //         let (ip4, ip6) = if self.pub_ips[0].pub_ip.is_ipv4() {
        //             (self.pub_ips[0], self.pub_ips[1])
        //         } else {
        //             (self.pub_ips[1], self.pub_ips[0])
        //         };
        //         if let IpAddr::V4(ip) = ip4.pub_ip {
        //             for octet in ip.octets() {
        //                 res.push(octet);
        //             }
        //             for byte in ip4.pub_port.to_be_bytes() {
        //                 res.push(byte);
        //             }
        //             res.push(ip4.nat_type as u8);
        //             res.push(ip4.port_allocation.0 as u8);
        //             res.push(ip4.port_allocation.1 as u8);
        //         }
        //         if let IpAddr::V6(ip) = ip6.pub_ip {
        //             for octet in ip.octets() {
        //                 res.push(octet);
        //             }
        //             for byte in ip6.pub_port.to_be_bytes() {
        //                 res.push(byte);
        //             }
        //             res.push(ip6.nat_type as u8);
        //             res.push(ip6.port_allocation.0 as u8);
        //             res.push(ip6.port_allocation.1 as u8);
        //         }
        //     }
        //     other => {
        //         eprintln!("Too many public IPs: {}", other);
        //     }
        // }
        for byte in self.description.bytes() {
            res.push(byte);
        }
        let first_data_bytes = std::mem::replace(&mut res, Vec::with_capacity(1024));
        let mut output = vec![Data::new(first_data_bytes).unwrap()];
        let mut tags_to_add = self.tags.len();
        let mut elements_pushed = 0;
        for i in 0..=255 {
            if let Some(tag) = self.tags.get(&i) {
                res.append(&mut tag.bytes());
                tags_to_add -= 1;
            } else {
                res.append(&mut vec![0; 32]);
            }
            elements_pushed += 1;
            if tags_to_add == 0 {
                break;
            }
            if elements_pushed >= 32 {
                elements_pushed = 0;
                let next_data_bytes = std::mem::replace(&mut res, Vec::with_capacity(1024));
                output.push(Data::new(next_data_bytes).unwrap());
            }
        }
        if res.len() > 0 {
            let next_data_bytes = std::mem::replace(&mut res, Vec::with_capacity(1024));
            output.push(Data::new(next_data_bytes).unwrap());
        }
        let mut d_types_to_add = self.d_types.len();
        let mut elements_pushed = 0;
        for i in 0..=255 {
            if let Some(tag) = self.d_types.get(&i) {
                res.append(&mut tag.bytes());
                d_types_to_add -= 1;
            } else {
                res.append(&mut vec![0; 32]);
            }
            elements_pushed += 1;
            if d_types_to_add == 0 {
                break;
            }
            if elements_pushed >= 32 {
                elements_pushed = 0;
                let next_data_bytes = std::mem::replace(&mut res, Vec::with_capacity(1024));
                output.push(Data::new(next_data_bytes).unwrap());
            }
        }
        if res.len() > 0 {
            let next_data_bytes = std::mem::replace(&mut res, Vec::with_capacity(1024));
            output.push(Data::new(next_data_bytes).unwrap());
        }

        output
    }

    pub fn add_tags(&mut self, tags: Vec<Tag>) -> bool {
        eprintln!("add_tags {:?}", tags);
        let mut any_tag_added = false;
        let mut existing_tags = Vec::with_capacity(self.tags.len());
        for (_id, tag) in &self.tags {
            // eprintln!("add_tags existing:{} {}", _id, tag.0);
            existing_tags.push(tag.clone());
        }
        let mut tags_to_add = vec![];
        let tags_iter = tags.into_iter();
        for tag in tags_iter {
            if tag.0.is_empty() {
                continue;
            }
            if !existing_tags.contains(&tag) {
                // eprintln!("add_tags new: {}", tag.0);
                tags_to_add.push(tag);
            }
        }
        let mut tags_iter = tags_to_add.into_iter();
        let mut last_id_checked = 0;
        while let Some(tag) = tags_iter.next() {
            for i in last_id_checked..=255 {
                if self.tags.contains_key(&i) {
                    continue;
                }
                any_tag_added = true;
                // eprintln!("add_tags insert:{} {}", i, tag.0);
                self.tags.insert(i, tag);
                last_id_checked = i;
                break;
            }
        }
        any_tag_added
    }

    pub fn tags_string(&self, tag_ids: &Vec<u8>) -> String {
        let mut tstring = String::new();
        for id in tag_ids {
            if let Some(tag) = self.tags.get(id) {
                tstring.push_str(&tag.0);
                tstring.push(' ');
                tstring.push(' ');
                tstring.push(' ');
                tstring.push(' ');
            }
        }
        tstring
    }
    pub fn add_data_type(&mut self, tag: Tag) -> bool {
        if tag.0.is_empty() {
            return false;
        }
        eprintln!("Adding dtype {} to manifest", tag.0);
        let mut added = false;
        if self.d_types.len() >= 256 {
            return added;
        }
        for i in 0..=255 {
            if !self.d_types.contains_key(&i) {
                self.d_types.insert(i, tag);
                added = true;
                break;
            }
        }
        added
    }
    pub fn dtype_string(&self, dtype_id: u8) -> String {
        eprintln!("Searching for DType name for {}", dtype_id);
        if dtype_id == 255 {
            return format!("Link");
        }
        let mut tstring = String::new();
        if let Some(tag) = self.d_types.get(&dtype_id) {
            eprintln!("Found: {}", tag.0);
            tstring.push_str(&tag.0);
        } else {
            eprintln!("Not found: {:?}", self.d_types.keys());
        }
        tstring
    }

    // TODO: before deleting a Tag make sure there is no Content labeled with it
    // If there is any, first modify all Content's headers to drop that label
    // on only then call this fn
    pub fn del_tags(&mut self, tags: Vec<Tag>) {
        for id in 0..=255 {
            if let Some(tag) = self.tags.get(&id) {
                if tags.contains(tag) {
                    self.tags.remove(&id);
                }
            }
        }
    }
    pub fn rename_tags(&mut self, mut tags: HashMap<Tag, Tag>) {
        for id in 0..=255 {
            if let Some(tag) = self.tags.get(&id) {
                if tags.contains_key(tag) {
                    let new_tag = tags.remove(&tag).unwrap();
                    self.tags.insert(id, new_tag);
                }
            }
            if tags.is_empty() {
                break;
            }
        }
    }
    pub fn tag_names(&self, filter: Option<Vec<u8>>) -> Vec<String> {
        if let Some(filter) = filter {
            let mut tag_names = Vec::with_capacity(filter.len());
            for id in filter {
                if let Some(tag) = self.tags.get(&id) {
                    tag_names.push(tag.0.clone());
                }
            }
            return tag_names;
        }
        let mut tag_names = Vec::with_capacity(256);
        for id in 0..=255 {
            if let Some(tag) = self.tags.get(&id) {
                tag_names.push(tag.0.clone());
            }
        }
        tag_names
    }
    pub fn dtype_names(&self) -> Vec<String> {
        let mut type_names = Vec::with_capacity(256);
        for id in 0..=255 {
            if let Some(tag) = self.d_types.get(&id) {
                type_names.push(tag.0.clone());
            }
        }
        type_names
    }

    pub fn get_pub_ips(&self) -> Vec<NetworkSettings> {
        self.pub_ips.get_settings()
    }

    pub fn update_pub_ips(
        &mut self,
        // ips: Vec<(IpAddr, u16, Nat, (PortAllocationRule, i8))>,
        ips: Vec<NetworkSettings>,
    ) -> bool {
        self.pub_ips.update(ips)
    }
    // pub fn update_pub_ips(
    //     &mut self,
    //     // ips: Vec<(IpAddr, u16, Nat, (PortAllocationRule, i8))>,
    //     ips: Vec<NetworkSettings>,
    // ) -> bool {
    //     // eprintln!("We should update Public IPs with \n{:?}", ips);
    //     if self.pub_ips.is_empty() {
    //         self.pub_ips = ips;
    //         eprintln!("Pub IPs updated 1");
    //         return true;
    //     }
    //     let mut ips_to_add = vec![];
    //     for ip in ips {
    //         if !self.pub_ips.contains(&ip) {
    //             ips_to_add.push(ip);
    //         }
    //     }
    //     if ips_to_add.is_empty() {
    //         // eprintln!("Public IPs are up to date");
    //         return false;
    //     }
    //     // We need to replace old IPv4 with new IPv4 only
    //     // and old IPv6 with new IPv6 only, other replacements are not allowed!
    //     // we can not have more than two Public IPs one for each protocol version

    //     eprintln!("We should add some IPs: \n{:?}", ips_to_add);
    //     let mut ip4idx = None;
    //     let mut ip6idx = None;
    //     for (idx, ns) in self.pub_ips.iter().enumerate() {
    //         if ns.pub_ip.is_ipv4() {
    //             ip4idx = Some(idx);
    //         } else {
    //             ip6idx = Some(idx);
    //         }
    //     }
    //     for ip in ips_to_add {
    //         if ip.pub_ip.is_ipv4() {
    //             if let Some(idx) = ip4idx {
    //                 self.pub_ips[idx] = ip;
    //             } else {
    //                 self.pub_ips.push(ip);
    //                 // Just in case we were given two IP4s
    //                 ip4idx = Some(self.pub_ips.len() - 1);
    //             }
    //         } else {
    //             if let Some(idx) = ip6idx {
    //                 self.pub_ips[idx] = ip;
    //             } else {
    //                 self.pub_ips.push(ip);
    //                 // Just in case we were given two IP6s
    //                 ip6idx = Some(self.pub_ips.len() - 1);
    //             }
    //         }
    //     }
    //     eprintln!("Pub IPs updated 2");
    //     true
    // }
}
fn read_ipv4<T>(iter: &mut T) -> (IpAddr, u16, Nat, (PortAllocationRule, i8))
where
    T: Iterator<Item = u8>,
{
    let ip1 = iter.next().unwrap();
    let ip2 = iter.next().unwrap();
    let ip3 = iter.next().unwrap();
    let ip4 = iter.next().unwrap();
    let port1 = iter.next().unwrap();
    let port2 = iter.next().unwrap();
    let nat = iter.next().unwrap();
    let port_alloc = iter.next().unwrap();
    let port_step = iter.next().unwrap() as i8;
    (
        IpAddr::V4(Ipv4Addr::new(ip1, ip2, ip3, ip4)),
        u16::from_be_bytes([port1, port2]),
        Nat::from(nat),
        (PortAllocationRule::from(port_alloc), port_step),
    )
}
fn read_ipv6<T>(iter: &mut T) -> (IpAddr, u16, Nat, (PortAllocationRule, i8))
where
    T: Iterator<Item = u8>,
{
    let ip1 = iter.next().unwrap();
    let ip2 = iter.next().unwrap();
    let ip3 = iter.next().unwrap();
    let ip4 = iter.next().unwrap();
    let ip5 = iter.next().unwrap();
    let ip6 = iter.next().unwrap();
    let ip7 = iter.next().unwrap();
    let ip8 = iter.next().unwrap();
    let ip9 = iter.next().unwrap();
    let ip10 = iter.next().unwrap();
    let ip11 = iter.next().unwrap();
    let ip12 = iter.next().unwrap();
    let ip13 = iter.next().unwrap();
    let ip14 = iter.next().unwrap();
    let ip15 = iter.next().unwrap();
    let ip16 = iter.next().unwrap();
    let port1 = iter.next().unwrap();
    let port2 = iter.next().unwrap();
    let nat = iter.next().unwrap();
    let port_alloc = iter.next().unwrap();
    let port_step = iter.next().unwrap() as i8;
    (
        IpAddr::V6(Ipv6Addr::new(
            u16::from_be_bytes([ip1, ip2]),
            u16::from_be_bytes([ip3, ip4]),
            u16::from_be_bytes([ip5, ip6]),
            u16::from_be_bytes([ip7, ip8]),
            u16::from_be_bytes([ip9, ip10]),
            u16::from_be_bytes([ip11, ip12]),
            u16::from_be_bytes([ip13, ip14]),
            u16::from_be_bytes([ip15, ip16]),
        )),
        u16::from_be_bytes([port1, port2]),
        Nat::from(nat),
        (PortAllocationRule::from(port_alloc), port_step),
    )
}
