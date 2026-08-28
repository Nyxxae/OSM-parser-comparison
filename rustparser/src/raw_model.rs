use std::collections::HashMap;

pub struct RawNode {
    pub id: i64,
    pub lat: f64,
    pub lon: f64,
    pub tags: HashMap<String, String>,
}

pub struct RawWay {
    pub id: i64,
    pub tags: HashMap<String, String>,
    pub node_refs: Vec<i64>,
}

pub struct RawMember {
    pub member_type: String,
    pub role: String,
    pub member_ref: i64,
}

pub struct RawRelation {
    pub id: i64,
    pub tags: HashMap<String, String>,
    pub members: Vec<RawMember>,
}

#[derive(Default)]
pub struct RawModel {
    pub nodes: Vec<RawNode>,
    pub ways: Vec<RawWay>,
    pub relations: Vec<RawRelation>,
}
