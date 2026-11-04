use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
};

#[derive(Deserialize)]
struct PageRecord {
    pageid: u64,
    ns: i32,
    title: String,
}

#[derive(Deserialize)]
struct AliasRecord {
    alias: String,
    target_title: String,
}

#[derive(Deserialize)]
struct LinkRecord {
    source_pageid: u64,
    source_revid: u64,
    target_title: String,
    kind: String,
}

#[derive(Serialize)]
struct ResolvedEdge {
    source_pageid: u64,
    target_pageid: u64,
    source_revid: u64,
    source_title: String,
    observed_target_title: String,
    canonical_target_title: String,
    relationship: String,
    evidence_kind: String,
    via_redirect: bool,
}

#[derive(Serialize)]
struct UnresolvedLink {
    source_pageid: u64,
    source_revid: u64,
    target_title: String,
    kind: String,
    reason: String,
}

fn normalize_title(raw: &str) -> String {
    let title = raw
        .split('#')
        .next()
        .unwrap_or(raw)
        .trim()
        .replace('_', " ");

    let mut chars = title.chars();

    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn is_non_main_namespace(title: &str) -> bool {
    const NAMESPACES: &[&str] = &[
        "file",
        "image",
        "category",
        "template",
        "help",
        "special",
        "talk",
        "user",
        "mediawiki",
        "module",
        "media",
        "project",
        "portal",
    ];

    let title = title.trim_start_matches(':');

    title
        .split_once(':')
        .is_some_and(|(prefix, _)| NAMESPACES.iter().any(|ns| prefix.eq_ignore_ascii_case(ns)))
}

fn resolve_title(
    title: &str,
    title_to_id: &HashMap<String, u64>,
    pageid_to_title: &HashMap<u64, String>,
    redirects: &HashMap<String, String>,
) -> std::result::Result<(u64, String, bool), &'static str> {
    let mut current = normalize_title(title);
    let mut visited = HashSet::new();
    let mut followed_redirect = false;

    if current.is_empty() {
        return Err("empty_target_title");
    }

    loop {
        if !visited.insert(current.clone()) {
            return Err("redirect_cycle");
        }

        if let Some(target) = redirects.get(&current) {
            current = normalize_title(target);
            followed_redirect = true;
            continue;
        }

        if let Some(&pageid) = title_to_id.get(&current) {
            let canonical = pageid_to_title
                .get(&pageid)
                .cloned()
                .unwrap_or_else(|| current.clone());

            return Ok((pageid, canonical, followed_redirect));
        }

        return Err("target_not_found");
    }
}

fn write_jsonl<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub fn resolve_corpus() -> Result<()> {
    fs::create_dir_all("data/graph")?;

    // 1. Build the canonical page index.
    let mut title_to_id = HashMap::<String, u64>::new();
    let mut pageid_to_title = HashMap::<u64, String>::new();

    let input = BufReader::new(File::open("data/pages.jsonl")?);

    for (i, line) in input.lines().enumerate() {
        let line = line?;
        let page: PageRecord = serde_json::from_str(&line)
            .with_context(|| format!("Invalid page at line {}", i + 1))?;

        if page.ns != 0 {
            continue;
        }

        let normalized = normalize_title(&page.title);

        title_to_id.insert(normalized, page.pageid);
        pageid_to_title.insert(page.pageid, page.title);
    }

    println!("Indexed pages: {}", title_to_id.len());

    // 2. Build the redirect map.
    let mut redirects = HashMap::<String, String>::new();

    let input = BufReader::new(File::open("data/aliases.jsonl")?);

    for (i, line) in input.lines().enumerate() {
        let line = line?;
        let alias: AliasRecord = serde_json::from_str(&line)
            .with_context(|| format!("Invalid alias at line {}", i + 1))?;

        redirects.insert(
            normalize_title(&alias.alias),
            normalize_title(&alias.target_title),
        );
    }

    println!("Redirects indexed: {}", redirects.len());

    // 3. Prepare output files.
    let mut resolved = BufWriter::new(File::create("data/graph/resolved_edges.jsonl")?);

    let mut unresolved = BufWriter::new(File::create("data/graph/unresolved_links.jsonl")?);

    let input = BufReader::new(File::open("data/relationships.jsonl")?);

    let mut seen_edges = HashSet::<(u64, u64, String)>::new();

    let mut total = 0usize;
    let mut resolved_count = 0usize;
    let mut unresolved_count = 0usize;
    let mut redirected_count = 0usize;
    let mut duplicate_count = 0usize;
    let mut self_links = 0usize;

    // 4. Resolve every wiki link.
    for (i, line) in input.lines().enumerate() {
        let line = line?;
        let link: LinkRecord = serde_json::from_str(&line)
            .with_context(|| format!("Invalid link at line {}", i + 1))?;

        total += 1;

        let source_title = match pageid_to_title.get(&link.source_pageid) {
            Some(title) => title.clone(),
            None => {
                write_jsonl(
                    &mut unresolved,
                    &UnresolvedLink {
                        source_pageid: link.source_pageid,
                        source_revid: link.source_revid,
                        target_title: link.target_title,
                        kind: link.kind,
                        reason: "source_page_missing".into(),
                    },
                )?;

                unresolved_count += 1;
                continue;
            }
        };

        if is_non_main_namespace(&link.target_title) {
            write_jsonl(
                &mut unresolved,
                &UnresolvedLink {
                    source_pageid: link.source_pageid,
                    source_revid: link.source_revid,
                    target_title: link.target_title,
                    kind: link.kind,
                    reason: "non_main_namespace".into(),
                },
            )?;

            unresolved_count += 1;
            continue;
        }

        match resolve_title(
            &link.target_title,
            &title_to_id,
            &pageid_to_title,
            &redirects,
        ) {
            Ok((target_id, canonical_title, via_redirect)) => {
                if link.source_pageid == target_id {
                    self_links += 1;
                    continue;
                }

                let key = (link.source_pageid, target_id, link.kind.clone());

                if !seen_edges.insert(key) {
                    duplicate_count += 1;
                    continue;
                }

                write_jsonl(
                    &mut resolved,
                    &ResolvedEdge {
                        source_pageid: link.source_pageid,
                        target_pageid: target_id,
                        source_revid: link.source_revid,
                        source_title,
                        observed_target_title: link.target_title,
                        canonical_target_title: canonical_title,
                        relationship: "LINKS_TO".into(),
                        evidence_kind: link.kind,
                        via_redirect,
                    },
                )?;

                resolved_count += 1;

                if via_redirect {
                    redirected_count += 1;
                }
            }

            Err(reason) => {
                write_jsonl(
                    &mut unresolved,
                    &UnresolvedLink {
                        source_pageid: link.source_pageid,
                        source_revid: link.source_revid,
                        target_title: link.target_title,
                        kind: link.kind,
                        reason: reason.into(),
                    },
                )?;

                unresolved_count += 1;
            }
        }

        if total % 50_000 == 0 {
            println!("Resolved {} links...", total);
        }
    }

    resolved.flush()?;
    unresolved.flush()?;

    // 5. Report results.
    println!("\n=== Graph Resolution Results ===");
    println!("Raw links:          {total}");
    println!("Resolved edges:     {resolved_count}");
    println!("Unresolved links:   {unresolved_count}");
    println!("Via redirects:      {redirected_count}");
    println!("Duplicate edges:    {duplicate_count}");
    println!("Self-links skipped: {self_links}");

    Ok(())
}
