use anyhow::Result;
use codesmith_core::{
    AppSettings, ChatMessage, CommandRun, IngestJob, SourceRecord, WikiPageMetadata,
};
use rusqlite::{Connection, params};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: Uuid,
    pub title: String,
}

pub fn settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codesmith")
        .join("settings.toml")
}

pub fn load_settings() -> Result<AppSettings> {
    load_settings_from(&settings_path())
}

pub fn save_settings(settings: &AppSettings) -> Result<()> {
    save_settings_to(&settings_path(), settings)
}

pub fn load_settings_from(path: &Path) -> Result<AppSettings> {
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let raw = fs::read_to_string(path)?;
    let mut settings: AppSettings = toml::from_str(&raw)?;
    settings.ensure_model_profiles();
    Ok(settings)
}

pub fn save_settings_to(path: &Path, settings: &AppSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut settings = settings.clone();
    settings.ensure_model_profiles();
    fs::write(path, toml::to_string_pretty(&settings)?)?;
    Ok(())
}

pub struct Storage {
    root: PathBuf,
    conn: Connection,
}

impl Storage {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("sessions"))?;
        let conn = Connection::open(root.join("codesmith.sqlite3"))?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS command_runs (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS source_records (
                id TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS ingest_jobs (
                id TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS wiki_page_metadata (
                id TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            ",
        )?;
        Ok(Self { root, conn })
    }

    pub fn create_session(&self, title: &str) -> Result<Uuid> {
        let id = Uuid::new_v4();
        self.conn.execute(
            "INSERT INTO sessions (id, title, created_at) VALUES (?1, ?2, ?3)",
            params![id.to_string(), title, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(id)
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionSummary>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, title FROM sessions ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            Ok(SessionSummary {
                id: Uuid::parse_str(&id).expect("stored uuid should parse"),
                title: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn append_message(&self, session_id: Uuid, message: &ChatMessage) -> Result<()> {
        let path = self.transcript_path(session_id);
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{}", serde_json::to_string(message)?)?;
        Ok(())
    }

    pub fn load_transcript(&self, session_id: Uuid) -> Result<Vec<ChatMessage>> {
        let path = self.transcript_path(session_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(path)?;
        let mut messages = Vec::new();
        for line in BufReader::new(file).lines() {
            messages.push(serde_json::from_str(&line?)?);
        }
        Ok(messages)
    }

    pub fn insert_command_run(&self, session_id: Uuid, run: &CommandRun) -> Result<()> {
        self.conn.execute(
            "INSERT INTO command_runs (id, session_id, payload, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                run.id.to_string(),
                session_id.to_string(),
                serde_json::to_string(run)?,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn list_command_runs(&self, session_id: Uuid) -> Result<Vec<CommandRun>> {
        let mut stmt = self.conn.prepare(
            "SELECT payload FROM command_runs WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([session_id.to_string()], |row| {
            let payload: String = row.get(0)?;
            Ok(serde_json::from_str(&payload).expect("stored command run should parse"))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn insert_source_record(&self, source: &SourceRecord) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO source_records (id, payload, created_at) VALUES (?1, ?2, ?3)",
            params![
                source.id.to_string(),
                serde_json::to_string(source)?,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn list_source_records(&self) -> Result<Vec<SourceRecord>> {
        self.list_payloads("source_records")
    }

    pub fn insert_ingest_job(&self, job: &IngestJob) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO ingest_jobs (id, payload, created_at) VALUES (?1, ?2, ?3)",
            params![
                job.id.to_string(),
                serde_json::to_string(job)?,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn list_ingest_jobs(&self) -> Result<Vec<IngestJob>> {
        self.list_payloads("ingest_jobs")
    }

    pub fn insert_wiki_page_metadata(&self, page: &WikiPageMetadata) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO wiki_page_metadata (id, payload, created_at) VALUES (?1, ?2, ?3)",
            params![
                page.id.to_string(),
                serde_json::to_string(page)?,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn list_wiki_page_metadata(&self) -> Result<Vec<WikiPageMetadata>> {
        self.list_payloads("wiki_page_metadata")
    }

    fn transcript_path(&self, session_id: Uuid) -> PathBuf {
        self.root
            .join("sessions")
            .join(format!("{session_id}.jsonl"))
    }

    fn list_payloads<T: serde::de::DeserializeOwned>(&self, table: &str) -> Result<Vec<T>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT payload FROM {table} ORDER BY created_at ASC"
        ))?;
        let rows = stmt.query_map([], |row| {
            let payload: String = row.get(0)?;
            Ok(serde_json::from_str(&payload).expect("stored payload should parse"))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codesmith_core::{
        ChatMessage, ChatRole, CommandProposal, CommandRun, CommandStatus, IngestJob, SourceRecord,
        SourceStatus, WikiPageMetadata, WikiStatus,
    };
    use std::path::PathBuf;

    #[test]
    fn settings_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        let mut settings = codesmith_core::AppSettings {
            llm_base_url: "http://localhost:11434/v1".to_string(),
            llm_model: "qwen2.5-coder:7b".to_string(),
            api_key: Some("ignored".to_string()),
            default_workspace: PathBuf::from("/tmp/project"),
            command_timeout_secs: 120,
            ..Default::default()
        };
        settings.model_profiles = vec![codesmith_core::ModelProfile::from_legacy(
            "default",
            settings.llm_base_url.clone(),
            settings.llm_model.clone(),
            settings.api_key.clone(),
        )];

        save_settings_to(&path, &settings).expect("save settings");
        let loaded = load_settings_from(&path).expect("load settings");

        assert_eq!(loaded, settings);
    }

    #[test]
    fn legacy_settings_migrate_to_default_model_profile() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        fs::write(
            &path,
            r#"llm_base_url = "http://localhost:11434/v1"
llm_model = "gemma4:e4b-mlx-bf16"
default_workspace = "/Users/gim-yonghyeon/CodeSmith"
command_timeout_secs = 120
"#,
        )
        .expect("write legacy settings");

        let loaded = load_settings_from(&path).expect("load legacy settings");

        assert_eq!(loaded.active_profile, "default");
        assert_eq!(loaded.model_profiles.len(), 1);
        assert_eq!(loaded.model_profiles[0].model, "gemma4:e4b-mlx-bf16");
        assert_eq!(loaded.llm_model, "gemma4:e4b-mlx-bf16");
    }

    #[test]
    fn stores_session_metadata_and_transcript() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Storage::open(dir.path()).expect("open storage");
        let session_id = store
            .create_session("Test Session")
            .expect("create session");
        store
            .append_message(
                session_id,
                &ChatMessage::new(ChatRole::User, "hello".to_string()),
            )
            .expect("append message");
        let run = CommandRun::new(
            CommandProposal::new("echo hello", PathBuf::from("."), "test"),
            CommandStatus::Succeeded,
            "hello\n".to_string(),
            String::new(),
            Some(0),
        );
        store
            .insert_command_run(session_id, &run)
            .expect("insert run");

        let sessions = store.list_sessions().expect("list sessions");
        let transcript = store.load_transcript(session_id).expect("load transcript");
        let runs = store.list_command_runs(session_id).expect("list runs");

        assert_eq!(sessions.len(), 1);
        assert_eq!(transcript.len(), 1);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].stdout, "hello\n");
    }

    #[test]
    fn stores_cli_first_wiki_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Storage::open(dir.path()).expect("open storage");
        let source = SourceRecord {
            id: uuid::Uuid::new_v4(),
            path: PathBuf::from("/tmp/project/notes.md"),
            hash: "abc123".to_string(),
            kind: "md".to_string(),
            ingested_at: chrono::Utc::now(),
            status: SourceStatus::Active,
        };
        let job = IngestJob {
            id: uuid::Uuid::new_v4(),
            source_id: source.id,
            status: SourceStatus::Active,
            analysis_path: Some(PathBuf::from("raw/analysis.md")),
            error: None,
        };
        let page = WikiPageMetadata {
            id: uuid::Uuid::new_v4(),
            title: "Source: notes.md".to_string(),
            path: PathBuf::from("wiki/source-notes.md"),
            sources: vec![source.id],
            updated_at: chrono::Utc::now(),
            status: WikiStatus::Active,
        };

        store.insert_source_record(&source).expect("insert source");
        store.insert_ingest_job(&job).expect("insert job");
        store
            .insert_wiki_page_metadata(&page)
            .expect("insert page metadata");

        assert_eq!(store.list_source_records().expect("sources"), vec![source]);
        assert_eq!(store.list_ingest_jobs().expect("jobs"), vec![job]);
        assert_eq!(store.list_wiki_page_metadata().expect("pages"), vec![page]);
    }
}
