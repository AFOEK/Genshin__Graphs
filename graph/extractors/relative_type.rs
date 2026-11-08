use parse_wiki_text_2::{Configuration, Node, Positioned};

#[derive(Debug, PartialEq)]
pub struct RelativeEntry {
    pub label: String,
    pub target: String,
}

fn raw_nodes(source: &str, nodes: &[Node<'_>]) -> String {
    match (nodes.first(), nodes.last()) {
        (Some(a), Some(b)) => source
            .get(a.start()..b.end())
            .unwrap_or("")
            .trim()
            .to_string(),
        _ => String::new(),
    }
}

pub fn extract(raw: &str) -> Vec<RelativeEntry> {
    let Ok(parsed) = Configuration::default().parse(raw) else {
        return Vec::new();
    };

    let mut results = Vec::new();

    for node in &parsed.nodes {
        let Node::Template {
            name, parameters, ..
        } = node
        else {
            continue;
        };

        let template = raw_nodes(raw, name);
        if !template.eq_ignore_ascii_case("Relative Type") {
            continue;
        }

        let positional: Vec<_> = parameters
            .iter()
            .filter(|p| p.name.is_none())
            .map(|p| raw_nodes(raw, &p.value))
            .collect();

        if positional.len() < 2 {
            continue;
        }

        results.push(RelativeEntry {
            label: positional[0].clone(),
            target: positional[1].clone(),
        });
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_parent() {
        let raw = "{{Relative Type|Parent|Macani|ref=Quest|Example}}";
        let result = extract(raw);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "Parent");
        assert_eq!(result[0].target, "Macani");
    }

    #[test]
    fn parses_multiple_relatives() {
        let raw = concat!(
            "{{Relative Type|Parent|Macani}}",
            "{{Relative Type|Sibling|Arasaka}}"
        );

        let result = extract(raw);

        assert_eq!(result.len(), 2);
        assert_eq!(result[1].label, "Sibling");
        assert_eq!(result[1].target, "Arasaka");
    }
}
