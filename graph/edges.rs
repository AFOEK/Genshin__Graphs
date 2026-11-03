use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct LinkEdge {
    pub source_pageid: u64,
    pub source_revid: u64,
    pub target_title: String,
    pub kind: String,
}
