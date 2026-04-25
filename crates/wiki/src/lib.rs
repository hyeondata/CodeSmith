use anyhow::{Context, Result};
use codesmith_core::{WikiPage, WikiStatus};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct WikiStore {
    root: PathBuf,
}

impl WikiStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("wiki"))?;
        fs::create_dir_all(root.join("raw"))?;
        fs::create_dir_all(root.join("index"))?;
        fs::create_dir_all(root.join("logs"))?;
        Ok(Self { root })
    }

    pub fn save_page(&self, title: &str, domain: &str, body: &str) -> Result<WikiPage> {
        let page = WikiPage {
            id: Uuid::new_v4(),
            title: title.to_string(),
            domain: domain.to_string(),
            source_count: 1,
            confidence: 1.0,
            status: WikiStatus::Active,
            body: body.to_string(),
        };
        let path = self.page_path(page.id);
        fs::write(path, render_page(&page))?;
        Ok(page)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<WikiPage>> {
        let mut scored = Vec::new();
        for entry in fs::read_dir(self.root.join("wiki"))? {
            let entry = entry?;
            if entry.path().extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            let raw = fs::read_to_string(entry.path())?;
            let page = parse_page(&raw).context("parse wiki page")?;
            let score = score_page(&page, query);
            if score > 0.0 {
                scored.push((score, page));
            }
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
        Ok(scored
            .into_iter()
            .take(limit)
            .map(|(_, page)| page)
            .collect())
    }

    pub fn list_pages(&self) -> Result<Vec<WikiPage>> {
        let mut pages = Vec::new();
        for entry in fs::read_dir(self.root.join("wiki"))? {
            let entry = entry?;
            if entry.path().extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            let raw = fs::read_to_string(entry.path())?;
            pages.push(parse_page(&raw).context("parse wiki page")?);
        }
        pages.sort_by(|a, b| a.title.cmp(&b.title));
        Ok(pages)
    }

    fn page_path(&self, id: Uuid) -> PathBuf {
        self.root.join("wiki").join(format!("{id}.md"))
    }
}

fn render_page(page: &WikiPage) -> String {
    format!(
        "---\nid: {}\ntitle: {}\ndomain: {}\nsource_count: {}\nconfidence: {}\nstatus: {:?}\n---\n{}",
        page.id,
        page.title,
        page.domain,
        page.source_count,
        page.confidence,
        page.status,
        page.body
    )
}

fn parse_page(raw: &str) -> Result<WikiPage> {
    let mut parts = raw.splitn(3, "---");
    let _ = parts.next();
    let frontmatter = parts.next().context("missing frontmatter")?;
    let body = parts.next().unwrap_or_default().trim_start().to_string();
    let mut id = None;
    let mut title = None;
    let mut domain = None;
    let mut source_count = 1;
    let mut confidence = 1.0;
    let mut status = WikiStatus::Active;

    for line in frontmatter.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "id" => id = Some(Uuid::parse_str(value)?),
            "title" => title = Some(value.to_string()),
            "domain" => domain = Some(value.to_string()),
            "source_count" => source_count = value.parse()?,
            "confidence" => confidence = value.parse()?,
            "status" => {
                status = match value {
                    "Conflict" => WikiStatus::Conflict,
                    "Archived" => WikiStatus::Archived,
                    _ => WikiStatus::Active,
                }
            }
            _ => {}
        }
    }

    Ok(WikiPage {
        id: id.context("missing id")?,
        title: title.context("missing title")?,
        domain: domain.context("missing domain")?,
        source_count,
        confidence,
        status,
        body,
    })
}

fn score_page(page: &WikiPage, query: &str) -> f32 {
    let haystack = format!("{} {} {}", page.title, page.domain, page.body).to_lowercase();
    query
        .split_whitespace()
        .map(|term| {
            let term = term.to_lowercase();
            haystack.matches(&term).count() as f32
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_loads_and_searches_pages() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wiki = WikiStore::open(dir.path()).expect("open wiki");
        wiki.save_page("Cargo Test", "commands", "Use cargo test for Rust checks.")
            .expect("save page");

        let pages = wiki.search("rust checks", 5).expect("search");

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].title, "Cargo Test");
        assert!(pages[0].body.contains("cargo test"));
    }

    #[test]
    fn lists_saved_pages_even_without_a_search_query() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wiki = WikiStore::open(dir.path()).expect("open wiki");
        wiki.save_page("Command: echo hello", "commands", "stdout hello")
            .expect("save page");

        let pages = wiki.list_pages().expect("list pages");

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].title, "Command: echo hello");
    }
}
