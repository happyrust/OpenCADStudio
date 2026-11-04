//! SQLite persistence for the collaboration server.
//!
//! The schema follows `architecture.md` §2.5, with the two additions that
//! appendix A made authoritative: `sessions.workspace` and
//! `conflicts.resolved_value`.
//!
//! Two invariants are enforced *in the database*, not only in this module:
//! `journal_mode = WAL` (so readers never block the writer and the `-wal` /
//! `-shm` sidecars exist on disk) and append-only `audit_log` (triggers reject
//! `UPDATE` and `DELETE` even for a caller that goes around the API).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use ocs_collab::SessionStatus;

/// The session `mode` that never counts as an editing session (appendix A.3).
///
/// Same spelling as the drawing `state` value, but a different column: this one
/// lives on `sessions.mode`, that one on `drawings.state`.
const READ_ONLY_MODE: &str = "只读";

/// Everything `--seed` may contain. Every section is optional so a partial
/// seed file is legal.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SeedDoc {
    #[serde(default)]
    pub users: Vec<SeedUser>,
    #[serde(default)]
    pub pbs_nodes: Vec<SeedPbsNode>,
    #[serde(default)]
    pub pbs_acl: Vec<SeedAcl>,
    #[serde(default)]
    pub drawings: Vec<SeedDrawing>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeedUser {
    pub user_id: String,
    pub display_name: String,
    pub role: String,
    #[serde(default)]
    pub company: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeedPbsNode {
    pub node_id: String,
    pub name: String,
    pub node_type: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeedAcl {
    pub node_id: String,
    pub subject: String,
    pub permission: String,
    pub allowed: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeedDrawing {
    pub drawing_id: String,
    pub file_name: String,
    pub pbs_node_id: String,
    pub state: String,
    #[serde(default = "default_lifecycle")]
    pub lifecycle: String,
}

fn default_lifecycle() -> String {
    "设计中".to_owned()
}

/// One audit row, in the 9-field shape the contract asserts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRow {
    pub seq: i64,
    pub module: String,
    pub action: String,
    pub at: i64,
    pub object_ref: String,
    pub user_id: String,
    pub company: String,
    pub ip: String,
    pub workspace: String,
    pub change_text: String,
}

/// An audit row before it is stored (`seq` is assigned by SQLite).
#[derive(Debug, Clone)]
pub struct NewAuditRow {
    pub module: String,
    pub action: String,
    pub at: i64,
    pub object_ref: String,
    pub user_id: String,
    pub company: String,
    pub ip: String,
    pub workspace: String,
    pub change_text: String,
}

/// The outcome of one `commit_op` call.
///
/// The variant is the point: a caller never has to re-read `drawings.revision`
/// to find out whether its op landed, and a `Stale` result already carries the
/// revision the client has to rebase onto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpCommit {
    /// `base_revision` matched; `revision` is the new value and `seq` the audit
    /// row id.
    Applied { revision: i64, seq: i64 },
    /// `base_revision` did not match; `current` is the drawing's revision now.
    Stale { current: i64 },
}

/// Filters accepted by `GET /audit`.
#[derive(Debug, Clone, Default)]
pub struct AuditQuery {
    pub object_ref: Option<String>,
    pub user_id: Option<String>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// One page of audit rows plus the total matching the filters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditPage {
    pub items: Vec<AuditRow>,
    pub total: i64,
}

/// A stored conflict entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictRow {
    pub conflict_id: String,
    pub drawing_id: String,
    pub object_ref: String,
    pub field: String,
    pub value_a: String,
    pub value_b: String,
    pub base_revision: i64,
    pub status: String,
    pub resolution: Option<String>,
    pub resolved_value: Option<String>,
    pub resolved_by: Option<String>,
}

/// The connection handle. One `Mutex<Connection>` is enough for a single
/// process: WAL lets readers proceed while a write is in flight, and SQLite
/// serializes writers itself.
pub struct Store {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl Store {
    /// Opens (creating if needed) the database at `path` and applies the schema.
    ///
    /// A path that names a directory fails here, which is what makes
    /// `GET /health` able to report an unusable database instead of pretending.
    pub fn open(path: impl AsRef<Path>) -> Result<Store, String> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("create {}: {err}", parent.display()))?;
            }
        }

        let conn = Connection::open(&path)
            .map_err(|err| format!("open {}: {err}", path.display()))?;
        configure(&conn)?;
        create_schema(&conn)?;

        Ok(Store {
            conn: Mutex::new(conn),
            path,
        })
    }

    /// An in-memory database, used by tests.
    pub fn open_in_memory() -> Result<Store, String> {
        let conn = Connection::open_in_memory().map_err(|err| format!("open memory: {err}"))?;
        configure(&conn)?;
        create_schema(&conn)?;
        Ok(Store {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `PRAGMA journal_mode` as reported by SQLite (`wal` when WAL is active).
    pub fn journal_mode(&self) -> Result<String, String> {
        let conn = self.lock()?;
        conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .map_err(|err| format!("pragma journal_mode: {err}"))
    }

    /// Liveness **and writability** probe.
    ///
    /// `SELECT 1` alone is a false healthy: a database file that exists and is
    /// readable but not writable (read-only attribute, read-only volume, a lock
    /// held by another user) answers the read and still makes every write path
    /// — `POST /session` included — fail. So the probe opens a real write
    /// transaction and rolls it back.
    ///
    /// `PRAGMA quick_check` is deliberately *not* used: it validates page
    /// structure and says nothing about write permission.
    ///
    /// Everything happens on one guard, because `lock()` is not reentrant.
    pub fn health_check(&self) -> bool {
        let conn = match self.lock() {
            Ok(conn) => conn,
            Err(_) => return false,
        };

        // Fast fail on a database that cannot even answer a trivial read.
        if !matches!(
            conn.query_row("SELECT 1", [], |row| row.get::<_, i64>(0)),
            Ok(1)
        ) {
            return false;
        }

        // `BEGIN IMMEDIATE` alone is not enough: SQLite grants the write lock on
        // a read-only database and only refuses at the first actual write. So
        // the probe performs a real write — a zero-row `UPDATE` — inside a
        // transaction that is always rolled back. A zero-row UPDATE still needs
        // the write lock, so it fails on a read-only file without changing data.
        if conn.execute_batch("BEGIN IMMEDIATE").is_err() {
            return false;
        }

        let writable = conn
            .execute("UPDATE drawings SET revision = revision WHERE 0", [])
            .is_ok();
        if !writable {
            let _ = conn.execute_batch("ROLLBACK");
            return false;
        }

        // Release the write lock immediately; a probe must not hold it. A failed
        // rollback leaves the connection mid-transaction, so it is unhealthy.
        conn.execute_batch("ROLLBACK").is_ok()
    }

    /// Clears sessions and claims. Called on startup: per appendix A.2 the old
    /// `session_id`s are gone, while users/PBS/drawings survive.
    pub fn clear_ephemeral(&self) -> Result<(), String> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM claims", [])
            .map_err(|err| format!("clear claims: {err}"))?;
        conn.execute("DELETE FROM sessions", [])
            .map_err(|err| format!("clear sessions: {err}"))?;
        Ok(())
    }

    /// Applies a seed document. Rows are upserted so re-seeding is idempotent.
    pub fn seed(&self, doc: &SeedDoc) -> Result<(), String> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|err| format!("begin seed: {err}"))?;

        for user in &doc.users {
            tx.execute(
                "INSERT INTO users (user_id, display_name, role, company)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(user_id) DO UPDATE SET
                   display_name = excluded.display_name,
                   role = excluded.role,
                   company = excluded.company",
                params![user.user_id, user.display_name, user.role, user.company],
            )
            .map_err(|err| format!("seed user {}: {err}", user.user_id))?;
        }

        for node in &doc.pbs_nodes {
            tx.execute(
                "INSERT INTO pbs_nodes (node_id, name, node_type, parent_id)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(node_id) DO UPDATE SET
                   name = excluded.name,
                   node_type = excluded.node_type,
                   parent_id = excluded.parent_id",
                params![node.node_id, node.name, node.node_type, node.parent_id],
            )
            .map_err(|err| format!("seed pbs node {}: {err}", node.node_id))?;
        }

        for acl in &doc.pbs_acl {
            tx.execute(
                "INSERT INTO pbs_acl (node_id, subject, permission, allowed)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(node_id, subject, permission) DO UPDATE SET
                   allowed = excluded.allowed",
                params![acl.node_id, acl.subject, acl.permission, i64::from(acl.allowed)],
            )
            .map_err(|err| format!("seed acl {}/{}: {err}", acl.node_id, acl.subject))?;
        }

        for drawing in &doc.drawings {
            tx.execute(
                "INSERT INTO drawings (drawing_id, file_name, pbs_node_id, state, lifecycle, revision)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0)
                 ON CONFLICT(drawing_id) DO UPDATE SET
                   file_name = excluded.file_name,
                   pbs_node_id = excluded.pbs_node_id,
                   state = excluded.state,
                   lifecycle = excluded.lifecycle",
                params![
                    drawing.drawing_id,
                    drawing.file_name,
                    drawing.pbs_node_id,
                    drawing.state,
                    drawing.lifecycle
                ],
            )
            .map_err(|err| format!("seed drawing {}: {err}", drawing.drawing_id))?;
        }

        tx.commit().map_err(|err| format!("commit seed: {err}"))
    }

    /// Reads a seed file from disk and applies it.
    pub fn seed_from_file(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let text = std::fs::read_to_string(path.as_ref())
            .map_err(|err| format!("read seed {}: {err}", path.as_ref().display()))?;
        let doc: SeedDoc =
            serde_json::from_str(&text).map_err(|err| format!("parse seed json: {err}"))?;
        self.seed(&doc)
    }

    pub fn user_role(&self, user_id: &str) -> Result<Option<(String, String)>, String> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT role, company FROM users WHERE user_id = ?1",
            params![user_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|err| format!("lookup user {user_id}: {err}"))
    }

    /// `(pbs_node_id, state, revision)` for a drawing.
    pub fn drawing(&self, drawing_id: &str) -> Result<Option<(String, String, i64)>, String> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT pbs_node_id, state, revision FROM drawings WHERE drawing_id = ?1",
            params![drawing_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|err| format!("lookup drawing {drawing_id}: {err}"))
    }

    /// Registers a drawing if it is not known yet, so a fresh database can
    /// accept `hello` for any drawing id. The default PBS root is created on
    /// demand because `drawings.pbs_node_id` is a foreign key.
    pub fn ensure_drawing(&self, drawing_id: &str) -> Result<(), String> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR IGNORE INTO pbs_nodes (node_id, name, node_type, parent_id)
             VALUES (?1, '根节点', '工厂', NULL)",
            params![DEFAULT_PBS_NODE],
        )
        .map_err(|err| format!("ensure pbs root: {err}"))?;
        conn.execute(
            "INSERT OR IGNORE INTO drawings
               (drawing_id, file_name, pbs_node_id, state, lifecycle, revision)
             VALUES (?1, ?1, ?2, '协同编辑', '设计中', 0)",
            params![drawing_id, DEFAULT_PBS_NODE],
        )
        .map_err(|err| format!("ensure drawing {drawing_id}: {err}"))?;
        Ok(())
    }

    pub fn set_drawing_state(&self, drawing_id: &str, state: &str) -> Result<(), String> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE drawings SET state = ?2 WHERE drawing_id = ?1",
            params![drawing_id, state],
        )
        .map_err(|err| format!("set drawing state {drawing_id}: {err}"))?;
        Ok(())
    }

    pub fn set_drawing_revision(&self, drawing_id: &str, revision: i64) -> Result<(), String> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE drawings SET revision = ?2 WHERE drawing_id = ?1",
            params![drawing_id, revision],
        )
        .map_err(|err| format!("set drawing revision {drawing_id}: {err}"))?;
        Ok(())
    }

    /// One op's commit: the revision check, the increment and the audit insert
    /// all happen under a single `lock()`.
    ///
    /// The read-compare-write sequence used to be spread over several calls
    /// (`drawing` → compare → `set_drawing_revision` → `audit_append`), so two
    /// ops that both read revision `n` could both "win" and both write `n + 1`.
    /// Here the whole sequence runs on one guard of one `Mutex<Connection>`,
    /// which means no other thread can interleave — that, and not the SQL, is
    /// where the atomicity comes from.
    ///
    /// Nothing inside the transaction may call a `&self` method that locks:
    /// `set_drawing_revision`, `audit_append` and `drawing` all take the same
    /// non-reentrant mutex. Only `conn` is used.
    pub fn commit_op(
        &self,
        drawing_id: &str,
        base_revision: i64,
        row: &NewAuditRow,
    ) -> Result<OpCommit, String> {
        let conn = self.lock()?;

        // A drawing that is not registered yet counts as revision 0, the same
        // fallback `ws::current_revision` uses.
        let current: i64 = conn
            .query_row(
                "SELECT revision FROM drawings WHERE drawing_id = ?1",
                params![drawing_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|err| format!("read revision of {drawing_id}: {err}"))?
            .unwrap_or(0);

        if current != base_revision {
            return Ok(OpCommit::Stale { current });
        }

        let next = current + 1;

        // `BEGIN IMMEDIATE` takes the write lock before the first write, so a
        // concurrent writer is serialized here rather than at COMMIT time.
        if let Err(err) = conn.execute_batch("BEGIN IMMEDIATE") {
            return Err(format!("begin op commit {drawing_id}: {err}"));
        }

        // The revision bump and the audit row are one unit: neither may survive
        // without the other. Nothing in here may call a `&self` method, because
        // `lock()` is not reentrant.
        let written = (|| -> Result<i64, String> {
            conn.execute(
                "UPDATE drawings SET revision = ?2 WHERE drawing_id = ?1",
                params![drawing_id, next],
            )
            .map_err(|err| format!("bump revision of {drawing_id}: {err}"))?;

            conn.execute(
                "INSERT INTO audit_log
                   (module, action, at, object_ref, user_id, company, ip, workspace, change_text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    row.module,
                    row.action,
                    row.at,
                    row.object_ref,
                    row.user_id,
                    row.company,
                    row.ip,
                    row.workspace,
                    row.change_text
                ],
            )
            .map_err(|err| format!("append audit for {drawing_id}: {err}"))?;

            Ok(conn.last_insert_rowid())
        })();

        match written {
            Ok(seq) => {
                if let Err(err) = conn.execute_batch("COMMIT") {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(format!("commit op {drawing_id}: {err}"));
                }
                Ok(OpCommit::Applied {
                    revision: next,
                    seq,
                })
            }
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }

    /// `(node_id, parent_id)` for every PBS node, in storage order.
    pub fn list_pbs_nodes(&self) -> Result<Vec<(String, Option<String>)>, String> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT node_id, parent_id FROM pbs_nodes")
            .map_err(|err| format!("prepare pbs nodes: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            })
            .map_err(|err| format!("query pbs nodes: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("read pbs nodes: {err}"))?;
        Ok(rows)
    }

    /// `(node_id, subject, permission, allowed)` for every ACL row.
    pub fn list_pbs_acl(&self) -> Result<Vec<(String, String, String, bool)>, String> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT node_id, subject, permission, allowed FROM pbs_acl")
            .map_err(|err| format!("prepare pbs acl: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            })
            .map_err(|err| format!("query pbs acl: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("read pbs acl: {err}"))?;
        Ok(rows)
    }

    /// Appends an audit row and returns the assigned `seq`.
    pub fn audit_append(&self, row: &NewAuditRow) -> Result<i64, String> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO audit_log
               (module, action, at, object_ref, user_id, company, ip, workspace, change_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                row.module,
                row.action,
                row.at,
                row.object_ref,
                row.user_id,
                row.company,
                row.ip,
                row.workspace,
                row.change_text
            ],
        )
        .map_err(|err| format!("append audit: {err}"))?;
        Ok(conn.last_insert_rowid())
    }

    /// Queries audit rows, newest first (`at DESC, seq DESC`).
    pub fn audit_query(&self, query: &AuditQuery) -> Result<AuditPage, String> {
        let conn = self.lock()?;

        let mut where_sql = String::from(" WHERE 1 = 1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(object_ref) = &query.object_ref {
            where_sql.push_str(" AND object_ref = ?");
            args.push(rusqlite::types::Value::Text(object_ref.clone()));
        }
        if let Some(user_id) = &query.user_id {
            where_sql.push_str(" AND user_id = ?");
            args.push(rusqlite::types::Value::Text(user_id.clone()));
        }
        if let Some(from) = query.from {
            where_sql.push_str(" AND at >= ?");
            args.push(rusqlite::types::Value::Integer(from));
        }
        if let Some(to) = query.to {
            where_sql.push_str(" AND at <= ?");
            args.push(rusqlite::types::Value::Integer(to));
        }

        let params_ref: Vec<&dyn rusqlite::ToSql> = args.iter().map(|arg| arg as &dyn rusqlite::ToSql).collect();

        let total: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM audit_log{where_sql}"),
                params_ref.as_slice(),
                |row| row.get(0),
            )
            .map_err(|err| format!("count audit: {err}"))?;

        let limit = query.limit.unwrap_or(100).clamp(1, 1000);
        let offset = query.offset.unwrap_or(0).max(0);
        let sql = format!(
            "SELECT seq, module, action, at, object_ref, user_id, company, ip, workspace, change_text
             FROM audit_log{where_sql}
             ORDER BY at DESC, seq DESC
             LIMIT ? OFFSET ?"
        );
        let mut with_page: Vec<rusqlite::types::Value> = args.clone();
        with_page.push(rusqlite::types::Value::Integer(limit));
        with_page.push(rusqlite::types::Value::Integer(offset));
        let page_ref: Vec<&dyn rusqlite::ToSql> =
            with_page.iter().map(|arg| arg as &dyn rusqlite::ToSql).collect();

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|err| format!("prepare audit: {err}"))?;
        let items = stmt
            .query_map(page_ref.as_slice(), |row| {
                Ok(AuditRow {
                    seq: row.get(0)?,
                    module: row.get(1)?,
                    action: row.get(2)?,
                    at: row.get(3)?,
                    object_ref: row.get(4)?,
                    user_id: row.get(5)?,
                    company: row.get(6)?,
                    ip: row.get(7)?,
                    workspace: row.get(8)?,
                    change_text: row.get(9)?,
                })
            })
            .map_err(|err| format!("query audit: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("read audit: {err}"))?;

        Ok(AuditPage { items, total })
    }

    pub fn audit_count(&self) -> Result<i64, String> {
        let conn = self.lock()?;
        conn.query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
            .map_err(|err| format!("count audit: {err}"))
    }

    /// Inserts a conflict entry awaiting adjudication.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_conflict(&self, conflict: &ConflictRow) -> Result<(), String> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO conflicts
               (conflict_id, drawing_id, object_ref, field, value_a, value_b,
                base_revision, status, resolution, resolved_value, resolved_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                conflict.conflict_id,
                conflict.drawing_id,
                conflict.object_ref,
                conflict.field,
                conflict.value_a,
                conflict.value_b,
                conflict.base_revision,
                conflict.status,
                conflict.resolution,
                conflict.resolved_value,
                conflict.resolved_by
            ],
        )
        .map_err(|err| format!("insert conflict: {err}"))?;
        Ok(())
    }

    pub fn list_conflicts(
        &self,
        drawing_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<ConflictRow>, String> {
        let conn = self.lock()?;
        let mut where_sql = String::from(" WHERE 1 = 1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(drawing_id) = drawing_id {
            where_sql.push_str(" AND drawing_id = ?");
            args.push(rusqlite::types::Value::Text(drawing_id.to_owned()));
        }
        if let Some(status) = status {
            where_sql.push_str(" AND status = ?");
            args.push(rusqlite::types::Value::Text(status.to_owned()));
        }
        let args_ref: Vec<&dyn rusqlite::ToSql> =
            args.iter().map(|arg| arg as &dyn rusqlite::ToSql).collect();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT conflict_id, drawing_id, object_ref, field, value_a, value_b,
                        base_revision, status, resolution, resolved_value, resolved_by
                 FROM conflicts{where_sql} ORDER BY conflict_id"
            ))
            .map_err(|err| format!("prepare conflicts: {err}"))?;
        let rows = stmt
            .query_map(args_ref.as_slice(), |row| {
                Ok(ConflictRow {
                    conflict_id: row.get(0)?,
                    drawing_id: row.get(1)?,
                    object_ref: row.get(2)?,
                    field: row.get(3)?,
                    value_a: row.get(4)?,
                    value_b: row.get(5)?,
                    base_revision: row.get(6)?,
                    status: row.get(7)?,
                    resolution: row.get(8)?,
                    resolved_value: row.get(9)?,
                    resolved_by: row.get(10)?,
                })
            })
            .map_err(|err| format!("query conflicts: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("read conflicts: {err}"))?;
        Ok(rows)
    }

    /// Resolves a pending conflict. Returns `false` when the row is unknown or
    /// already resolved (adjudication happens once).
    pub fn resolve_conflict(
        &self,
        conflict_id: &str,
        resolution: &str,
        resolved_value: Option<&str>,
        resolved_by: &str,
    ) -> Result<bool, String> {
        let conn = self.lock()?;
        let changed = conn
            .execute(
                "UPDATE conflicts
                 SET status = '已裁决', resolution = ?2, resolved_value = ?3, resolved_by = ?4
                 WHERE conflict_id = ?1 AND status = '待裁决'",
                params![conflict_id, resolution, resolved_value, resolved_by],
            )
            .map_err(|err| format!("resolve conflict {conflict_id}: {err}"))?;
        Ok(changed > 0)
    }

    /// Persists one session row (status included, so restarts and takeovers
    /// stay observable through the database).
    pub fn upsert_session(&self, session: &SessionRow) -> Result<(), String> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO sessions
               (session_id, drawing_id, user_id, mode, status, base_revision,
                last_heartbeat, created_at, workspace)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(session_id) DO UPDATE SET
               mode = excluded.mode,
               status = excluded.status,
               base_revision = excluded.base_revision,
               last_heartbeat = excluded.last_heartbeat,
               workspace = excluded.workspace",
            params![
                session.session_id,
                session.drawing_id,
                session.user_id,
                session.mode,
                session.status,
                session.base_revision,
                session.last_heartbeat,
                session.created_at,
                session.workspace
            ],
        )
        .map_err(|err| format!("upsert session {}: {err}", session.session_id))?;
        Ok(())
    }

    pub fn session_row(&self, session_id: &str) -> Result<Option<SessionRow>, String> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT session_id, drawing_id, user_id, mode, status, base_revision,
                    last_heartbeat, created_at, workspace
             FROM sessions WHERE session_id = ?1",
            params![session_id],
            |row| {
                Ok(SessionRow {
                    session_id: row.get(0)?,
                    drawing_id: row.get(1)?,
                    user_id: row.get(2)?,
                    mode: row.get(3)?,
                    status: row.get(4)?,
                    base_revision: row.get(5)?,
                    last_heartbeat: row.get(6)?,
                    created_at: row.get(7)?,
                    workspace: row.get(8)?,
                })
            },
        )
        .optional()
        .map_err(|err| format!("lookup session {session_id}: {err}"))
    }

    /// Every live editing session on `drawing_id`, as `(session_id, user_id)`.
    ///
    /// This is the authoritative source for the single-editor half of the state
    /// tier (appendix A.3): the row is `status='活跃'`, its `mode` is not
    /// `只读`, and its `last_heartbeat` is still inside the heartbeat timeout.
    /// The timeout is applied here instead of trusting the stored status alone,
    /// because a crashed client never writes `失效` itself — `SessionRegistry::sweep`
    /// is the only thing that flips the column, and it only runs on live sockets.
    ///
    /// Reading the table (rather than the in-memory registry) means a session
    /// created by `POST /session` is visible to `/permission/check` even though
    /// it never went through the WebSocket handshake (`VAL-PERM-017`).
    ///
    /// One statement, one lock acquisition: the store mutex is not reentrant.
    pub fn active_editing_sessions(
        &self,
        drawing_id: &str,
        now: i64,
        heartbeat_timeout: i64,
    ) -> Result<Vec<(String, String)>, String> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare(
                "SELECT session_id, user_id FROM sessions
                 WHERE drawing_id = ?1
                   AND status = ?2
                   AND mode != ?3
                   AND (?4 - last_heartbeat) <= ?5
                 ORDER BY created_at, session_id",
            )
            .map_err(|err| format!("prepare active sessions on {drawing_id}: {err}"))?;
        let rows = statement
            .query_map(
                params![
                    drawing_id,
                    SessionStatus::Active.as_str(),
                    READ_ONLY_MODE,
                    now,
                    heartbeat_timeout
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(|err| format!("query active sessions on {drawing_id}: {err}"))?;
        let mut live = Vec::new();
        for row in rows {
            live.push(row.map_err(|err| format!("read active session on {drawing_id}: {err}"))?);
        }
        Ok(live)
    }

    pub fn insert_claim(&self, claim: &ClaimRow) -> Result<(), String> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO claims (tag, drawing_id, session_id, user_id, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(tag) DO UPDATE SET
               drawing_id = excluded.drawing_id,
               session_id = excluded.session_id,
               user_id = excluded.user_id,
               expires_at = excluded.expires_at",
            params![
                claim.tag,
                claim.drawing_id,
                claim.session_id,
                claim.user_id,
                claim.expires_at
            ],
        )
        .map_err(|err| format!("insert claim {}: {err}", claim.tag))?;
        Ok(())
    }

    pub fn delete_claim(&self, tag: &str) -> Result<(), String> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM claims WHERE tag = ?1", params![tag])
            .map_err(|err| format!("delete claim {tag}: {err}"))?;
        Ok(())
    }

    pub fn delete_claims_of_session(&self, session_id: &str) -> Result<Vec<String>, String> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT tag FROM claims WHERE session_id = ?1")
            .map_err(|err| format!("prepare session claims: {err}"))?;
        let tags = stmt
            .query_map(params![session_id], |row| row.get::<_, String>(0))
            .map_err(|err| format!("query session claims: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("read session claims: {err}"))?;
        drop(stmt);
        conn.execute("DELETE FROM claims WHERE session_id = ?1", params![session_id])
            .map_err(|err| format!("delete session claims: {err}"))?;
        Ok(tags)
    }

    pub fn claims_on_drawing(&self, drawing_id: &str) -> Result<Vec<ClaimRow>, String> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT tag, drawing_id, session_id, user_id, expires_at
                 FROM claims WHERE drawing_id = ?1 ORDER BY tag",
            )
            .map_err(|err| format!("prepare drawing claims: {err}"))?;
        let rows = stmt
            .query_map(params![drawing_id], |row| {
                Ok(ClaimRow {
                    tag: row.get(0)?,
                    drawing_id: row.get(1)?,
                    session_id: row.get(2)?,
                    user_id: row.get(3)?,
                    expires_at: row.get(4)?,
                })
            })
            .map_err(|err| format!("query drawing claims: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("read drawing claims: {err}"))?;
        Ok(rows)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.conn
            .lock()
            .map_err(|_| "store mutex poisoned".to_owned())
    }
}

/// One `sessions` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub session_id: String,
    pub drawing_id: String,
    pub user_id: String,
    pub mode: String,
    pub status: String,
    pub base_revision: i64,
    pub last_heartbeat: i64,
    pub created_at: i64,
    pub workspace: String,
}

/// One `claims` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimRow {
    pub tag: String,
    pub drawing_id: String,
    pub session_id: String,
    pub user_id: String,
    pub expires_at: i64,
}

/// The PBS node a drawing lands on when nothing else is configured.
pub const DEFAULT_PBS_NODE: &str = "n-root";

fn configure(conn: &Connection) -> Result<(), String> {
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|err| format!("busy_timeout: {err}"))?;

    // WAL is the whole point of this store: readers proceed while a write is
    // in flight, and the -wal/-shm sidecars are what VAL-SRV-023 looks for.
    let mode: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(|err| format!("enable WAL: {err}"))?;
    if !mode.eq_ignore_ascii_case("wal") && !mode.eq_ignore_ascii_case("memory") {
        return Err(format!("journal_mode is {mode}, expected wal"));
    }

    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;",
    )
    .map_err(|err| format!("pragmas: {err}"))?;
    Ok(())
}

fn create_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS users (
  user_id      TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  role         TEXT NOT NULL,
  company      TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS pbs_nodes (
  node_id   TEXT PRIMARY KEY,
  name      TEXT NOT NULL,
  node_type TEXT NOT NULL,
  parent_id TEXT REFERENCES pbs_nodes(node_id)
);

CREATE TABLE IF NOT EXISTS pbs_acl (
  node_id    TEXT NOT NULL REFERENCES pbs_nodes(node_id),
  subject    TEXT NOT NULL,
  permission TEXT NOT NULL,
  allowed    INTEGER NOT NULL,
  PRIMARY KEY (node_id, subject, permission)
);

CREATE TABLE IF NOT EXISTS drawings (
  drawing_id  TEXT PRIMARY KEY,
  file_name   TEXT NOT NULL UNIQUE,
  pbs_node_id TEXT NOT NULL REFERENCES pbs_nodes(node_id),
  state       TEXT NOT NULL,
  lifecycle   TEXT NOT NULL DEFAULT '设计中',
  revision    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS sessions (
  session_id     TEXT PRIMARY KEY,
  drawing_id     TEXT NOT NULL REFERENCES drawings(drawing_id),
  user_id        TEXT NOT NULL,
  mode           TEXT NOT NULL,
  status         TEXT NOT NULL,
  base_revision  INTEGER NOT NULL,
  last_heartbeat INTEGER NOT NULL,
  created_at     INTEGER NOT NULL,
  workspace      TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS claims (
  tag        TEXT PRIMARY KEY,
  drawing_id TEXT NOT NULL REFERENCES drawings(drawing_id),
  session_id TEXT NOT NULL REFERENCES sessions(session_id),
  user_id    TEXT NOT NULL,
  expires_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS conflicts (
  conflict_id   TEXT PRIMARY KEY,
  drawing_id    TEXT NOT NULL,
  object_ref    TEXT NOT NULL,
  field         TEXT NOT NULL,
  value_a       TEXT NOT NULL,
  value_b       TEXT NOT NULL,
  base_revision INTEGER NOT NULL,
  status        TEXT NOT NULL,
  resolution    TEXT,
  resolved_value TEXT,
  resolved_by   TEXT
);

CREATE TABLE IF NOT EXISTS audit_log (
  seq         INTEGER PRIMARY KEY AUTOINCREMENT,
  module      TEXT NOT NULL,
  action      TEXT NOT NULL,
  at          INTEGER NOT NULL,
  object_ref  TEXT NOT NULL,
  user_id     TEXT NOT NULL,
  company     TEXT NOT NULL,
  ip          TEXT NOT NULL,
  workspace   TEXT NOT NULL,
  change_text TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS audit_by_tag ON audit_log(object_ref, at DESC);
CREATE INDEX IF NOT EXISTS audit_by_user ON audit_log(user_id, at DESC);
CREATE INDEX IF NOT EXISTS sessions_by_drawing ON sessions(drawing_id, status);

-- Append-only audit log, enforced by the database itself so no code path can
-- mutate history (VAL-SRV-021).
CREATE TRIGGER IF NOT EXISTS audit_log_no_update
BEFORE UPDATE ON audit_log
BEGIN
  SELECT RAISE(ABORT, 'audit_log is append-only');
END;

CREATE TRIGGER IF NOT EXISTS audit_log_no_delete
BEFORE DELETE ON audit_log
BEGIN
  SELECT RAISE(ABORT, 'audit_log is append-only');
END;
"#,
    )
    .map_err(|err| format!("create schema: {err}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("ocs_collab_store_tests");
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir.join(format!("{name}-{}.db", uuid::Uuid::new_v4()))
    }

    fn audit_row(object_ref: &str, at: i64, change: &str) -> NewAuditRow {
        NewAuditRow {
            module: "P&ID".into(),
            action: "set_property".into(),
            at,
            object_ref: object_ref.into(),
            user_id: "u1".into(),
            company: "甲设计院".into(),
            ip: "127.0.0.1".into(),
            workspace: "plant-a".into(),
            change_text: change.into(),
        }
    }

    #[test]
    fn opening_a_file_database_enables_wal() {
        let path = temp_db_path("wal");
        let store = Store::open(&path).expect("open store");
        assert_eq!(store.journal_mode().expect("journal mode"), "wal");
        assert!(store.health_check());
        drop(store);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn a_write_produces_the_wal_and_shm_sidecars() {
        let path = temp_db_path("sidecar");
        let store = Store::open(&path).expect("open store");
        store
            .audit_append(&audit_row("V-1001", 1_700_000_000, "1.6MPa"))
            .expect("append audit");
        assert!(path.exists(), "main db file must exist");
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        let shm = PathBuf::from(format!("{}-shm", path.display()));
        assert!(wal.exists(), "expected {wal:?}");
        assert!(shm.exists(), "expected {shm:?}");
        drop(store);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&wal);
        let _ = std::fs::remove_file(&shm);
    }

    #[test]
    fn opening_a_directory_as_the_database_fails() {
        let dir = std::env::temp_dir().join("ocs_collab_store_tests_dir");
        std::fs::create_dir_all(&dir).expect("create dir");
        let result = Store::open(&dir);
        assert!(result.is_err(), "a directory must not open as a database");
    }

    #[test]
    fn audit_rows_round_trip_with_all_nine_fields() {
        let store = Store::open_in_memory().expect("memory store");
        let seq = store
            .audit_append(&audit_row("V-1001", 1_700_000_000, "设计压力 1.0 -> 1.6MPa"))
            .expect("append");
        assert!(seq > 0);
        let page = store
            .audit_query(&AuditQuery {
                object_ref: Some("V-1001".into()),
                ..AuditQuery::default()
            })
            .expect("query");
        assert_eq!(page.total, 1);
        let row = &page.items[0];
        assert_eq!(row.seq, seq);
        assert_eq!(row.object_ref, "V-1001");
        assert_eq!(row.user_id, "u1");
        assert_eq!(row.company, "甲设计院");
        assert_eq!(row.ip, "127.0.0.1");
        assert_eq!(row.workspace, "plant-a");
        assert!(!row.change_text.is_empty());
        assert_eq!(row.at, 1_700_000_000);
        assert_eq!(row.module, "P&ID");
        assert_eq!(row.action, "set_property");
    }

    #[test]
    fn audit_by_tag_returns_the_newest_first() {
        let store = Store::open_in_memory().expect("memory store");
        store
            .audit_append(&audit_row("V-1001", 1_700_000_000, "A"))
            .expect("append A");
        store
            .audit_append(&audit_row("V-1001", 1_700_000_100, "B"))
            .expect("append B");
        let page = store
            .audit_query(&AuditQuery {
                object_ref: Some("V-1001".into()),
                ..AuditQuery::default()
            })
            .expect("query");
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].change_text, "B");
        assert_eq!(page.items[1].change_text, "A");
    }

    #[test]
    fn audit_from_and_to_are_inclusive() {
        let store = Store::open_in_memory().expect("memory store");
        for at in [100, 200, 300] {
            store
                .audit_append(&audit_row("V-1001", at, "x"))
                .expect("append");
        }
        let page = store
            .audit_query(&AuditQuery {
                from: Some(200),
                to: Some(300),
                ..AuditQuery::default()
            })
            .expect("query");
        assert_eq!(page.total, 2, "both bounds are inclusive");
    }

    #[test]
    fn audit_log_rejects_update_and_delete_at_the_database_level() {
        let store = Store::open_in_memory().expect("memory store");
        store
            .audit_append(&audit_row("V-1001", 1_700_000_000, "A"))
            .expect("append");
        {
            let conn = store.conn.lock().expect("lock");
            let update = conn.execute("UPDATE audit_log SET change_text = 'B'", []);
            assert!(update.is_err(), "UPDATE must be refused by the trigger");
            let delete = conn.execute("DELETE FROM audit_log", []);
            assert!(delete.is_err(), "DELETE must be refused by the trigger");
        }
        assert_eq!(store.audit_count().expect("count"), 1);
    }

    #[test]
    fn audit_limit_and_offset_page_the_results() {
        let store = Store::open_in_memory().expect("memory store");
        for index in 0..5 {
            store
                .audit_append(&audit_row("V-1001", 1_000 + index, "x"))
                .expect("append");
        }
        let page = store
            .audit_query(&AuditQuery {
                limit: Some(2),
                offset: Some(1),
                ..AuditQuery::default()
            })
            .expect("query");
        assert_eq!(page.total, 5, "total ignores paging");
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].at, 1_003);
    }

    #[test]
    fn seeding_registers_users_pbs_drawings_and_acl() {
        let store = Store::open_in_memory().expect("memory store");
        let doc: SeedDoc = serde_json::from_str(
            r#"{
              "users":[{"user_id":"u1","display_name":"工艺甲","role":"工艺设计人员","company":"甲设计院"}],
              "pbs_nodes":[{"node_id":"n-plant","name":"工厂","node_type":"工厂"},
                           {"node_id":"n-unit-a","name":"装置A","node_type":"装置","parent_id":"n-plant"}],
              "pbs_acl":[{"node_id":"n-unit-a","subject":"u1","permission":"编辑","allowed":true}],
              "drawings":[{"drawing_id":"PID-1101","file_name":"1101.dwg","pbs_node_id":"n-unit-a","state":"协同编辑"}]
            }"#,
        )
        .expect("seed parses");
        store.seed(&doc).expect("seed applies");

        let (role, company) = store.user_role("u1").expect("lookup").expect("user exists");
        assert_eq!(role, "工艺设计人员");
        assert_eq!(company, "甲设计院");
        let (node, state, revision) = store
            .drawing("PID-1101")
            .expect("lookup")
            .expect("drawing exists");
        assert_eq!(node, "n-unit-a");
        assert_eq!(state, "协同编辑");
        assert_eq!(revision, 0);
    }

    #[test]
    fn seeding_twice_is_idempotent() {
        let store = Store::open_in_memory().expect("memory store");
        let doc: SeedDoc = serde_json::from_str(
            r#"{"users":[{"user_id":"u1","display_name":"工艺甲","role":"工艺设计人员"}]}"#,
        )
        .expect("seed parses");
        store.seed(&doc).expect("first seed");
        store.seed(&doc).expect("second seed");
        assert_eq!(
            store.user_role("u1").expect("lookup").map(|(r, _)| r),
            Some("工艺设计人员".into())
        );
    }

    #[test]
    fn a_partial_seed_document_is_legal() {
        let store = Store::open_in_memory().expect("memory store");
        let doc: SeedDoc = serde_json::from_str("{}").expect("empty seed parses");
        store.seed(&doc).expect("empty seed applies");
    }

    #[test]
    fn clearing_ephemeral_drops_sessions_and_claims_but_keeps_drawings() {
        let store = Store::open_in_memory().expect("memory store");
        store.ensure_drawing("PID-1101").expect("drawing");
        store
            .upsert_session(&SessionRow {
                session_id: "s-1".into(),
                drawing_id: "PID-1101".into(),
                user_id: "u1".into(),
                mode: "协同编辑".into(),
                status: "活跃".into(),
                base_revision: 0,
                last_heartbeat: 1_000,
                created_at: 1_000,
                workspace: "plant-a".into(),
            })
            .expect("session");
        store
            .insert_claim(&ClaimRow {
                tag: "V-1001".into(),
                drawing_id: "PID-1101".into(),
                session_id: "s-1".into(),
                user_id: "u1".into(),
                expires_at: 1_300,
            })
            .expect("claim");

        store.clear_ephemeral().expect("clear");

        assert!(store.session_row("s-1").expect("lookup").is_none());
        assert!(store
            .claims_on_drawing("PID-1101")
            .expect("claims")
            .is_empty());
        assert!(
            store.drawing("PID-1101").expect("lookup").is_some(),
            "drawings survive a restart"
        );
    }

    #[test]
    fn claims_are_unique_per_tag_and_replaceable() {
        let store = Store::open_in_memory().expect("memory store");
        store.ensure_drawing("PID-1101").expect("drawing");
        for session in ["s-1", "s-2"] {
            store
                .upsert_session(&SessionRow {
                    session_id: session.into(),
                    drawing_id: "PID-1101".into(),
                    user_id: "u1".into(),
                    mode: "协同编辑".into(),
                    status: "活跃".into(),
                    base_revision: 0,
                    last_heartbeat: 1_000,
                    created_at: 1_000,
                    workspace: String::new(),
                })
                .expect("session");
        }
        for session in ["s-1", "s-2"] {
            store
                .insert_claim(&ClaimRow {
                    tag: "V-1001".into(),
                    drawing_id: "PID-1101".into(),
                    session_id: session.into(),
                    user_id: "u1".into(),
                    expires_at: 1_300,
                })
                .expect("claim");
        }
        let claims = store.claims_on_drawing("PID-1101").expect("claims");
        assert_eq!(claims.len(), 1, "one row per tag");
        assert_eq!(claims[0].session_id, "s-2", "the later claim replaces");
    }

    #[test]
    fn deleting_a_session_claims_returns_the_tags() {
        let store = Store::open_in_memory().expect("memory store");
        store.ensure_drawing("PID-1101").expect("drawing");
        store
            .upsert_session(&SessionRow {
                session_id: "s-1".into(),
                drawing_id: "PID-1101".into(),
                user_id: "u1".into(),
                mode: "协同编辑".into(),
                status: "活跃".into(),
                base_revision: 0,
                last_heartbeat: 1_000,
                created_at: 1_000,
                workspace: String::new(),
            })
            .expect("session");
        for tag in ["V-1001", "V-1002"] {
            store
                .insert_claim(&ClaimRow {
                    tag: tag.into(),
                    drawing_id: "PID-1101".into(),
                    session_id: "s-1".into(),
                    user_id: "u1".into(),
                    expires_at: 1_300,
                })
                .expect("claim");
        }
        let tags = store.delete_claims_of_session("s-1").expect("delete");
        assert_eq!(tags, vec!["V-1001".to_owned(), "V-1002".to_owned()]);
        assert!(store.claims_on_drawing("PID-1101").expect("claims").is_empty());
    }

    #[test]
    fn conflicts_are_created_pending_and_resolved_once() {
        let store = Store::open_in_memory().expect("memory store");
        store
            .insert_conflict(&ConflictRow {
                conflict_id: "c-1".into(),
                drawing_id: "PID-1101".into(),
                object_ref: "V-1001".into(),
                field: "设计压力".into(),
                value_a: "1.6MPa".into(),
                value_b: "2.5MPa".into(),
                base_revision: 4,
                status: "待裁决".into(),
                resolution: None,
                resolved_value: None,
                resolved_by: None,
            })
            .expect("insert conflict");

        let pending = store
            .list_conflicts(Some("PID-1101"), Some("待裁决"))
            .expect("list");
        assert_eq!(pending.len(), 1);

        assert!(store
            .resolve_conflict("c-1", "采用甲方", None, "u-admin")
            .expect("resolve"));
        assert!(
            !store
                .resolve_conflict("c-1", "采用乙方", None, "u-admin")
                .expect("second resolve"),
            "a conflict is adjudicated exactly once"
        );

        let resolved = store
            .list_conflicts(None, Some("已裁决"))
            .expect("list resolved");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].resolution.as_deref(), Some("采用甲方"));
        assert_eq!(resolved[0].resolved_by.as_deref(), Some("u-admin"));
    }

    #[test]
    fn resolving_with_a_manual_value_stores_it() {
        let store = Store::open_in_memory().expect("memory store");
        store
            .insert_conflict(&ConflictRow {
                conflict_id: "c-2".into(),
                drawing_id: "PID-1101".into(),
                object_ref: "V-1002".into(),
                field: "公称直径".into(),
                value_a: "DN50".into(),
                value_b: "DN80".into(),
                base_revision: 2,
                status: "待裁决".into(),
                resolution: None,
                resolved_value: None,
                resolved_by: None,
            })
            .expect("insert");
        store
            .resolve_conflict("c-2", "人工改值", Some("DN65"), "u-admin")
            .expect("resolve");
        let rows = store.list_conflicts(None, None).expect("list");
        assert_eq!(rows[0].resolved_value.as_deref(), Some("DN65"));
    }

    #[test]
    fn ensure_drawing_registers_an_unknown_drawing_once() {
        let store = Store::open_in_memory().expect("memory store");
        store.ensure_drawing("PID-9999").expect("first");
        store.ensure_drawing("PID-9999").expect("second");
        let (node, state, _) = store
            .drawing("PID-9999")
            .expect("lookup")
            .expect("exists");
        assert_eq!(node, DEFAULT_PBS_NODE);
        assert_eq!(state, "协同编辑");
    }

    #[test]
    fn session_rows_round_trip_including_workspace() {
        let store = Store::open_in_memory().expect("memory store");
        store.ensure_drawing("PID-1101").expect("drawing");
        let row = SessionRow {
            session_id: "s-1".into(),
            drawing_id: "PID-1101".into(),
            user_id: "u1".into(),
            mode: "协同编辑".into(),
            status: "活跃".into(),
            base_revision: 3,
            last_heartbeat: 1_000,
            created_at: 900,
            workspace: "plant-a".into(),
        };
        store.upsert_session(&row).expect("insert");
        assert_eq!(store.session_row("s-1").expect("lookup"), Some(row));
    }

    /// `active_editing_sessions` is the authoritative input for the single-editor
    /// cap (appendix A.3 / `VAL-PERM-017`), and each of its four predicates fails
    /// silently when it is off by one character:
    ///   - `status = '活跃'` and not `失效` / `已接管`
    ///   - `mode != '只读'`
    ///   - the heartbeat window is `<=` and not `<` (a session exactly on the
    ///     timeout still counts as live)
    ///   - only the requested drawing
    #[test]
    fn active_editing_sessions_applies_status_mode_heartbeat_and_drawing_filters() {
        let store = Store::open_in_memory().expect("memory store");
        store.ensure_drawing("PID-A").expect("drawing A");
        store.ensure_drawing("PID-B").expect("drawing B");

        const TIMEOUT: i64 = 90;
        const NOW: i64 = 10_000;

        let row = |session_id: &str,
                   drawing_id: &str,
                   user_id: &str,
                   mode: &str,
                   status: &str,
                   last_heartbeat: i64,
                   created_at: i64| SessionRow {
            session_id: session_id.into(),
            drawing_id: drawing_id.into(),
            user_id: user_id.into(),
            mode: mode.into(),
            status: status.into(),
            base_revision: 0,
            last_heartbeat,
            created_at,
            workspace: String::new(),
        };

        // The only row that should come back: active / writable / heartbeat in
        // window / this drawing.
        store
            .upsert_session(&row(
                "s-live",
                "PID-A",
                "u1",
                "单人编辑",
                "活跃",
                NOW - 10,
                100,
            ))
            .expect("live");
        // `mode` is read-only — excluded.
        store
            .upsert_session(&row("s-readonly", "PID-A", "u2", "只读", "活跃", NOW, 200))
            .expect("readonly");
        // Status stale — excluded.
        store
            .upsert_session(&row("s-stale", "PID-A", "u3", "单人编辑", "失效", NOW, 300))
            .expect("stale");
        // Status taken over — excluded.
        store
            .upsert_session(&row(
                "s-taken",
                "PID-A",
                "u4",
                "单人编辑",
                "已接管",
                NOW,
                400,
            ))
            .expect("taken");
        // Heartbeat expired — excluded.
        store
            .upsert_session(&row(
                "s-expired",
                "PID-A",
                "u5",
                "单人编辑",
                "活跃",
                NOW - TIMEOUT - 1,
                500,
            ))
            .expect("expired");
        // Heartbeat exactly on the timeout — **must be included** (`<=`, not `<`).
        store
            .upsert_session(&row(
                "s-boundary",
                "PID-A",
                "u6",
                "单人编辑",
                "活跃",
                NOW - TIMEOUT,
                600,
            ))
            .expect("boundary");
        // A different drawing — excluded.
        store
            .upsert_session(&row("s-other", "PID-B", "u7", "单人编辑", "活跃", NOW, 700))
            .expect("other");

        let live = store
            .active_editing_sessions("PID-A", NOW, TIMEOUT)
            .expect("active sessions");

        assert_eq!(
            live,
            vec![
                ("s-live".to_owned(), "u1".to_owned()),
                ("s-boundary".to_owned(), "u6".to_owned()),
            ],
            "only active + writable + heartbeat-in-window (boundary included) + this drawing should come back, got {live:?}"
        );

        // `ORDER BY created_at, session_id` — moving `now` past the timeout for
        // both survivors empties the result instead of reordering it.
        assert!(
            store
                .active_editing_sessions("PID-A", NOW + TIMEOUT + 1, TIMEOUT)
                .expect("all expired")
                .is_empty(),
            "no session should stay active once `now` moves past the heartbeat timeout"
        );
    }

    /// The defect this guards: `SELECT 1` alone answers fine on a database that
    /// is readable but not writable, so `/health` used to return 200 while
    /// `POST /session` returned 500 on the same instance.
    ///
    /// Three states are probed, weakest to strongest:
    /// 1. `PRAGMA query_only = ON` — the connection answers reads and refuses
    ///    write transactions. The probe first checks that the pragma really does
    ///    refuse `BEGIN IMMEDIATE` on this build; only then is the hard
    ///    assertion made, so the test can never pass by asserting something that
    ///    was never true.
    /// 2. A `SQLITE_OPEN_READ_ONLY` handle — the real read-only open path.
    /// 3. The OS read-only attribute — whether Windows enforces it for an
    ///    already-created file varies, so here the assertion is the invariant
    ///    (`health_check` must agree with whether a real write succeeds), not a
    ///    fixed value. A healthy report over a failing write is the false
    ///    positive this test exists to catch.
    #[test]
    #[allow(clippy::permissions_set_readonly_false)]
    fn health_check_fails_on_a_readonly_database() {
        let path = temp_db_path("readonly");
        let writable = Store::open(&path).expect("open writable");
        assert!(
            writable.health_check(),
            "precondition: a writable database is healthy"
        );

        // (1) Deterministic read-only emulation. `query_only` is a connection
        // pragma: reads still work, write transactions are refused.
        {
            let conn = Connection::open(&path).expect("open second handle");
            conn.execute_batch("PRAGMA query_only = ON")
                .expect("query_only");
            // Verify the premise before relying on it.
            let write_blocked = conn.execute_batch("BEGIN IMMEDIATE").is_err();
            if !write_blocked {
                let _ = conn.execute_batch("ROLLBACK");
            }
            let query_only_store = Store {
                conn: Mutex::new(conn),
                path: path.clone(),
            };
            if write_blocked {
                assert!(
                    !query_only_store.health_check(),
                    "a query_only connection answers SELECT 1 but refuses writes, so it must not report healthy"
                );
            } else {
                // This build lets `BEGIN IMMEDIATE` through under `query_only`,
                // so the pragma does not model the defect here; cases (2) and
                // (3) carry it.
                println!("query_only did not block BEGIN IMMEDIATE on this build");
            }
        }

        // (2) A real read-only handle. The `-wal`/`-shm` sidecars still exist
        // because `writable` is open, which is what lets SQLite attach the
        // database read-only.
        match Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(conn) => {
                let readonly_store = Store {
                    conn: Mutex::new(conn),
                    path: path.clone(),
                };
                assert!(
                    !readonly_store.health_check(),
                    "a read-only handle must not report healthy"
                );
            }
            Err(err) => {
                // SQLite refused to attach the WAL database read-only (no
                // writable `-shm`). That is stricter than "not writable", so
                // there is nothing to assert — cases (1) and (3) cover it.
                println!("read-only handle unavailable on this platform: {err}");
            }
        }

        drop(writable);

        // (3) The OS attribute, asserted against the invariant.
        let mut perms = std::fs::metadata(&path).expect("metadata").permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms).expect("set readonly");
        match Store::open(&path) {
            Ok(store) => {
                // `audit_log` is append-only, so this INSERT is a legitimate
                // write and the only permission it needs is the one under test.
                let wrote = store
                    .audit_append(&audit_row("V-1001", 1_700_000_000, "ro-probe"))
                    .is_ok();
                let healthy = store.health_check();
                assert_eq!(
                    healthy, wrote,
                    "health_check must report exactly what a write can do \
                     (write succeeded: {wrote}); a healthy report over a failed \
                     write is the false positive this test catches"
                );
            }
            Err(err) => {
                // Opening a read-only file needs a writable WAL pragma, so a
                // failure here is expected and stricter than the defect.
                println!("read-only file refused at open on this platform: {err}");
            }
        }

        // Restore write permission before cleanup, or the delete below fails.
        let mut perms = std::fs::metadata(&path).expect("metadata").permissions();
        perms.set_readonly(false);
        let _ = std::fs::set_permissions(&path, perms);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(PathBuf::from(format!("{}-wal", path.display())));
        let _ = std::fs::remove_file(PathBuf::from(format!("{}-shm", path.display())));
    }

    #[test]
    fn health_check_passes_on_a_writable_database() {
        let path = temp_db_path("healthy");
        let store = Store::open(&path).expect("open store");
        assert!(
            store.health_check(),
            "a writable database with the schema applied is healthy"
        );
        // The probe leaves no transaction behind: a normal write still works.
        store
            .audit_append(&audit_row("V-1001", 1_700_000_000, "x"))
            .expect("append after health probe");
        assert!(store.health_check(), "repeated probes stay healthy");
        drop(store);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(PathBuf::from(format!("{}-wal", path.display())));
        let _ = std::fs::remove_file(PathBuf::from(format!("{}-shm", path.display())));
    }

    #[test]
    fn commit_op_rejects_a_stale_base_revision() {
        let store = Store::open_in_memory().expect("memory store");
        store.ensure_drawing("PID-1101").expect("drawing");

        let outcome = store
            .commit_op("PID-1101", 5, &audit_row("V-1001", 1_700_000_000, "x"))
            .expect("commit");

        assert_eq!(outcome, OpCommit::Stale { current: 0 });
        assert_eq!(
            store.audit_count().expect("count"),
            0,
            "a stale op must not leave an audit row behind"
        );
        let (_, _, revision) = store.drawing("PID-1101").expect("lookup").expect("exists");
        assert_eq!(revision, 0, "a stale op must not move the revision");
    }

    #[test]
    fn commit_op_advances_the_revision_and_writes_one_audit_row() {
        let store = Store::open_in_memory().expect("memory store");
        store.ensure_drawing("PID-1101").expect("drawing");

        let first = store
            .commit_op("PID-1101", 0, &audit_row("V-1001", 1_700_000_000, "A"))
            .expect("first commit");
        let second = store
            .commit_op("PID-1101", 1, &audit_row("V-1002", 1_700_000_100, "B"))
            .expect("second commit");

        assert_eq!(
            first,
            OpCommit::Applied {
                revision: 1,
                seq: 1
            }
        );
        assert_eq!(
            second,
            OpCommit::Applied {
                revision: 2,
                seq: 2
            }
        );
        assert_eq!(store.audit_count().expect("count"), 2);
        let (_, _, revision) = store.drawing("PID-1101").expect("lookup").expect("exists");
        assert_eq!(revision, 2);
        // Replaying the first base revision is now stale, and it reports the
        // revision the client has to rebase onto.
        assert_eq!(
            store
                .commit_op("PID-1101", 0, &audit_row("V-1003", 1_700_000_200, "C"))
                .expect("replay"),
            OpCommit::Stale { current: 2 }
        );
    }

    #[test]
    fn commit_op_serializes_concurrent_callers() {
        let store = std::sync::Arc::new(Store::open_in_memory().expect("memory store"));
        store.ensure_drawing("PID-1101").expect("drawing");

        let mut handles = Vec::new();
        for index in 0..8_i64 {
            let store = std::sync::Arc::clone(&store);
            handles.push(std::thread::spawn(move || {
                store
                    .commit_op(
                        "PID-1101",
                        0,
                        &audit_row("V-1001", 1_700_000_000 + index, "concurrent"),
                    )
                    .expect("commit")
            }));
        }

        let outcomes: Vec<OpCommit> = handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect();

        let mut applied_revisions = Vec::new();
        let mut stale_currents = Vec::new();
        for outcome in &outcomes {
            match outcome {
                OpCommit::Applied { revision, .. } => applied_revisions.push(*revision),
                OpCommit::Stale { current } => stale_currents.push(*current),
            }
        }

        assert_eq!(
            applied_revisions,
            vec![1],
            "exactly one caller may win, got {outcomes:?}"
        );
        assert_eq!(
            stale_currents,
            vec![1; 7],
            "the other seven must be stale against revision 1, got {outcomes:?}"
        );
        let (_, _, revision) = store.drawing("PID-1101").expect("lookup").expect("exists");
        assert_eq!(
            revision, 1,
            "eight racing ops may advance the revision exactly once"
        );
        let page = store
            .audit_query(&AuditQuery {
                object_ref: Some("V-1001".into()),
                ..AuditQuery::default()
            })
            .expect("query");
        assert_eq!(page.total, 1, "one op, one audit row");
    }
}
