use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct Entity {
    pub id: String,
    pub pageid: u64,
    pub revid: u64,
    pub title: String,
    pub kind: String,
    pub source_template: String,
    pub attributes: BTreeMap<String, String>,
}
