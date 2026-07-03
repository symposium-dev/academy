use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use toasty::stmt::{List, Query};

#[derive(Debug, toasty::Model)]
pub struct Session {
    #[key]
    pub id: String,
    pub cwd: String,
    pub created_at: String,
    pub updated_at: String,

    #[has_many]
    pub messages: toasty::Deferred<Vec<Message>>,
}

#[derive(Debug, toasty::Model)]
pub struct Message {
    #[key]
    #[auto]
    pub id: u64,

    #[index]
    pub session_id: String,

    #[belongs_to(key = session_id, references = id)]
    pub session: toasty::Deferred<Session>,

    pub payload: String,
}

#[derive(Debug, toasty::Model)]
pub struct Trace {
    #[key]
    #[auto]
    pub id: u64,

    #[index]
    pub ts: String,

    #[index]
    pub session_id: Option<String>,

    pub dir: String,
    pub role: Option<String>,
    pub kind: String,
    pub method: Option<String>,
    pub request_id: Option<String>,
    pub payload: toasty::Json<serde_json::Value>,
}

/// A session's membership in a jamsession team.
///
/// Keyed by `session_id`, so a session belongs to at most one team — the
/// one-team-per-session invariant is the primary key. A team "exists" exactly
/// when at least one membership row references it; there is no separate teams
/// table.
#[derive(Debug, toasty::Model)]
pub struct TeamMembership {
    #[key]
    pub session_id: String,

    #[index]
    pub team: String,

    pub joined_at: String,
}

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub session_id: String,
    pub cwd: PathBuf,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Store {
    db: toasty::Db,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceDirection {
    ClientToDaemon,
    DaemonToAgent,
    AgentToDaemon,
    DaemonToClient,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceKind {
    Request,
    Response,
    Notification,
    Event,
}

#[derive(Debug, Clone)]
pub struct NewTrace {
    pub session_id: Option<String>,
    pub dir: TraceDirection,
    pub role: Option<String>,
    pub kind: TraceKind,
    pub method: Option<String>,
    pub request_id: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraceRecord {
    pub id: u64,
    pub ts: DateTime<Utc>,
    pub session_id: Option<String>,
    pub dir: TraceDirection,
    pub role: Option<String>,
    pub kind: TraceKind,
    pub method: Option<String>,
    pub request_id: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct TraceQuery {
    pub after_id: Option<u64>,
    pub session_id: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub method: Option<String>,
    pub dir: Option<TraceDirection>,
    pub limit: Option<u32>,
}

impl Store {
    pub async fn open(path: &Path) -> crate::error::Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let db = toasty::Db::builder()
            .models(toasty::models!(Session, Message, Trace, TeamMembership))
            .build(toasty_driver_sqlite::Sqlite::open(path))
            .await?;

        configure_sqlite_file(&db).await?;

        if !table_exists(&db, "sessions").await? {
            db.push_schema().await?;
        } else {
            // Existing database: add any tables introduced after it was
            // created. These are always a suffix of the registered models
            // (tables are only ever appended), so they can be added together.
            let mut missing = Vec::new();
            for table in ["traces", "team_memberships"] {
                if !table_exists(&db, table).await? {
                    missing.push(table);
                }
            }
            if !missing.is_empty() {
                add_missing_tables(&db, &missing).await?;
            }
        }

        Ok(Self { db })
    }

    pub async fn in_memory() -> crate::error::Result<Self> {
        let db = toasty::Db::builder()
            .models(toasty::models!(Session, Message, Trace, TeamMembership))
            .build(toasty_driver_sqlite::Sqlite::in_memory())
            .await?;
        db.push_schema().await?;

        Ok(Self { db })
    }

    pub async fn list_sessions(
        &self,
        cwd: Option<&Path>,
    ) -> crate::error::Result<Vec<SessionRecord>> {
        let mut db = self.db.clone();
        let sessions = match cwd {
            Some(cwd) => {
                let cwd = cwd.to_string_lossy().into_owned();
                Session::filter(Session::fields().cwd().eq(cwd))
                    .exec(&mut db)
                    .await?
            }
            None => Query::<List<Session>>::all().exec(&mut db).await?,
        };

        sessions.into_iter().map(SessionRecord::try_from).collect()
    }

    pub async fn get_session(
        &self,
        session_id: &str,
    ) -> crate::error::Result<Option<SessionRecord>> {
        let mut db = self.db.clone();
        let session = Session::get_by_id(&mut db, session_id).await?;
        SessionRecord::try_from(session).map(Some)
    }

    pub async fn add_session(&self, session_id: &str, cwd: &Path) -> crate::error::Result<()> {
        let now = Utc::now().to_rfc3339();
        let cwd = cwd.to_string_lossy().into_owned();
        let mut db = self.db.clone();

        toasty::create!(Session {
            id: session_id.to_string(),
            cwd,
            created_at: now.clone(),
            updated_at: now,
        })
        .exec(&mut db)
        .await?;

        Ok(())
    }

    pub async fn append_message(
        &self,
        session_id: &str,
        payload: &serde_json::Value,
    ) -> crate::error::Result<()> {
        let payload = serde_json::to_string(payload)?;
        let mut db = self.db.clone();

        toasty::create!(Message {
            session_id: session_id.to_string(),
            payload,
        })
        .exec(&mut db)
        .await?;
        toasty::update!(Session::filter_by_id(session_id) {
            updated_at: Utc::now().to_rfc3339()
        })
        .exec(&mut db)
        .await?;

        Ok(())
    }

    pub async fn messages_for_session(
        &self,
        session_id: &str,
    ) -> crate::error::Result<Vec<serde_json::Value>> {
        let mut db = self.db.clone();
        let query = Message::filter(Message::fields().session_id().eq(session_id))
            .order_by(Message::fields().id().asc());

        query
            .exec(&mut db)
            .await?
            .into_iter()
            .map(|message| serde_json::from_str(&message.payload).map_err(Into::into))
            .collect()
    }

    pub async fn remove_session(&self, session_id: &str) -> crate::error::Result<()> {
        let mut db = self.db.clone();

        remove_traces_for_session(&self.db, session_id).await?;

        Message::filter(Message::fields().session_id().eq(session_id))
            .delete()
            .exec(&mut db)
            .await?;
        TeamMembership::filter(TeamMembership::fields().session_id().eq(session_id))
            .delete()
            .exec(&mut db)
            .await?;
        Session::filter(Session::fields().id().eq(session_id))
            .delete()
            .exec(&mut db)
            .await?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Team membership
    // -----------------------------------------------------------------------

    /// Join `session_id` to `team`, creating the team if it is new.
    ///
    /// A session belongs to at most one team, so joining replaces any previous
    /// membership for that session.
    pub async fn join_team(&self, session_id: &str, team: &str) -> crate::error::Result<()> {
        let mut db = self.db.clone();

        // Replace any existing membership (one team per session).
        TeamMembership::filter(TeamMembership::fields().session_id().eq(session_id))
            .delete()
            .exec(&mut db)
            .await?;

        toasty::create!(TeamMembership {
            session_id: session_id.to_string(),
            team: team.to_string(),
            joined_at: Utc::now().to_rfc3339(),
        })
        .exec(&mut db)
        .await?;

        Ok(())
    }

    /// Remove `session_id` from whatever team it is on, if any.
    pub async fn leave_team(&self, session_id: &str) -> crate::error::Result<()> {
        let mut db = self.db.clone();
        TeamMembership::filter(TeamMembership::fields().session_id().eq(session_id))
            .delete()
            .exec(&mut db)
            .await?;
        Ok(())
    }

    /// The team `session_id` currently belongs to, or `None`.
    pub async fn team_of_session(&self, session_id: &str) -> crate::error::Result<Option<String>> {
        let mut db = self.db.clone();
        let membership =
            TeamMembership::filter(TeamMembership::fields().session_id().eq(session_id))
                .exec(&mut db)
                .await?
                .into_iter()
                .next();
        Ok(membership.map(|m| m.team))
    }

    /// The session ids of every member of `team`, in ascending id order.
    pub async fn team_members(&self, team: &str) -> crate::error::Result<Vec<String>> {
        let mut db = self.db.clone();
        let mut members: Vec<String> =
            TeamMembership::filter(TeamMembership::fields().team().eq(team))
                .exec(&mut db)
                .await?
                .into_iter()
                .map(|m| m.session_id)
                .collect();
        members.sort();
        Ok(members)
    }

    /// The names of all active teams (those with at least one member), sorted.
    pub async fn list_teams(&self) -> crate::error::Result<Vec<String>> {
        let mut db = self.db.clone();
        let mut teams: Vec<String> = Query::<List<TeamMembership>>::all()
            .exec(&mut db)
            .await?
            .into_iter()
            .map(|m| m.team)
            .collect();
        teams.sort();
        teams.dedup();
        Ok(teams)
    }

    pub async fn record_trace(&self, trace: NewTrace) -> crate::error::Result<()> {
        let mut db = self.db.clone();

        toasty::create!(Trace {
            ts: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            session_id: trace.session_id,
            dir: trace.dir.as_str().to_string(),
            role: trace.role,
            kind: trace.kind.as_str().to_string(),
            method: trace.method,
            request_id: trace.request_id,
            payload: trace.payload,
        })
        .exec(&mut db)
        .await?;

        Ok(())
    }

    pub async fn traces(&self, query: TraceQuery) -> crate::error::Result<Vec<TraceRecord>> {
        select_traces(&self.db, query).await
    }
}

impl TryFrom<Session> for SessionRecord {
    type Error = crate::error::Error;

    fn try_from(session: Session) -> Result<Self, Self::Error> {
        Ok(Self {
            session_id: session.id,
            cwd: PathBuf::from(session.cwd),
            created_at: DateTime::parse_from_rfc3339(&session.created_at)?.with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&session.updated_at)?.with_timezone(&Utc),
        })
    }
}

async fn configure_sqlite_file(db: &toasty::Db) -> crate::error::Result<()> {
    let mut db = db.clone();
    toasty::sql::query("PRAGMA journal_mode = WAL")
        .exec(&mut db)
        .await?;
    Ok(())
}

async fn table_exists(db: &toasty::Db, table_name: &str) -> crate::error::Result<bool> {
    let mut db = db.clone();
    table_exists_on(&mut db, table_name).await
}

async fn table_exists_on(
    executor: &mut dyn toasty::Executor,
    table_name: &str,
) -> crate::error::Result<bool> {
    let rows = toasty::sql::query(
        "SELECT EXISTS (
            SELECT 1 FROM sqlite_master
            WHERE type = 'table' AND name = ?1
        )",
    )
    .bind(table_name)
    .column_types([toasty::stmt::Type::Bool])
    .exec(executor)
    .await?;

    Ok(matches!(
        rows.as_slice(),
        [toasty::stmt::Value::Record(record)]
            if matches!(record.as_slice(), [toasty::stmt::Value::Bool(true)])
    ))
}

/// Add missing tables to an existing database.
///
/// Diffs the current model schema against a copy with `table_names` removed, so
/// the generated migration contains only the statements that create those
/// tables (plus their indexes). Used to evolve older databases that predate a
/// table without a full rebuild.
///
/// The removed tables must be a *suffix* of the registered models (toasty
/// identifies tables by positional id, so removing a non-final table would
/// shift the ids of surviving tables and corrupt the schema). Tables are only
/// ever appended in this crate, so newly-missing tables are always a suffix.
async fn add_missing_tables(db: &toasty::Db, table_names: &[&str]) -> crate::error::Result<()> {
    let next_schema = db.schema().db.clone();
    let mut previous_schema = next_schema.clone();
    previous_schema
        .tables
        .retain(|table| !table_names.contains(&table.name.as_str()));

    let Some(generated) = toasty::migration::generate(
        db.driver(),
        &previous_schema,
        &next_schema,
        &toasty::schema::diff::RenameHints::new(),
    ) else {
        return Ok(());
    };

    let mut conn = db.connection().await?;
    toasty::sql::statement("BEGIN IMMEDIATE")
        .exec(&mut conn)
        .await?;

    let result = async {
        for statement in generated.migration.statements() {
            toasty::sql::statement(statement).exec(&mut conn).await?;
        }

        Ok(())
    }
    .await;

    match result {
        Ok(()) => {
            toasty::sql::statement("COMMIT").exec(&mut conn).await?;
            Ok(())
        }
        Err(err) => {
            let _ = toasty::sql::statement("ROLLBACK").exec(&mut conn).await;
            Err(err)
        }
    }
}

async fn remove_traces_for_session(db: &toasty::Db, session_id: &str) -> crate::error::Result<()> {
    let mut db = db.clone();
    Trace::filter(
        Trace::fields()
            .session_id()
            .eq(Some(session_id.to_string())),
    )
    .delete()
    .exec(&mut db)
    .await?;
    Ok(())
}

async fn select_traces(
    db: &toasty::Db,
    query: TraceQuery,
) -> crate::error::Result<Vec<TraceRecord>> {
    let mut db = db.clone();
    let mut stmt = Query::<List<Trace>>::all();
    if let Some(after_id) = query.after_id {
        stmt = stmt.and(Trace::fields().id().gt(after_id));
    }
    if let Some(session_id) = query.session_id {
        stmt = stmt.and(Trace::fields().session_id().eq(Some(session_id)));
    }
    if let Some(since) = query.since {
        stmt = stmt.and(
            Trace::fields()
                .ts()
                .ge(since.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        );
    }
    if let Some(method) = query.method {
        stmt = stmt.and(Trace::fields().method().eq(Some(method)));
    }
    if let Some(dir) = query.dir {
        stmt = stmt.and(Trace::fields().dir().eq(dir.as_str()));
    }

    stmt.order_by(Trace::fields().id().asc());
    stmt.limit(query.limit.unwrap_or(500).min(5000) as usize);
    stmt.exec(&mut db)
        .await?
        .into_iter()
        .map(TraceRecord::try_from)
        .collect()
}

impl TraceDirection {
    fn as_str(&self) -> &'static str {
        match self {
            Self::ClientToDaemon => "client_to_daemon",
            Self::DaemonToAgent => "daemon_to_agent",
            Self::AgentToDaemon => "agent_to_daemon",
            Self::DaemonToClient => "daemon_to_client",
            Self::Internal => "internal",
        }
    }

    pub fn parse(value: &str) -> std::result::Result<Self, TraceParseError> {
        match value {
            "client_to_daemon" => Ok(Self::ClientToDaemon),
            "daemon_to_agent" => Ok(Self::DaemonToAgent),
            "agent_to_daemon" => Ok(Self::AgentToDaemon),
            "daemon_to_client" => Ok(Self::DaemonToClient),
            "internal" => Ok(Self::Internal),
            _ => Err(TraceParseError(value.to_string())),
        }
    }
}

impl TraceKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Response => "response",
            Self::Notification => "notification",
            Self::Event => "event",
        }
    }

    fn parse(value: &str) -> std::result::Result<Self, TraceParseError> {
        match value {
            "request" => Ok(Self::Request),
            "response" => Ok(Self::Response),
            "notification" => Ok(Self::Notification),
            "event" => Ok(Self::Event),
            _ => Err(TraceParseError(value.to_string())),
        }
    }
}

#[derive(Debug)]
pub struct TraceParseError(String);

impl std::fmt::Display for TraceParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown trace enum value: {}", self.0)
    }
}

impl std::error::Error for TraceParseError {}

impl TryFrom<Trace> for TraceRecord {
    type Error = crate::error::Error;

    fn try_from(trace: Trace) -> Result<Self, Self::Error> {
        Ok(Self {
            id: trace.id,
            ts: DateTime::parse_from_rfc3339(&trace.ts)?.with_timezone(&Utc),
            session_id: trace.session_id,
            dir: TraceDirection::parse(&trace.dir)?,
            role: trace.role,
            kind: TraceKind::parse(&trace.kind)?,
            method: trace.method,
            request_id: trace.request_id,
            payload: trace.payload.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn records_and_filters_traces() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("jamsession.db"))
            .await
            .unwrap();

        store
            .record_trace(NewTrace {
                session_id: Some("sess-1".to_string()),
                dir: TraceDirection::ClientToDaemon,
                role: Some("acp-client".to_string()),
                kind: TraceKind::Request,
                method: Some("session/prompt".to_string()),
                request_id: Some("7".to_string()),
                payload: serde_json::json!({"prompt": "hello"}),
            })
            .await
            .unwrap();
        store
            .record_trace(NewTrace {
                session_id: Some("sess-2".to_string()),
                dir: TraceDirection::Internal,
                role: Some("daemon".to_string()),
                kind: TraceKind::Event,
                method: Some("session_created".to_string()),
                request_id: None,
                payload: serde_json::json!({}),
            })
            .await
            .unwrap();

        let traces = store
            .traces(TraceQuery {
                session_id: Some("sess-1".to_string()),
                ..TraceQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].dir, TraceDirection::ClientToDaemon);
        assert_eq!(traces[0].kind, TraceKind::Request);
        assert_eq!(traces[0].method.as_deref(), Some("session/prompt"));
        assert_eq!(traces[0].payload, serde_json::json!({"prompt": "hello"}));
    }

    #[tokio::test]
    async fn open_adds_trace_schema_to_existing_database() {
        let dir = tempfile::TempDir::new().unwrap();
        let db_path = dir.path().join("jamsession.db");
        let old_db = toasty::Db::builder()
            .models(toasty::models!(Session, Message))
            .build(toasty_driver_sqlite::Sqlite::open(&db_path))
            .await
            .unwrap();
        old_db.push_schema().await.unwrap();
        drop(old_db);

        let store = Store::open(&db_path).await.unwrap();
        store
            .record_trace(NewTrace {
                session_id: Some("sess-1".to_string()),
                dir: TraceDirection::Internal,
                role: Some("daemon".to_string()),
                kind: TraceKind::Event,
                method: Some("session_created".to_string()),
                request_id: None,
                payload: serde_json::json!({}),
            })
            .await
            .unwrap();

        let traces = store.traces(TraceQuery::default()).await.unwrap();
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].method.as_deref(), Some("session_created"));
    }

    #[tokio::test]
    async fn deleting_session_removes_its_traces() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("jamsession.db"))
            .await
            .unwrap();
        store.add_session("sess-1", dir.path()).await.unwrap();
        store
            .record_trace(NewTrace {
                session_id: Some("sess-1".to_string()),
                dir: TraceDirection::Internal,
                role: Some("daemon".to_string()),
                kind: TraceKind::Event,
                method: Some("session_created".to_string()),
                request_id: None,
                payload: serde_json::json!({}),
            })
            .await
            .unwrap();

        store.remove_session("sess-1").await.unwrap();

        assert!(
            store
                .traces(TraceQuery::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn join_leave_and_list_teams() {
        let store = Store::in_memory().await.unwrap();

        assert!(store.list_teams().await.unwrap().is_empty());
        assert_eq!(store.team_of_session("a").await.unwrap(), None);

        store.join_team("a", "frontend").await.unwrap();
        store.join_team("b", "frontend").await.unwrap();
        store.join_team("c", "backend").await.unwrap();

        assert_eq!(
            store.team_of_session("a").await.unwrap().as_deref(),
            Some("frontend")
        );
        assert_eq!(
            store.list_teams().await.unwrap(),
            vec!["backend".to_string(), "frontend".to_string()]
        );
        assert_eq!(
            store.team_members("frontend").await.unwrap(),
            vec!["a".to_string(), "b".to_string()]
        );

        store.leave_team("a").await.unwrap();
        assert_eq!(store.team_of_session("a").await.unwrap(), None);
        assert_eq!(
            store.team_members("frontend").await.unwrap(),
            vec!["b".to_string()]
        );
    }

    #[tokio::test]
    async fn joining_a_second_team_replaces_membership() {
        let store = Store::in_memory().await.unwrap();

        store.join_team("a", "frontend").await.unwrap();
        store.join_team("a", "backend").await.unwrap();

        // One-team-per-session: the session is only on the most recent team.
        assert_eq!(
            store.team_of_session("a").await.unwrap().as_deref(),
            Some("backend")
        );
        assert!(store.team_members("frontend").await.unwrap().is_empty());
        assert_eq!(
            store.team_members("backend").await.unwrap(),
            vec!["a".to_string()]
        );
        assert_eq!(
            store.list_teams().await.unwrap(),
            vec!["backend".to_string()]
        );
    }

    #[tokio::test]
    async fn removing_session_clears_team_membership() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("jamsession.db"))
            .await
            .unwrap();
        store.add_session("sess-1", dir.path()).await.unwrap();
        store.join_team("sess-1", "frontend").await.unwrap();

        store.remove_session("sess-1").await.unwrap();

        assert_eq!(store.team_of_session("sess-1").await.unwrap(), None);
        assert!(store.list_teams().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn team_membership_survives_reopen() {
        let dir = tempfile::TempDir::new().unwrap();
        let db_path = dir.path().join("jamsession.db");
        {
            let store = Store::open(&db_path).await.unwrap();
            store.join_team("a", "frontend").await.unwrap();
        }
        let store = Store::open(&db_path).await.unwrap();
        assert_eq!(
            store.team_of_session("a").await.unwrap().as_deref(),
            Some("frontend")
        );
    }

    #[tokio::test]
    async fn open_adds_team_table_to_existing_database() {
        let dir = tempfile::TempDir::new().unwrap();
        let db_path = dir.path().join("jamsession.db");
        // Simulate a database created before the team_memberships table existed.
        let old_db = toasty::Db::builder()
            .models(toasty::models!(Session, Message, Trace))
            .build(toasty_driver_sqlite::Sqlite::open(&db_path))
            .await
            .unwrap();
        old_db.push_schema().await.unwrap();
        drop(old_db);

        let store = Store::open(&db_path).await.unwrap();
        store.join_team("a", "frontend").await.unwrap();
        assert_eq!(
            store.team_of_session("a").await.unwrap().as_deref(),
            Some("frontend")
        );
    }
}
