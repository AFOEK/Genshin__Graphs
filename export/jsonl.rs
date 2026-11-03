use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;

#[derive(Debug, Serialize, Deserialize)]
pub struct WikiPage {
    pub pageid: u64,
    pub ns: i64,
    pub title: String,
    pub revid: u64,
    pub timestamp: String,
    pub content: String,
}

impl WikiPage {
    pub fn from_api(page: &Value) -> Result<Self> {
        let revision = page["revisions"]
            .as_array()
            .and_then(|v| v.first())
            .context("Missing page revision")?;

        Ok(Self {
            pageid: page["pageid"].as_u64().context("Missing page ID")?,
            ns: page["ns"].as_i64().context("Missing namespace")?,
            title: page["title"].as_str().context("Missing title")?.to_owned(),
            revid: revision["revid"].as_u64().context("Missing revision ID")?,
            timestamp: revision["timestamp"]
                .as_str()
                .context("Missing timestamp")?
                .to_owned(),
            content: revision
                .pointer("/slots/main/content")
                .and_then(Value::as_str)
                .context("Missing wikitext")?
                .to_owned(),
        })
    }
}

pub fn write_record(writer: &mut impl Write, page: &WikiPage) -> Result<()> {
    serde_json::to_writer(&mut *writer, page)?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub fn write_item<T: serde::Serialize>(
    writer: &mut impl std::io::Write,
    value: &T,
) -> anyhow::Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}
