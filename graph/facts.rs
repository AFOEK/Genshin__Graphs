use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
};

#[derive(Debug, Deserialize)]
struct EntityRecord {
    id: String,
    pageid: u64,
    revid: u64,
    title: String,
    kind: String,
    source_template: String,
    attributes: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct Fact {
    id: String,
    source_id: String,
    source_pageid: u64,
    source_revid: u64,
    source_title: String,
    source_kind: String,
    source_template: String,
    source_field: String,
    relation: String,
    target_title: String,
    target_kind: String,
    target_source: String,
    target_policy: String,
    raw_value: String,
}

#[derive(Clone, Copy, PartialEq)]
enum ValueMode {
    LinkedOrPlain,
    LinksOnly,
}

#[derive(Clone, Copy)]
struct FieldRule {
    relation: &'static str,
    target_kind: &'static str,
    mode: ValueMode,
}

const fn rule(relation: &'static str, target_kind: &'static str) -> FieldRule {
    FieldRule {
        relation,
        target_kind,
        mode: ValueMode::LinkedOrPlain,
    }
}

const fn linked_rule(relation: &'static str, target_kind: &'static str) -> FieldRule {
    FieldRule {
        relation,
        target_kind,
        mode: ValueMode::LinksOnly,
    }
}

// Matches a base field and its numbered variants.
// Example: affiliation, affiliation2 ... affiliation6.
fn numbered(field: &str, base: &str, max: usize) -> bool {
    if field == base {
        return true;
    }

    field
        .strip_prefix(base)
        .and_then(|suffix| suffix.parse::<usize>().ok())
        .is_some_and(|n| (1..=max).contains(&n))
}

// Template-specific relationship registry.
fn field_rule(kind: &str, field: &str) -> Option<FieldRule> {
    match kind {
        "Character" => {
            if numbered(field, "element", 5) {
                Some(rule("HAS_ELEMENT", "Element"))
            } else if field == "weapon" {
                Some(rule("USES_WEAPON_TYPE", "WeaponType"))
            } else if numbered(field, "affiliation", 6) {
                Some(rule("AFFILIATED_WITH", "Entity"))
            } else if numbered(field, "region", 3) {
                Some(rule("ASSOCIATED_WITH", "Region"))
            } else if numbered(field, "location", 8) {
                Some(rule("LOCATED_IN", "Entity"))
            } else {
                match field {
                    "constellation" => Some(rule("HAS_CONSTELLATION", "Constellation")),
                    "dish" => Some(rule("HAS_SPECIAL_DISH", "Food")),
                    _ => None,
                }
            }
        }

        "Weapon" => match field {
            "type" => Some(rule("HAS_WEAPON_TYPE", "WeaponType")),
            "series" | "series2" => Some(rule("PART_OF_SERIES", "WeaponSeries")),
            "quest" => Some(rule("OBTAINED_FROM_QUEST", "Quest")),
            "exclusiveto" => Some(rule("EXCLUSIVE_TO", "Character")),
            _ => None,
        },

        "Artifact" => match field {
            "set" => Some(rule("PART_OF_SET", "ArtifactSet")),
            _ => None,
        },

        "ArtifactSet" => match field {
            "flower" | "plume" | "sands" | "goblet" | "circlet" => {
                Some(rule("CONTAINS_PIECE", "Artifact"))
            }
            _ => None,
        },

        "Food" => match field {
            "base" => Some(rule("BASED_ON_FOOD", "Food")),
            "character" => Some(rule("SPECIAL_DISH_OF", "Character")),
            "quest" => Some(rule("UNLOCKED_BY", "Quest")),
            "region" => Some(rule("ASSOCIATED_WITH", "Region")),
            _ => None,
        },

        "Quest" => {
            if field == "characters" || field == "character" {
                Some(rule("FEATURES_CHARACTER", "Character"))
            } else if field == "npc" {
                Some(rule("INVOLVES_NPC", "Character"))
            } else if field == "requirement" {
                Some(linked_rule("REQUIRES_QUEST", "Quest"))
            } else if numbered(field, "startlocation", 5) {
                Some(rule("STARTS_IN", "Entity"))
            } else if field == "region" {
                Some(rule("TAKES_PLACE_IN", "Region"))
            } else if matches!(field, "area" | "subarea" | "subregion" | "subreagion") {
                Some(rule("TAKES_PLACE_IN", "Entity"))
            } else if numbered(field, "domain", 4) {
                Some(rule("INVOLVES_DOMAIN", "Domain"))
            } else if numbered(field, "prev", 5) {
                Some(rule("PRECEDED_BY", "Quest"))
            } else if numbered(field, "next", 5) {
                Some(rule("FOLLOWED_BY", "Quest"))
            } else if field == "parent" {
                Some(rule("SUBQUEST_OF", "Quest"))
            } else if numbered(field, "child", 5) {
                Some(rule("HAS_SUBQUEST", "Quest"))
            } else if numbered(field, "concurrent", 6) {
                Some(rule("CONCURRENT_WITH", "Quest"))
            } else if field == "rewards" || field == "otherrewards" {
                Some(linked_rule("REWARDS", "Entity"))
            } else if numbered(field, "poi", 2) {
                Some(rule("HAS_POINT_OF_INTEREST", "Entity"))
            } else {
                None
            }
        }

        _ => None,
    }
}

// Remove HTML comments, including placeholder comments.
fn strip_comments(input: &str) -> String {
    let mut output = String::new();
    let mut rest = input;

    while let Some(start) = rest.find("<!--") {
        output.push_str(&rest[..start]);

        let after = &rest[start + 4..];

        match after.find("-->") {
            Some(end) => rest = &after[end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }

    output.push_str(rest);
    output
}

// Extract actual [[Wiki links]].
// For [[Page|Display Text]], retain Page.
// For [[Page#Section]], retain Page.
fn extract_wikilinks(raw: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut rest = raw;

    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];

        let Some(end) = after.find("]]") else {
            break;
        };

        let inner = &after[..end];

        let target = inner
            .split('|')
            .next()
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("")
            .trim();

        if !target.is_empty() {
            targets.push(target.replace('_', " "));
        }

        rest = &after[end + 2..];
    }

    targets
}

// Avoid creating entities from placeholders or unexpanded templates.
fn is_placeholder(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "none" | "null" | "n/a" | "na" | "-" | "—" | "?"
    )
}

// Extract candidates from an attribute value.
fn extract_targets(raw: &str, mode: ValueMode) -> Vec<(String, &'static str)> {
    let links = extract_wikilinks(raw);

    if !links.is_empty() {
        let mut seen = HashSet::new();

        return links
            .into_iter()
            .filter(|value| seen.insert(value.to_ascii_lowercase()))
            .map(|value| (value, "wikilink"))
            .collect();
    }

    if mode == ValueMode::LinksOnly {
        return Vec::new();
    }

    let cleaned = strip_comments(raw)
        .replace("<br />", ";")
        .replace("<br/>", ";")
        .replace("<br>", ";")
        .replace("'''", "")
        .replace("''", "")
        .replace("&nbsp;", " ");

    // Don't interpret unexpanded templates as entity names.
    if cleaned.contains("{{") || cleaned.contains("}}") {
        return Vec::new();
    }

    let mut seen = HashSet::new();

    cleaned
        .split([';', '\n', '\r'])
        .filter_map(|part| {
            let value =
                part.trim_matches(|c: char| c.is_whitespace() || matches!(c, '*' | '#' | ';'));

            if is_placeholder(value) {
                return None;
            }

            let normalized = value.replace('_', " ");
            if !seen.insert(normalized.to_ascii_lowercase()) {
                return None;
            }

            Some((normalized, "plain_value"))
        })
        .collect()
}

fn write_jsonl<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub fn extract_facts() -> Result<()> {
    fs::create_dir_all("data/graph")?;

    let input = BufReader::new(File::open("data/entities.jsonl")?);

    let mut output = BufWriter::new(File::create("data/graph/facts.jsonl")?);

    let mut fact_count = 0usize;
    let mut entity_count = 0usize;
    let mut relation_counts = BTreeMap::<String, usize>::new();

    // Prevent duplicate facts from the same field and entity.
    let mut seen = HashSet::<(String, String, String, String)>::new();

    for (line_no, line) in input.lines().enumerate() {
        let line = line?;

        let entity: EntityRecord = serde_json::from_str(&line)
            .with_context(|| format!("Invalid entity at line {}", line_no + 1))?;

        let mut entity_fact_count = 0usize;

        for (field, raw_value) in &entity.attributes {
            let Some(mapping) = field_rule(&entity.kind, field) else {
                continue;
            };

            let targets = extract_targets(raw_value, mapping.mode);

            for (index, (target, target_source)) in targets.into_iter().enumerate() {
                let key = (
                    entity.id.clone(),
                    field.clone(),
                    mapping.relation.to_string(),
                    target.to_ascii_lowercase(),
                );

                if !seen.insert(key) {
                    continue;
                }

                let fact = Fact {
                    id: format!(
                        "genshin:fact:{}:{}:{}:{}",
                        entity.pageid,
                        entity.revid,
                        field,
                        index + 1
                    ),
                    source_id: entity.id.clone(),
                    source_pageid: entity.pageid,
                    source_revid: entity.revid,
                    source_title: entity.title.clone(),
                    source_kind: entity.kind.clone(),
                    source_template: entity.source_template.clone(),
                    source_field: field.clone(),
                    relation: mapping.relation.to_string(),
                    target_title: target,
                    target_kind: mapping.target_kind.to_string(),
                    target_policy: if target_source == "wikilink" {
                        "require_page"
                    } else {
                        "page_or_literal"
                    }
                    .to_string(),
                    target_source: target_source.to_string(),
                    raw_value: raw_value.clone(),
                };

                write_jsonl(&mut output, &fact)?;

                *relation_counts
                    .entry(mapping.relation.to_string())
                    .or_default() += 1;

                fact_count += 1;
                entity_fact_count += 1;
            }
        }

        if entity_fact_count > 0 {
            entity_count += 1;
        }
    }

    output.flush()?;

    println!("\n=== Semantic Fact Extraction ===");
    println!("Entities with facts: {entity_count}");
    println!("Total facts: {fact_count}");

    println!("\n=== Relationship Distribution ===");

    for (relation, count) in relation_counts {
        println!("{count:>8}  {relation}");
    }

    Ok(())
}
