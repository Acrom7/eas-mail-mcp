use hmac::{Hmac, Mac as _};
use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};
use sha2::Sha256;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use crate::platform;
use crate::{AppError, ErrorCode, Result};

/// Durable operation state stored without mailbox content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStatus {
    /// Written before the Exchange request.
    Pending,
    /// Exchange confirmed success.
    Succeeded,
    /// Exchange safely rejected the request.
    Failed,
    /// Some confirmed steps succeeded before a later safe failure.
    Partial,
    /// The request may have reached Exchange.
    Unknown,
}

impl OperationStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Partial => "partial",
            Self::Unknown => "unknown",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "partial" => Ok(Self::Partial),
            "unknown" => Ok(Self::Unknown),
            _ => Err(storage_error()),
        }
    }
}

/// Content-free operation journal row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalRecord {
    /// Caller UUID.
    pub operation_id: String,
    /// Account ID.
    pub account_id: String,
    /// Mutation kind.
    pub kind: String,
    /// HMAC of the canonical payload.
    pub payload_hmac: String,
    /// Stable EAS ClientId.
    pub client_id: String,
    /// Durable state.
    pub status: OperationStatus,
    /// Content-free bit mask of confirmed Calendar lifecycle steps.
    pub completed_steps: u32,
}

/// Whether `begin` inserted a row or found the same prior operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalBegin {
    /// Durable row.
    pub record: JournalRecord,
    /// True only for the caller that inserted the pending row.
    pub inserted: bool,
}

/// I/O boundary for idempotent external mutations.
pub trait OperationJournal: Send + Sync {
    /// Returns one existing operation without changing its state.
    fn lookup(&self, operation_id: &str) -> Result<Option<JournalRecord>>;
    /// Inserts a pending row or returns an existing matching row.
    fn begin(&self, record: &JournalRecord) -> Result<JournalBegin>;
    /// Persists confirmed completed steps while an operation remains pending.
    fn checkpoint(&self, operation_id: &str, completed_steps: u32) -> Result<()>;
    /// Changes durable operation state.
    fn finish(
        &self,
        operation_id: &str,
        status: OperationStatus,
        completed_steps: u32,
    ) -> Result<()>;
    /// Removes terminal rows older than 90 days.
    fn prune(&self) -> Result<usize>;
    /// Removes all operation metadata for one remotely wiped account.
    fn purge_account(&self, account_id: &str) -> Result<usize>;
}

/// SQLite WAL journal safe for independent stdio processes.
pub struct SqliteJournal {
    connection: Mutex<Connection>,
}

impl SqliteJournal {
    /// Opens the journal and converts crash-left pending operations to unknown.
    pub fn open(path: &Path) -> Result<Self> {
        let connection = private_connection(path)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA synchronous=FULL;
                 CREATE TABLE IF NOT EXISTS operations (
                   operation_id TEXT PRIMARY KEY,
                   account_id TEXT NOT NULL,
                   kind TEXT NOT NULL,
                   payload_hmac TEXT NOT NULL,
                   client_id TEXT NOT NULL,
                   status TEXT NOT NULL,
                   completed_steps INTEGER NOT NULL DEFAULT 0,
                   created_at INTEGER NOT NULL,
                   updated_at INTEGER NOT NULL
                 );",
            )
            .map_err(|_| storage_error())?;
        ensure_completed_steps_column(&connection)?;
        connection
            .execute_batch(
                "UPDATE operations SET status='unknown', updated_at=unixepoch()
                 WHERE status='pending';",
            )
            .map_err(|_| storage_error())?;
        Ok(Self { connection: Mutex::new(connection) })
    }
}

pub(crate) fn with_storage_write_lock<T>(
    path: &Path,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let mut connection = private_connection(path)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| storage_error())?;
    let output = action()?;
    transaction.commit().map_err(|_| storage_error())?;
    Ok(output)
}

fn private_connection(path: &Path) -> Result<Connection> {
    let parent = path.parent().ok_or_else(storage_error)?;
    platform::ensure_private_directory(parent).map_err(|_| storage_error())?;
    create_private_file(path)?;
    let connection = Connection::open(path).map_err(|_| storage_error())?;
    connection.busy_timeout(Duration::from_secs(5)).map_err(|_| storage_error())?;
    Ok(connection)
}

fn create_private_file(path: &Path) -> Result<()> {
    platform::open_private_append(path).map(|_| ()).map_err(|_| storage_error())
}

impl OperationJournal for SqliteJournal {
    fn lookup(&self, operation_id: &str) -> Result<Option<JournalRecord>> {
        self.connection
            .lock()
            .map_err(|_| storage_error())?
            .query_row(
                "SELECT operation_id, account_id, kind, payload_hmac, client_id, status,
                        completed_steps
                 FROM operations WHERE operation_id=?1",
                [operation_id],
                row_to_record,
            )
            .optional()
            .map_err(|_| storage_error())
    }

    fn begin(&self, record: &JournalRecord) -> Result<JournalBegin> {
        let mut connection = self.connection.lock().map_err(|_| storage_error())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| storage_error())?;
        let existing = transaction
            .query_row(
                "SELECT operation_id, account_id, kind, payload_hmac, client_id, status,
                        completed_steps
                 FROM operations WHERE operation_id=?1",
                [&record.operation_id],
                row_to_record,
            )
            .optional()
            .map_err(|_| storage_error())?;
        if let Some(existing) = existing {
            transaction.commit().map_err(|_| storage_error())?;
            if existing.account_id != record.account_id
                || existing.kind != record.kind
                || existing.payload_hmac != record.payload_hmac
            {
                return Err(AppError::new(
                    ErrorCode::IdempotencyConflict,
                    "idempotency key was already used for different input",
                ));
            }
            return Ok(JournalBegin { record: existing, inserted: false });
        }
        transaction
            .execute(
                "INSERT INTO operations
                 (operation_id, account_id, kind, payload_hmac, client_id, status,
                  completed_steps, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, unixepoch(), unixepoch())",
                params![
                    record.operation_id,
                    record.account_id,
                    record.kind,
                    record.payload_hmac,
                    record.client_id,
                    record.completed_steps
                ],
            )
            .map_err(|_| storage_error())?;
        transaction.commit().map_err(|_| storage_error())?;
        Ok(JournalBegin { record: record.clone(), inserted: true })
    }

    fn checkpoint(&self, operation_id: &str, completed_steps: u32) -> Result<()> {
        let updated = self
            .connection
            .lock()
            .map_err(|_| storage_error())?
            .execute(
                "UPDATE operations SET completed_steps=?1, updated_at=unixepoch()
                 WHERE operation_id=?2 AND status='pending'",
                params![completed_steps, operation_id],
            )
            .map_err(|_| storage_error())?;
        if updated != 1 {
            return Err(storage_error());
        }
        Ok(())
    }

    fn finish(
        &self,
        operation_id: &str,
        status: OperationStatus,
        completed_steps: u32,
    ) -> Result<()> {
        if status == OperationStatus::Pending {
            return Err(AppError::new(
                ErrorCode::StorageError,
                "cannot finish an operation as pending",
            ));
        }
        let updated = self
            .connection
            .lock()
            .map_err(|_| storage_error())?
            .execute(
                "UPDATE operations SET status=?1, completed_steps=?2, updated_at=unixepoch()
                 WHERE operation_id=?3",
                params![status.as_str(), completed_steps, operation_id],
            )
            .map_err(|_| storage_error())?;
        if updated != 1 {
            return Err(storage_error());
        }
        Ok(())
    }

    fn prune(&self) -> Result<usize> {
        self.connection
            .lock()
            .map_err(|_| storage_error())?
            .execute(
                "DELETE FROM operations
                 WHERE status != 'pending' AND updated_at < unixepoch() - 7776000",
                [],
            )
            .map_err(|_| storage_error())
    }

    fn purge_account(&self, account_id: &str) -> Result<usize> {
        self.connection
            .lock()
            .map_err(|_| storage_error())?
            .execute("DELETE FROM operations WHERE account_id=?1", [account_id])
            .map_err(|_| storage_error())
    }
}

/// Computes a deterministic HMAC without storing the canonical payload.
pub fn payload_fingerprint(key: &[u8], payload: &[u8]) -> Result<String> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key)
        .map_err(|_| AppError::new(ErrorCode::StorageError, "invalid journal HMAC key"))?;
    mac.update(payload);
    Ok(mac.finalize().into_bytes().iter().map(|byte| format!("{byte:02x}")).collect())
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<JournalRecord> {
    let status: String = row.get(5)?;
    let status = OperationStatus::parse(&status).map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(JournalRecord {
        operation_id: row.get(0)?,
        account_id: row.get(1)?,
        kind: row.get(2)?,
        payload_hmac: row.get(3)?,
        client_id: row.get(4)?,
        status,
        completed_steps: row.get(6)?,
    })
}

fn ensure_completed_steps_column(connection: &Connection) -> Result<()> {
    let mut statement =
        connection.prepare("PRAGMA table_info(operations)").map_err(|_| storage_error())?;
    let columns =
        statement.query_map([], |row| row.get::<_, String>(1)).map_err(|_| storage_error())?;
    for column in columns {
        if column.map_err(|_| storage_error())? == "completed_steps" {
            return Ok(());
        }
    }
    connection
        .execute("ALTER TABLE operations ADD COLUMN completed_steps INTEGER NOT NULL DEFAULT 0", [])
        .map_err(|_| storage_error())?;
    Ok(())
}

fn storage_error() -> AppError {
    AppError::new(ErrorCode::StorageError, "operation journal is unavailable")
}

#[cfg(test)]
mod tests;
