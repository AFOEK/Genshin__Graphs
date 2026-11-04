mod crawler {
    pub mod api;
    pub mod revisions;
    pub mod xml;
}
mod extractor {
    pub mod aliases;
    pub mod entities;
    pub mod relationships;
}
mod graph {
    pub mod edges;
    pub mod neo4j;
    pub mod nodes;
    pub mod resolvers;
}
mod export {
    pub mod jsonl;
}

use anyhow::{Context, Result};
use parse_wiki_text_2::Configuration;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
    time::Duration,
};

#[derive(Default, Serialize, Deserialize)]
struct Checkpoint {
    continuation: Option<Map<String, Value>>,
    completed: bool,
}

fn load_seen(path: &Path) -> Result<HashSet<(u64, u64)>> {
    let mut seen = HashSet::new();

    if !path.exists() {
        return Ok(seen);
    }

    let file = BufReader::new(File::open(path)?);

    for (line_no, line) in file.lines().enumerate() {
        let line = line?;
        let page: export::jsonl::WikiPage = serde_json::from_str(&line)
            .with_context(|| format!("Invalid JSONL at line {}", line_no + 1))?;

        seen.insert((page.pageid, page.revid));
    }

    Ok(seen)
}

fn save_checkpoint(path: &Path, state: &Checkpoint) -> Result<()> {
    let temp = path.with_extension("tmp");

    fs::write(&temp, serde_json::to_vec_pretty(state)?)?;
    fs::rename(temp, path)?;

    Ok(())
}

fn extract_corpus(limit: Option<usize>) -> anyhow::Result<()> {
    let input = BufReader::new(File::open("data/pages.jsonl")?);

    let mut entities = BufWriter::new(File::create("data/entities.jsonl")?);
    let mut aliases = BufWriter::new(File::create("data/aliases.jsonl")?);
    let mut edges = BufWriter::new(File::create("data/relationships.jsonl")?);
    let mut unknown = BufWriter::new(File::create("data/unclassified_infoboxes.jsonl")?);

    let parser = Configuration::default();
    let mut entity_count = 0usize;
    let mut alias_count = 0usize;
    let mut edge_count = 0usize;
    let mut parse_errors = 0usize;
    let mut infobox_counts = std::collections::BTreeMap::<String, usize>::new();

    for (index, line) in input.lines().take(limit.unwrap_or(usize::MAX)).enumerate() {
        let line = line?;
        let page: export::jsonl::WikiPage = serde_json::from_str(&line)?;

        if page.ns != 0 {
            continue;
        }

        let parsed = match parser.parse(&page.content) {
            Ok(result) => result,
            Err(err) => {
                eprintln!("Parse error on {}: {err:?}", page.title);
                parse_errors += 1;
                continue;
            }
        };

        for name in extractor::entities::infobox_names(&page, &parsed.nodes) {
            *infobox_counts.entry(name).or_default() += 1;
        }

        let boxes = extractor::entities::infobox_names(&page, &parsed.nodes);
        let entity = extractor::entities::extract(&page, &parsed.nodes);

        if entity.is_none() && !boxes.is_empty() {
            export::jsonl::write_item(
                &mut unknown,
                &serde_json::json!({
                    "pageid": page.pageid,
                    "title": page.title,
                    "infoboxes": boxes
                }),
            )?;
        }

        if let Some(entity) = entity {
            export::jsonl::write_item(&mut entities, &entity)?;
            entity_count += 1;
        }

        if let Some(alias) = extractor::aliases::extract(&page, &parsed.nodes) {
            export::jsonl::write_item(&mut aliases, &alias)?;
            alias_count += 1;
        }

        for edge in extractor::relationships::extract(&page, &parsed.nodes) {
            export::jsonl::write_item(&mut edges, &edge)?;
            edge_count += 1;
        }

        if (index + 1) % 1000 == 0 {
            println!("Processed {} pages", index + 1);
        }
    }

    entities.flush()?;
    aliases.flush()?;
    edges.flush()?;

    println!("Entities: {entity_count}");
    println!("Aliases: {alias_count}");
    println!("Links: {edge_count}");
    println!("Parse errors: {parse_errors}");

    let mut sorted: Vec<_> = infobox_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    println!("\n=== Infobox Template Distribution ===");
    for (name, count) in sorted {
        println!("{count:>6}  {name}");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("extract") {
        let limit = std::env::args()
            .nth(2)
            .map(|v| v.parse::<usize>())
            .transpose()?;

        return extract_corpus(limit);
    }

    if std::env::args().nth(1).as_deref() == Some("resolve") {
        return graph::resolvers::resolve_corpus();
    }

    let max_batches = std::env::args()
        .nth(1)
        .map(|s| s.parse::<usize>())
        .transpose()?;

    fs::create_dir_all("data")?;

    let output = Path::new("data/pages.jsonl");
    let checkpoint_path = Path::new("data/checkpoint.json");

    if checkpoint_path.exists() && !output.exists() {
        anyhow::bail!("Checkpoint exists but pages.jsonl is missing");
    }

    let mut checkpoint: Checkpoint = if checkpoint_path.exists() {
        serde_json::from_slice(&fs::read(checkpoint_path)?)?
    } else {
        Checkpoint::default()
    };

    if checkpoint.completed {
        println!("Crawl already completed.");
        return Ok(());
    }

    let mut seen = load_seen(output)?;
    let file = OpenOptions::new().create(true).append(true).open(output)?;
    let mut writer = BufWriter::new(file);

    let client = reqwest::Client::builder()
        .user_agent("GenshinGraphs/0.1 (research; contact: YOUR_PROJECT_URL)")
        .timeout(Duration::from_secs(90))
        .build()?;

    let mut batches = 0usize;
    let mut added = 0usize;
    const BATCH_SIZE: usize = 50;

    loop {
        if max_batches.is_some_and(|n| batches >= n) {
            break;
        }

        let response =
            crawler::api::fetch_batch(&client, checkpoint.continuation.as_ref(), BATCH_SIZE)
                .await?;

        let pages = response
            .pointer("/query/pages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        // Validate the entire batch before writing.
        let records = pages
            .iter()
            .map(export::jsonl::WikiPage::from_api)
            .collect::<Result<Vec<_>>>()?;

        let mut saved = 0usize;

        for page in records {
            if seen.insert((page.pageid, page.revid)) {
                export::jsonl::write_record(&mut writer, &page)?;
                saved += 1;
            }
        }

        // Flush before advancing the checkpoint.
        writer.flush()?;

        checkpoint.continuation = response.get("continue").and_then(Value::as_object).cloned();

        checkpoint.completed = checkpoint.continuation.is_none();
        save_checkpoint(checkpoint_path, &checkpoint)?;

        batches += 1;
        added += saved;

        println!(
            "Batch {batches}: +{saved} pages | Total new: {added} | Done: {}",
            checkpoint.completed
        );

        if checkpoint.completed {
            break;
        }

        // Keep request traffic polite.
        tokio::time::sleep(Duration::from_millis(750)).await;
    }

    println!("Finished this run. Added {added} pages.");
    Ok(())
}
