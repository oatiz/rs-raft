use protobuf::Message as Message_imported_for_functions;
use protobuf::ProtobufEnum as ProtobufEnum_imported_for_functions;

#[derive(PartialEq, Clone, Default)]
pub struct Entry {
    pub entry_type: Nil,
    pub term: u64,
    pub index: u64,
    pub data: ::std::vec::Vec<u8>,
    pub context: ::std::vec::Vec<u8>,
    pub sync_log: bool,
}

#[derive(PartialEq, Clone, Default)]
pub struct HardState {
    pub term: u64,
    pub vote: u64,
    pub commit: u64,
}

impl HardState {
    pub fn new() -> HardState {
        ::std::default::Default::default()
    }

    pub fn clean_term(&mut self) {
        self.term = 0;
    }

    pub fn set_term(&mut self, new_term: u64) {
        self.term = new_term;
    }

    pub fn get_term(&self) -> u64 {
        self.term
    }

    pub fn clean_vote(&mut self) {
        self.vote = 0;
    }

    pub fn set_vote(&mut self, new_vote: u64) {
        self.vote = new_vote;
    }

    pub fn get_vote(&self) -> u64 {
        self.vote
    }

    pub fn clean_commit(&mut self) {
        self.commit = 0;
    }

    pub fn set_commit(&mut self, new_commit: u64) {
        self.commit = new_commit;
    }

    pub fn get_commit(&self) -> u64 {
        self.commit
    }
}

#[derive(PartialEq, Clone, Default)]
pub struct ConfState {
    pub nodes: ::std::vec::Vec<u64>,
    pub learners: ::std::vec::Vec<u64>,
}

impl ConfState {
    pub fn new() -> ConfState {
        ::std::default::Default::default()
    }

    pub fn clear_nodes(&mut self) {
        self.nodes.clear();
    }

    pub fn set_nodes(&mut self, new_nodes: ::std::vec::Vec<u64>) {
        self.nodes = new_nodes;
    }

    pub fn mut_nodes(&mut self) -> &mut ::std::vec::Vec<u64> {
        &mut self.nodes
    }

    pub fn take_nodes(&mut self) -> ::std::vec::Vec<u64> {
        ::std::mem::replace(&mut self.nodes, ::std::vec::Vec::new())
    }

    pub fn get_nodes(&self) -> &[u64] {
        &self.nodes
    }

    pub fn clear_learners(&mut self) {
        self.learners.clear();
    }

    pub fn set_learners(&mut self, new_learner: ::std::vec::Vec<u64>) {
        self.learners = new_learner
    }

    pub fn mut_learners(&mut self) -> &mut ::std::vec::Vec<u64> {
        &mut self.learners
    }

    pub fn take_learners(&mut self) -> ::std::vec::Vec<u64> {
        ::std::mem::replace(&mut self.learners, ::std::vec::Vec::new())
    }

    pub fn get_learners(&self) -> &[u64] {
        &self.learners
    }
}

//#[derive(PartialEq, Clone, Default)]
//pub struct ConfState {
//    pub id: u64,
//    pub change_type: ConfChangeType,
//    pub node_id: u64,
//    pub context: ::std::vec::Vec<u8>,
//}

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub enum ConfChangeType {
    AddNode = 0,
    RemoveNode = 1,
    AddLearnerNode = 2,
}
