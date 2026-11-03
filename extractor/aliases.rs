use parse_wiki_text_2::Node;
use serde::Serialize;

use crate::export::jsonl::WikiPage;

#[derive(Debug, Serialize)]
pub struct Alias {
    pub alias: String,
    pub target_title: String,
    pub source_pageid: u64,
    pub source_revid: u64,
    pub kind: String,
}

pub fn extract(page: &WikiPage, nodes: &[Node<'_>]) -> Option<Alias> {
    nodes.iter().find_map(|node| {
        if let Node::Redirect { target, .. } = node {
            Some(Alias {
                alias: page.title.clone(),
                target_title: target.to_string(),
                source_pageid: page.pageid,
                source_revid: page.revid,
                kind: "WIKI_REDIRECT".into(),
            })
        } else {
            None
        }
    })
}
