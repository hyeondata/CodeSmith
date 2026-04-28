use anyhow::{Context, Result};
use chrono::Utc;
use codesmith_core::{SourceRecord, SourceStatus, WikiPage, WikiStatus};
use std::cmp::Ordering;
use std::collections::HashMap;
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
        fs::create_dir_all(root.join("schema"))?;
        fs::create_dir_all(root.join("index"))?;
        fs::create_dir_all(root.join("logs"))?;
        ensure_file(
            &root.join("index.md"),
            "# CodeSmith Wiki Index\n\nNo sources ingested yet.\n",
        )?;
        ensure_file(&root.join("log.md"), "# CodeSmith Operation Log\n\n")?;
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

    pub fn ingest_file(
        &self,
        workspace: impl AsRef<Path>,
        source: impl AsRef<Path>,
    ) -> Result<IngestResult> {
        let workspace = workspace
            .as_ref()
            .canonicalize()
            .with_context(|| format!("canonicalize workspace {}", workspace.as_ref().display()))?;
        let source = source
            .as_ref()
            .canonicalize()
            .with_context(|| format!("canonicalize source {}", source.as_ref().display()))?;
        if !source.starts_with(&workspace) {
            anyhow::bail!(
                "source path is outside trusted workspace: {}",
                source.display()
            );
        }
        let kind = source_kind(&source)?;
        let bytes = fs::read(&source)?;
        let hash = fnv1a_hex(&bytes);

        if let Some(existing) = self.find_source_by_hash(&hash)? {
            self.append_log(
                "ingest_file",
                &source.display().to_string(),
                "skipped",
                None,
            )?;
            return Ok(IngestResult {
                record: existing,
                raw_path: self.root.join("raw"),
                skipped: true,
            });
        }

        let id = Uuid::new_v4();
        let raw_name = format!(
            "{}-{}",
            id,
            source
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("source.txt")
        );
        let raw_path = self.root.join("raw").join(raw_name);
        fs::write(&raw_path, &bytes)?;

        let record = SourceRecord {
            id,
            path: source.clone(),
            hash,
            kind,
            ingested_at: Utc::now(),
            status: SourceStatus::Active,
        };
        self.append_source_record(&record)?;
        self.save_source_summary_page(&record, &bytes)?;
        self.rebuild_index()?;
        self.append_log(
            "ingest_file",
            &source.display().to_string(),
            "succeeded",
            None,
        )?;

        Ok(IngestResult {
            record,
            raw_path,
            skipped: false,
        })
    }

    pub fn query_context(&self, query: &str, budget: usize) -> Result<String> {
        let mut context = String::new();
        let index = fs::read_to_string(self.root.join("index.md")).unwrap_or_default();
        if !index.is_empty() {
            context.push_str(&index);
            context.push_str("\n\n");
        }
        let pages = self.search(query, 5)?;
        append_context_section(
            &mut context,
            "Source facts",
            pages.iter().filter(|page| page.domain == "source"),
        );
        append_context_section(
            &mut context,
            "Prior run evidence",
            pages.iter().filter(|page| {
                matches!(
                    page.domain.as_str(),
                    "command" | "commands" | "debugging" | "plan" | "verification"
                )
            }),
        );
        append_context_section(
            &mut context,
            "Other wiki context",
            pages.iter().filter(|page| {
                !matches!(
                    page.domain.as_str(),
                    "source" | "command" | "commands" | "debugging" | "plan" | "verification"
                )
            }),
        );
        if context.len() > budget {
            context.truncate(budget);
        }
        Ok(context)
    }

    pub fn lint_wiki(&self) -> Result<Vec<WikiLintIssue>> {
        let mut issues = Vec::new();
        let mut titles = Vec::new();
        let mut title_paths = HashMap::<String, Vec<PathBuf>>::new();
        let mut page_bodies = Vec::new();
        for entry in fs::read_dir(self.root.join("wiki"))? {
            let entry = entry?;
            if entry.path().extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            let raw = fs::read_to_string(entry.path())?;
            match parse_page(&raw) {
                Ok(page) => {
                    titles.push(page.title.clone());
                    title_paths
                        .entry(page.title)
                        .or_default()
                        .push(entry.path());
                    page_bodies.push((entry.path(), raw));
                }
                Err(error) => {
                    issues.push(WikiLintIssue {
                        kind: "missing_frontmatter".to_string(),
                        path: entry.path(),
                        message: error.to_string(),
                    });
                    page_bodies.push((entry.path(), raw));
                }
            }
        }

        for (title, paths) in title_paths {
            if paths.len() > 1 {
                for path in paths {
                    issues.push(WikiLintIssue {
                        kind: "duplicate_title".to_string(),
                        path,
                        message: format!("duplicate wiki title: {title}"),
                    });
                }
            }
        }

        for (path, raw) in page_bodies {
            for link in wikilinks(&raw) {
                if !titles.iter().any(|title| title == &link) {
                    issues.push(WikiLintIssue {
                        kind: "broken_wikilink".to_string(),
                        path: path.clone(),
                        message: format!("missing page for [[{link}]]"),
                    });
                }
            }
        }

        Ok(issues)
    }

    fn page_path(&self, id: Uuid) -> PathBuf {
        self.root.join("wiki").join(format!("{id}.md"))
    }

    fn source_records_path(&self) -> PathBuf {
        self.root.join("source_records.tsv")
    }

    fn find_source_by_hash(&self, hash: &str) -> Result<Option<SourceRecord>> {
        Ok(self
            .source_records()?
            .into_iter()
            .find(|record| record.hash == hash))
    }

    fn source_records(&self) -> Result<Vec<SourceRecord>> {
        let path = self.source_records_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(path)?;
        let mut records = Vec::new();
        for line in raw.lines() {
            let parts = line.split('\t').collect::<Vec<_>>();
            if parts.len() != 6 {
                continue;
            }
            records.push(SourceRecord {
                id: Uuid::parse_str(parts[0])?,
                path: PathBuf::from(parts[1]),
                hash: parts[2].to_string(),
                kind: parts[3].to_string(),
                ingested_at: parts[4].parse()?,
                status: match parts[5] {
                    "Skipped" => SourceStatus::Skipped,
                    "Failed" => SourceStatus::Failed,
                    _ => SourceStatus::Active,
                },
            });
        }
        Ok(records)
    }

    fn append_source_record(&self, record: &SourceRecord) -> Result<()> {
        let status = match record.status {
            SourceStatus::Active => "Active",
            SourceStatus::Skipped => "Skipped",
            SourceStatus::Failed => "Failed",
        };
        let line = format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            record.id,
            record.path.display(),
            record.hash,
            record.kind,
            record.ingested_at.to_rfc3339(),
            status
        );
        use std::io::Write as _;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.source_records_path())?;
        file.write_all(line.as_bytes())?;
        Ok(())
    }

    fn save_source_summary_page(&self, record: &SourceRecord, bytes: &[u8]) -> Result<()> {
        let preview = String::from_utf8_lossy(bytes);
        let preview = preview.chars().take(4000).collect::<String>();
        let title = format!(
            "Source: {} ({})",
            record
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown"),
            record.hash.chars().take(8).collect::<String>()
        );
        let body = format!(
            "Source path: `{}`\n\nHash: `{}`\n\n```text\n{}\n```",
            record.path.display(),
            record.hash,
            preview
        );
        self.save_page(&title, "source", &body)?;
        Ok(())
    }

    fn rebuild_index(&self) -> Result<()> {
        let mut output = String::from("# CodeSmith Wiki Index\n\n");
        for page in self.list_pages()? {
            output.push_str(&format!("- [[{}]] ({})\n", page.title, page.domain));
        }
        fs::write(self.root.join("index.md"), output)?;
        Ok(())
    }

    pub fn append_log(
        &self,
        operation: &str,
        input: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        use std::io::Write as _;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("log.md"))?;
        writeln!(
            file,
            "- time: {}\n  operation: {}\n  input: {}\n  status: {}\n  error: {}",
            Utc::now().to_rfc3339(),
            operation,
            input,
            status,
            error.unwrap_or("")
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct IngestResult {
    pub record: SourceRecord,
    pub raw_path: PathBuf,
    pub skipped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLintIssue {
    pub kind: String,
    pub path: PathBuf,
    pub message: String,
}

fn render_page(page: &WikiPage) -> String {
    format!(
        "---\nid: {}\ntitle: {}\ntype: {}\ndomain: {}\nsource_count: {}\nconfidence: {}\nstatus: {:?}\n---\n{}",
        page.id,
        page.title,
        page.domain,
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
            "domain" | "type" => domain = Some(value.to_string()),
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

fn append_context_section<'a>(
    context: &mut String,
    heading: &str,
    pages: impl Iterator<Item = &'a WikiPage>,
) {
    let pages = pages.collect::<Vec<_>>();
    if pages.is_empty() {
        return;
    }
    context.push_str(&format!("## {heading}\n"));
    for page in pages {
        context.push_str(&format!("### {}\n{}\n\n", page.title, page.body));
    }
}

fn score_page(page: &WikiPage, query: &str) -> f32 {
    let title = page.title.to_lowercase();
    let haystack = format!("{} {} {}", page.title, page.domain, page.body).to_lowercase();
    query
        .split_whitespace()
        .map(|term| {
            let term = term.to_lowercase();
            let title_bonus = if title.contains(&term) { 10.0 } else { 0.0 };
            title_bonus + haystack.matches(&term).count() as f32
        })
        .sum()
}

fn ensure_file(path: &Path, contents: &str) -> Result<()> {
    if !path.exists() {
        fs::write(path, contents)?;
    }
    Ok(())
}

fn source_kind(path: &Path) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("txt")
        .to_ascii_lowercase();
    let supported = ["txt", "md", "markdown", "toml", "json", "rs", "py"];
    if !supported.contains(&ext.as_str()) {
        anyhow::bail!("unsupported source file type: {}", path.display());
    }
    Ok(ext)
}

fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn wikilinks(raw: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = raw;
    while let Some(start) = rest.find("[[") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find("]]") else {
            break;
        };
        links.push(rest[..end].trim().to_string());
        rest = &rest[end + 2..];
    }
    links
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

    #[test]
    fn ingest_file_rejects_paths_outside_workspace() {
        let root = tempfile::tempdir().expect("root");
        let workspace = tempfile::tempdir().expect("workspace");
        let outside = tempfile::tempdir().expect("outside");
        let source = outside.path().join("secret.md");
        fs::write(&source, "outside").expect("write source");
        let wiki = WikiStore::open(root.path()).expect("open wiki");

        let error = wiki
            .ingest_file(workspace.path(), &source)
            .expect_err("outside path should be rejected");

        assert!(error.to_string().contains("outside trusted workspace"));
    }

    #[test]
    fn ingest_file_copies_raw_writes_index_and_log_then_skips_same_hash() {
        let root = tempfile::tempdir().expect("root");
        let workspace = tempfile::tempdir().expect("workspace");
        let source = workspace.path().join("notes.md");
        fs::write(&source, "# Notes\nhello wiki").expect("write source");
        let wiki = WikiStore::open(root.path()).expect("open wiki");

        let first = wiki
            .ingest_file(workspace.path(), &source)
            .expect("first ingest");
        let second = wiki
            .ingest_file(workspace.path(), &source)
            .expect("second ingest");

        assert!(!first.skipped);
        assert!(first.raw_path.exists());
        assert!(second.skipped);
        assert_eq!(first.record.hash, second.record.hash);
        assert!(root.path().join("index.md").exists());
        assert!(root.path().join("log.md").exists());
        assert!(
            fs::read_to_string(root.path().join("log.md"))
                .expect("log")
                .contains("ingest_file")
        );
    }

    #[test]
    fn lint_wiki_reports_missing_frontmatter_and_broken_links() {
        let root = tempfile::tempdir().expect("root");
        let wiki_dir = root.path().join("wiki");
        fs::create_dir_all(&wiki_dir).expect("wiki dir");
        fs::write(
            wiki_dir.join("bad.md"),
            "No frontmatter with [[Missing Page]]",
        )
        .expect("write bad page");
        let wiki = WikiStore::open(root.path()).expect("open wiki");

        let issues = wiki.lint_wiki().expect("lint");

        assert!(
            issues
                .iter()
                .any(|issue| issue.kind == "missing_frontmatter")
        );
        assert!(issues.iter().any(|issue| issue.kind == "broken_wikilink"));
    }

    #[test]
    fn lint_wiki_reports_duplicate_titles() {
        let root = tempfile::tempdir().expect("root");
        let wiki = WikiStore::open(root.path()).expect("open wiki");
        wiki.save_page("Duplicate", "source", "first")
            .expect("save first");
        wiki.save_page("Duplicate", "source", "second")
            .expect("save second");

        let issues = wiki.lint_wiki().expect("lint");

        assert!(issues.iter().any(|issue| issue.kind == "duplicate_title"));
    }

    #[test]
    fn query_context_includes_index_and_matching_pages_with_budget() {
        let root = tempfile::tempdir().expect("root");
        let wiki = WikiStore::open(root.path()).expect("open wiki");
        wiki.save_page("Rust Notes", "source", "cargo test validates Rust code.")
            .expect("save page");
        fs::write(root.path().join("index.md"), "# Index\n- Rust Notes").expect("index");

        let context = wiki.query_context("rust code", 400).expect("query context");

        assert!(context.contains("# Index"));
        assert!(context.contains("Rust Notes"));
        assert!(context.len() <= 400);
    }

    #[test]
    fn parses_type_frontmatter_as_wiki_page_domain() {
        let raw = "---\nid: 00000000-0000-0000-0000-000000000001\ntitle: Debug Note\ntype: debugging\nsource_count: 1\nconfidence: 1\nstatus: Active\n---\nroot cause evidence";

        let page = parse_page(raw).expect("parse page with type frontmatter");

        assert_eq!(page.domain, "debugging");
        assert!(page.body.contains("root cause evidence"));
    }

    #[test]
    fn query_context_separates_source_facts_and_run_evidence() {
        let root = tempfile::tempdir().expect("root");
        let wiki = WikiStore::open(root.path()).expect("open wiki");
        wiki.save_page("Source: README.md (abc12345)", "source", "project facts")
            .expect("save source");
        wiki.save_page(
            "Command run: Failed python",
            "debugging",
            "SyntaxError evidence",
        )
        .expect("save debug");

        let context = wiki
            .query_context("python README SyntaxError", 1000)
            .expect("query context");

        assert!(context.contains("## Source facts"));
        assert!(context.contains("project facts"));
        assert!(context.contains("## Prior run evidence"));
        assert!(context.contains("SyntaxError evidence"));
    }
}
