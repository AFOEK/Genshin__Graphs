use parse_wiki_text_2::{Node, Positioned};
use std::collections::BTreeMap;

use crate::export::jsonl::WikiPage;
use crate::graph::nodes::Entity;

fn simple_text(nodes: &[Node<'_>]) -> Option<String> {
    let mut result = String::new();

    for node in nodes {
        match node {
            Node::Text { value, .. } => result.push_str(value),
            Node::Link { target, .. } => result.push_str(target),
            Node::Template { .. } => return None,
            _ => {}
        }
    }

    Some(result.trim().to_owned())
}

fn raw_value(nodes: &[Node<'_>], source: &str) -> String {
    match (nodes.first(), nodes.last()) {
        (Some(first), Some(last)) => source
            .get(first.start()..last.end())
            .unwrap_or("")
            .trim()
            .to_string(),
        _ => String::new(),
    }
}

fn classify_template(name: &str) -> Option<&'static str> {
    match name {
        "character infobox" => Some("Character"),
        "weapon infobox" => Some("Weapon"),
        "artifact infobox" => Some("Artifact"),
        "artifact set infobox" => Some("ArtifactSet"),
        "book infobox" => Some("Book"),
        "book collection infobox" => Some("BookCollection"),
        "food infobox" => Some("Food"),
        "item infobox" => Some("Item"),
        "furnishing infobox" => Some("Furnishing"),
        "quest infobox" => Some("Quest"),
        "location infobox" => Some("Location"),
        "region infobox" => Some("Region"),
        "enemy infobox" => Some("Enemy"),
        "faction infobox" => Some("Faction"),
        "domain infobox" => Some("Domain"),
        "element infobox" => Some("Element"),
        "achievement infobox" => Some("Achievement"),
        "event infobox" => Some("Event"),
        "talent infobox" => Some("Talent"),
        "constellation infobox" => Some("Constellation"),
        "wildlife infobox" => Some("Wildlife"),
        "achievement set infobox" => Some("AchievementSet"),
        "genius invokation tcg infobox" => Some("TCGCard"),
        "terminology infobox" => Some("Terminology"),
        "hidden exploration objectives infobox" => Some("ExplorationObjective"),
        "act infobox" => Some("QuestAct"),
        "avatar infobox" => Some("Avatar"),
        "outfit infobox" => Some("Outfit"),
        _ => None,
    }
}

pub fn extract(page: &WikiPage, nodes: &[Node<'_>]) -> Option<Entity> {
    for node in nodes {
        let Node::Template {
            name, parameters, ..
        } = node
        else {
            continue;
        };

        let template = raw_value(name, &page.content)
            .replace('_', " ")
            .trim()
            .to_ascii_lowercase();

        let Some(kind) = classify_template(&template) else {
            continue;
        };

        let mut attributes = BTreeMap::new();

        for parameter in parameters {
            let Some(name) = &parameter.name else {
                continue;
            };

            let key = raw_value(name, &page.content).trim().to_ascii_lowercase();

            let value = raw_value(&parameter.value, &page.content);

            if !key.is_empty() && !value.is_empty() {
                attributes.insert(key, value);
            }
        }

        return Some(Entity {
            id: format!("genshin:page:{}", page.pageid),
            pageid: page.pageid,
            revid: page.revid,
            title: page.title.clone(),
            kind: kind.to_string(),
            attributes,
        });
    }

    None
}

pub fn infobox_names(page: &WikiPage, nodes: &[Node<'_>]) -> Vec<String> {
    nodes
        .iter()
        .filter_map(|node| {
            if let Node::Template { name, .. } = node {
                let name = raw_value(name, &page.content)
                    .trim()
                    .replace('_', " ")
                    .to_ascii_lowercase();

                if name.ends_with("infobox") {
                    return Some(name);
                }
            }
            None
        })
        .collect()
}
