use parse_wiki_text_2::Node;
use std::collections::BTreeSet;

use crate::export::jsonl::WikiPage;
use crate::graph::edges::LinkEdge;

fn walk(nodes: &[Node<'_>], targets: &mut BTreeSet<String>) {
    for node in nodes {
        match node {
            Node::Link { target, .. } => {
                let title = target
                    .split('#')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .replace('_', " ");

                if !title.is_empty() {
                    targets.insert(title);
                }
            }

            Node::Template { parameters, .. } => {
                for p in parameters {
                    walk(&p.value, targets);
                }
            }

            Node::Heading { nodes, .. }
            | Node::Tag { nodes, .. }
            | Node::ExternalLink { nodes, .. }
            | Node::Preformatted { nodes, .. } => {
                walk(nodes, targets);
            }

            Node::OrderedList { items, .. } | Node::UnorderedList { items, .. } => {
                for item in items {
                    walk(&item.nodes, targets);
                }
            }

            Node::DefinitionList { items, .. } => {
                for item in items {
                    walk(&item.nodes, targets);
                }
            }

            Node::Table { captions, rows, .. } => {
                for caption in captions {
                    walk(&caption.content, targets);
                }
                for row in rows {
                    for cell in &row.cells {
                        walk(&cell.content, targets);
                    }
                }
            }

            _ => {}
        }
    }
}

pub fn extract(page: &WikiPage, nodes: &[Node<'_>]) -> Vec<LinkEdge> {
    let mut targets = BTreeSet::new();
    walk(nodes, &mut targets);

    targets
        .into_iter()
        .map(|title| LinkEdge {
            source_pageid: page.pageid,
            source_revid: page.revid,
            target_title: title,
            kind: "WIKI_LINK".into(),
        })
        .collect()
}
