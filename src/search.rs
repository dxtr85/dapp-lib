// use crate::manifest;
use crate::manifest::Manifest;
use crate::manifest::Tag;
use crate::prelude::read_tags_and_header;
use crate::prelude::AppError;
use crate::prelude::AppType;
use crate::prelude::DataType;
use crate::ContentID;
use crate::Data;
use crate::SwarmName;
use crate::ToAppData;
use std::collections::HashMap;
use std::collections::HashSet;
// use std::ops::Index;

// use crate::LibRequest;
// pub use crate::LibResponse;
use crate::Requestor;
pub use crate::ToApp;
// pub use crate::ToAppMgr;
use async_std::channel::Receiver;
use async_std::channel::Sender;
use gnome::prelude::sha_hash;
use gnome::prelude::SwarmID;

// TODO: build a search engine.
// It should be run against every Catalog Swarm,
// and not other Swarm types (at least not initially).
//
// Application can send a Search request to add a Query containing some text.
// Application can send ListSearches request to get all active queries.
// Application can send SearchResults to get all Hits for specific Query.
//
// Engine is responsible for managing Queries.
// Engine gets notified when a new Swarm has been added,
// and can decide to retrieve Manifest and/or Content Data from it.
// Engine should also get notified when a Manifest or a MainPage was modified.
//
// Upon receiving requested Data, Engine compares it against existing
// Queries and adds/removes Hits respectively.
//
// Comparing Queries against received Data should be done on multiple depths:
// 1. Comparing Query against Tags from Manifest.
// 2. Comparing Query against Tags assigned to given Content.
// 3. Comparing Query against Description of Content.
// Second and third comparisons require getting MainPage of given Content,
// so it can take some time to retrieve it from storage or network,
// since all MainPages' size can be up to 64MiB for each Swarm.
//
// Comparison should be done in multiple steps:
// - check if entire Query String is contained within given text;
// - count how namy words from Query string are found within given text;
// - later some more sophisticated algorithms can be deployed.

// Hit should contain a set of (SwarmName,CID) pairs, no less, no more.
#[derive(Debug)]
pub struct SwarmLink {
    pub s_name: SwarmName,
    pub app_type: Option<AppType>,
    pub max_cid: ContentID,
    pub sender: Sender<ToAppData>,
    pub root_hash: u64,
    pub s_descr: String,
    pub s_tags: HashMap<u8, Tag>,
}

pub enum EngineState {
    Idling,
    Processing(SwarmID, Sender<ToAppData>, Vec<ContentID>, Vec<SwarmID>),
}
impl EngineState {
    pub fn is_idling(&self) -> bool {
        matches!(&self, Self::Idling)
    }
    pub fn enqueue_swarm(&mut self, add_swarm_id: SwarmID) {
        // if self.is_idling() {
        //     return;
        // }
        let state = std::mem::replace(self, EngineState::Idling);
        if let Self::Processing(s_id, sender, processing, mut to_process) = state {
            if !to_process.contains(&add_swarm_id) {
                to_process.push(add_swarm_id);
            }
            *self = Self::Processing(s_id, sender, processing, to_process);
        }
    }
    /// Returns bool indicating if we should start processing next swarm id
    pub fn processing_done(&mut self, s_done_id: SwarmID, c_done_ids: Vec<ContentID>) -> bool {
        let state = std::mem::replace(self, EngineState::Idling);
        if let Self::Processing(s_id, sender, mut processing, to_process) = state {
            if s_id != s_done_id {
                *self = Self::Processing(s_id, sender, processing, to_process);
                return false;
            }
            // for c_done_id in c_done_ids {
            processing.retain(|j| !c_done_ids.contains(j));
            // if let Some(index) = processing.iter().position(|&r| r == c_done_id) {
            //     processing.remove(index);
            // }
            // }
            let is_empty = processing.is_empty();

            *self = Self::Processing(s_id, sender, processing, to_process);
            return is_empty;
        }
        false
    }
}
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Hit(pub SwarmName, pub ContentID, pub u8);
struct Engine {
    queries: HashMap<u64, (Query, HashSet<Hit>)>,
    swarm_links: HashMap<SwarmID, SwarmLink>,
    tags: HashMap<SwarmID, HashMap<u8, Tag>>,
    state: EngineState,
    //TODO: some structure tracking SwarmName to root hash,
    //      so that we do not query non-changed given swarm multiple times
}
impl Engine {
    pub fn new() -> Self {
        Engine {
            queries: HashMap::new(),
            swarm_links: HashMap::new(),
            tags: HashMap::new(),
            state: EngineState::Idling,
        }
    }
    pub async fn add_query(&mut self, phrase: String) {
        let phrase = phrase.trim().to_string();
        let q_hash = sha_hash(phrase.as_bytes());
        let query = Query::new(phrase);
        self.queries.insert(q_hash, (query, HashSet::new()));
        if self.swarm_links.is_empty() {
            self.state = EngineState::Idling;
            return;
        }
        if self.state.is_idling() {
            let mut s_ids = Vec::with_capacity(self.swarm_links.len());
            for key in self.swarm_links.keys() {
                s_ids.push(*key);
            }
            while let Some(s_id) = s_ids.pop() {
                if let Some(s_link) = self.swarm_links.get(&s_id) {
                    let mut queried_cids = vec![];
                    for c_id in 1..=s_link.max_cid {
                        queried_cids.push(c_id);
                    }
                    self.state =
                        EngineState::Processing(s_id, s_link.sender.clone(), queried_cids, s_ids);
                    // TODO: only query for some chunk of contents, not everything at once
                    eprintln!("Requesting all first pages for: {}", s_id);
                    let _ = s_link
                        .sender
                        .send(ToAppData::ReadAllFirstPages(Requestor::Search, None))
                        .await;

                    break;
                }
            }
        } else {
            // we need to go through all swarms once again
            let cur_state = std::mem::replace(&mut self.state, EngineState::Idling);
            if let EngineState::Processing(s_id, sender, processing, mut queued_swarms) = cur_state
            {
                for key in self.swarm_links.keys() {
                    if !queued_swarms.contains(key) {
                        queued_swarms.push(*key);
                    }
                }
                self.state = EngineState::Processing(s_id, sender, processing, queued_swarms);
            }
        }
    }
    pub fn del_query(&mut self, phrase: String) {
        self.queries.remove(&sha_hash(phrase.as_bytes()));
    }
    pub fn get_query(&self, phrase: String) -> (String, Vec<Hit>) {
        let q_hash = sha_hash(phrase.as_bytes());
        if let Some((_q, hset)) = self.queries.get(&q_hash) {
            (phrase, hset.clone().into_iter().collect())
        } else {
            (phrase, vec![])
        }
    }

    pub fn has_queries(&self) -> bool {
        !self.queries.is_empty()
    }

    pub fn summary(&self) -> Vec<(String, usize)> {
        let mut all_queries = Vec::with_capacity(self.queries.len());
        for (q, hset) in self.queries.values() {
            all_queries.push((q.text.clone(), hset.len()));
        }
        all_queries
    }
    pub async fn parse_content(
        &mut self,
        s_id: SwarmID,
        s_name: SwarmName,
        c_id: ContentID,
        d_type: DataType,
        data_vec: Vec<Data>,
    ) {
        if c_id == 0 {
            let manif = Manifest::from(data_vec);
            let mut phrase = manif.description.clone();
            self.tags.insert(s_id, manif.tags.clone());
            for tag in manif.tags.values() {
                phrase.push_str(" ");
                phrase.push_str(&tag.0);
            }
            let _anything_added = self.iter_queries(&s_name, c_id, &phrase);
            if let Some(s_link) = self.swarm_links.get_mut(&s_id) {
                s_link.s_descr = manif.description;
                s_link.app_type = Some(manif.app_type);
                s_link.s_tags = manif.tags;
                if self.queries.is_empty() {
                    return;
                }
                if self.state.is_idling() {
                    // All swarms contents have been matched against all existing queries,
                    // so we only need to check this swarm
                    //      Processing(SwarmID, Sender<ToAppData>, Vec<ContentID>, Vec<SwarmID>),
                    let mut queried_cids = vec![];
                    for c_id in 1..=s_link.max_cid {
                        queried_cids.push(c_id);
                    }
                    self.state =
                        EngineState::Processing(s_id, s_link.sender.clone(), queried_cids, vec![]);
                    // TODO: only query for some chunk of contents, not everything at once
                    eprintln!("2 Requesting all first pages for: {}", s_id);
                    let _ = s_link
                        .sender
                        .send(ToAppData::ReadAllFirstPages(Requestor::Search, None))
                        .await;
                } else {
                    self.state.enqueue_swarm(s_id);
                }
            }
        } else if !data_vec.is_empty() {
            // if queries is empty we ignore this message and return
            if !self.has_queries() {
                return;
            }
            // in any state we process this data
            let first_data = data_vec[0].clone();
            let (tag_bytes, mut header) = read_tags_and_header(d_type, first_data);
            if let Some(tags) = self.tags.get(&s_id) {
                for t_byte in tag_bytes {
                    if let Some(tag) = tags.get(&t_byte) {
                        header.push_str(" ");
                        header.push_str(&tag.0);
                    }
                }
            }
            self.iter_queries(&s_name, c_id, &header);
            // if state is Processing, we remove this CID from list of processing cids
            let advance_to_next_swarm = self.state.processing_done(s_id, vec![c_id]);
            if advance_to_next_swarm {
                self.advance_to_next_swarm().await;
                // TODO: when we are Processing and done with current SwarmID,
                // take next from list and change state to that one
                // or to Idling in case all swarms have been searched for
                // if let Some(s_id) = to_process.pop(){
                //     if let Some(s_link) = self.s
                // }
            }
        }
    }
    pub async fn parse_first_pages(
        &mut self,
        s_id: SwarmID,
        s_name: SwarmName,
        first_pages: Vec<(ContentID, DataType, Data)>,
    ) {
        let mut processed_cids = Vec::with_capacity(first_pages.len());
        for (c_id, d_type, first_data) in first_pages {
            let (tag_bytes, mut header) = read_tags_and_header(d_type, first_data);
            if let Some(tags) = self.tags.get(&s_id) {
                for t_byte in tag_bytes {
                    if let Some(tag) = tags.get(&t_byte) {
                        header.push_str(" ");
                        header.push_str(&tag.0);
                    }
                }
            }
            self.iter_queries(&s_name, c_id, &header);
            processed_cids.push(c_id);
        }
        let advance_to_next_swarm = self.state.processing_done(s_id, processed_cids);
        eprintln!(
            "After first pages parse, should advance to next swarm?: {}",
            advance_to_next_swarm
        );
        if advance_to_next_swarm {
            self.advance_to_next_swarm().await;
        }
    }

    fn iter_queries(&mut self, s_name: &SwarmName, c_id: ContentID, text: &String) -> bool {
        eprintln!("iter_queries for: {}", text);
        let mut anything_added = false;
        for (_h, (q, hits)) in &mut self.queries {
            // eprintln!("Q: {}", &q.text);
            let score = q.compare(text);
            eprintln!("Q: {}, score: {}", &q.text, score);
            if score > 0 {
                let hit = Hit(s_name.clone(), c_id, score);
                hits.insert(hit);
                anything_added = true;
            }
        }
        anything_added
    }
    async fn advance_to_next_swarm(&mut self) {
        eprintln!("In advance_to_next_swarm");
        let current_state = std::mem::replace(&mut self.state, EngineState::Idling);
        match current_state {
            EngineState::Idling => {
                eprintln!("Not advancing to search next SwarmID when Idling");
                // Avoid infinite loop — do nothing, we only transition from Idling
                // when new swarm is synced or new query arrived
            }
            EngineState::Processing(s_id, sender, empty_vec, mut swarms_to_inquire) => {
                if !empty_vec.is_empty() {
                    eprintln!("We should not advance to search next swarm!");
                    self.state =
                        EngineState::Processing(s_id, sender, empty_vec, swarms_to_inquire);
                    return;
                }
                eprintln!("Searching through next swarm, if any.");
                // When we are Processing and done with current SwarmID,
                // take next from list and change state to that one
                // or to Idling in case all swarms have been searched for
                while let Some(s_id) = swarms_to_inquire.pop() {
                    if let Some(s_link) = self.swarm_links.get(&s_id) {
                        // do smthg
                        let mut queried_cids = vec![];
                        for c_id in 1..=s_link.max_cid {
                            queried_cids.push(c_id);
                        }
                        self.state = EngineState::Processing(
                            s_id,
                            s_link.sender.clone(),
                            queried_cids,
                            swarms_to_inquire,
                        );
                        // TODO: only query for some chunk of contents, not everything at once
                        eprintln!("3 Requesting all first pages for: {}", s_id);
                        let _ = s_link
                            .sender
                            .send(ToAppData::ReadAllFirstPages(Requestor::Search, None))
                            .await;
                        break;
                    } else {
                        eprintln!(" Could not find SwarmLink for {}", s_id);
                    }
                }
            }
        }
    }
}
struct Query {
    text: String,
    words: HashSet<String>,
}
impl Query {
    pub fn new(text: String) -> Self {
        let mut words = HashSet::new();
        for word in text.split_whitespace() {
            words.insert(word.to_string());
        }
        Query { text, words }
    }

    pub fn compare(&self, phrase: &String) -> u8 {
        eprintln!("{} contains: {}  ?", self.text, phrase);
        if phrase.contains(&self.text) {
            return 101;
        }
        eprintln!("Words: {:?}", self.words);
        let mut score = 0;
        let splited = phrase.split_whitespace();
        for word in splited {
            eprintln!("Test word: {}", word);
            if self.words.contains(word) {
                score += 1;
            }
        }
        let res = (100 * score / self.words.len()) as u8;
        eprintln!(
            "score: {}, total: {}, res: {}",
            score,
            self.words.len(),
            res
        );
        res
    }
}
#[derive(Debug)]
pub enum SearchMsg {
    AddQuery(String),
    DelQuery(String),
    ListQueries,
    GetResults(String),
    SwarmSynced(SwarmID, SwarmLink),
    FirstPages(SwarmID, SwarmName, Vec<(ContentID, DataType, Data)>),
    ReadSuccess(SwarmID, SwarmName, ContentID, DataType, Vec<Data>),
    ReadError(SwarmID, ContentID, AppError),
    AppDataTerminated(SwarmID),
}
pub async fn serve_search_engine(
    to_user: Sender<ToApp>,
    // to_app_mgr: Sender<ToAppMgr>,
    //TODO: replace LibResponse with a dedicated struct
    response: Receiver<SearchMsg>,
) {
    let mut engine = Engine::new();
    loop {
        while let Ok(message) = response.recv().await {
            eprintln!("SearchEngine received: {:?}", message);
            match message {
                SearchMsg::AddQuery(phrase) => {
                    eprintln!("Added new Search");
                    engine.add_query(phrase).await;
                    // for link in engine.swarm_links.values() {
                    //     for c_id in 0..=link.max_cid {
                    //         link.sender
                    //             .send(ToAppData::ReadData(Requestor::Search, c_id))
                    //             .await;
                    //     }
                    // }
                }
                SearchMsg::DelQuery(phrase) => {
                    engine.del_query(phrase);
                }
                SearchMsg::ListQueries => {
                    let _ = to_user.send(ToApp::SearchQueries(engine.summary())).await;
                }
                SearchMsg::GetResults(phrase) => {
                    let (phrase, results) = engine.get_query(phrase);
                    let _ = to_user.send(ToApp::SearchResults(phrase, results)).await;
                }
                SearchMsg::SwarmSynced(s_id, s_link) => {
                    // TODO: check if root_hash changed
                    // TODO: create a mechanism to enqueue subsequent swarms
                    //       until we are done with current one,
                    //       or something with that flavor
                    let _ = s_link
                        .sender
                        .send(ToAppData::ReadPagesRange(Requestor::Search, 0, 0, 63))
                        .await;
                    engine.swarm_links.insert(s_id, s_link);
                }
                SearchMsg::FirstPages(s_id, s_name, first_pages) => {
                    eprintln!(
                        "SearchEngine received requested first pages from {}",
                        s_name
                    );
                    if engine.has_queries() {
                        engine.parse_first_pages(s_id, s_name, first_pages).await;
                    }
                }
                SearchMsg::ReadSuccess(s_id, s_name, c_id, d_type, data_vec) => {
                    // if engine.has_queries() {
                    engine
                        .parse_content(s_id, s_name, c_id, d_type, data_vec)
                        .await;
                    // }
                }
                SearchMsg::ReadError(s_id, c_id, _err) => {
                    let advance_to_next_swarm = engine.state.processing_done(s_id, vec![c_id]);
                    if advance_to_next_swarm {
                        engine.advance_to_next_swarm().await;
                    }
                }
                SearchMsg::AppDataTerminated(s_id) => {
                    engine.swarm_links.remove(&s_id);
                    engine.tags.remove(&s_id);
                }
            }
        }
        break;
    }
    eprintln!("SearchEngine is done.");
}
