//! Hourly encrypted SQLite backups to S3-compatible storage with retention.
//!
//! Mirrors `github.com/suhlig/sqlite-vault`: a consistent snapshot via
//! `VACUUM INTO`, age/scrypt encryption, deterministic slot-based object names
//! that implement retention purely by overwriting (no deletion step), a canary
//! row that prevents a restart from overwriting the current slot's backup, and
//! a `verify` subcommand that downloads, decrypts, and checks the latest backup.
//!
//! Backups run in-process: `main` spawns [`run_forever`], which takes a backup
//! immediately at startup and then once per hour.

mod encrypt;
mod naming;
mod store;

use std::env;
use std::fmt;
use std::io;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Utc};
use object_store::ObjectStore;
use rand::Rng;
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::Connection;

use crate::db::Db;

pub use naming::{latest_alias_name, object_name, slot_of, Slot};
pub use store::{build_store, init_rustls_crypto};

use encrypt::{decrypt, encrypt};
use store::{get, put};

/// Overall timeout for a single backup run (snapshot + encryption + upload).
const RUN_TIMEOUT: StdDuration = StdDuration::from_secs(120);

/// Clock interval between backup runs; the canary keeps one backup per slot.
const RUN_INTERVAL: StdDuration = StdDuration::from_secs(3600);

#[derive(Debug)]
pub enum BackupError {
    Config(String),
    Sqlx(sqlx::Error),
    Store(object_store::Error),
    Encrypt(age::EncryptError),
    Decrypt(age::DecryptError),
    Io(io::Error),
    Msg(String),
}

impl BackupError {
    fn config(message: impl Into<String>) -> Self {
        BackupError::Config(message.into())
    }

    fn msg(message: impl Into<String>) -> Self {
        BackupError::Msg(message.into())
    }
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackupError::Config(m) | BackupError::Msg(m) => f.write_str(m),
            BackupError::Sqlx(e) => write!(f, "database error: {e}"),
            BackupError::Store(e) => write!(f, "object store error: {e}"),
            BackupError::Encrypt(e) => write!(f, "encryption error: {e}"),
            BackupError::Decrypt(e) => write!(f, "decryption error: {e}"),
            BackupError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for BackupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BackupError::Sqlx(e) => Some(e),
            BackupError::Store(e) => Some(e),
            BackupError::Encrypt(e) => Some(e),
            BackupError::Decrypt(e) => Some(e),
            BackupError::Io(e) => Some(e),
            BackupError::Config(_) | BackupError::Msg(_) => None,
        }
    }
}

impl From<sqlx::Error> for BackupError {
    fn from(e: sqlx::Error) -> Self {
        BackupError::Sqlx(e)
    }
}

impl From<object_store::Error> for BackupError {
    fn from(e: object_store::Error) -> Self {
        BackupError::Store(e)
    }
}

impl From<io::Error> for BackupError {
    fn from(e: io::Error) -> Self {
        BackupError::Io(e)
    }
}

impl From<age::EncryptError> for BackupError {
    fn from(e: age::EncryptError) -> Self {
        BackupError::Encrypt(e)
    }
}

impl From<age::DecryptError> for BackupError {
    fn from(e: age::DecryptError) -> Self {
        BackupError::Decrypt(e)
    }
}

/// Backup configuration, read from the `KEHRKRAFT_BACKUP_*` environment.
#[derive(Debug, Clone)]
pub struct Config {
    pub bucket: String,
    pub region: String,
    /// S3-compatible endpoint, e.g. `https://s3.us-west-004.backblazeb2.com`.
    /// When unset, the AWS regional endpoint is used.
    pub endpoint: Option<String>,
    pub access_key: String,
    pub secret_key: String,
    pub passphrase: String,
    /// Object name prefix; defaults to `kehrkraft`.
    pub prefix: String,
    /// Maximum acceptable canary age in hours; used by `verify`.
    pub max_age_hours: u64,
}

impl Config {
    /// Read the backup configuration from the environment.
    ///
    /// Returns `Ok(None)` when backups are disabled (`KEHRKRAFT_BACKUP_BUCKET`
    /// unset). When the bucket is set, all credentials and the passphrase are
    /// required: backups are never stored unencrypted.
    pub fn from_env() -> Result<Option<Self>, BackupError> {
        let Some(bucket) = env_var("KEHRKRAFT_BACKUP_BUCKET") else {
            return Ok(None);
        };
        let access_key = required_var("KEHRKRAFT_BACKUP_ACCESS_KEY")?;
        let secret_key = required_var("KEHRKRAFT_BACKUP_SECRET_KEY")?;
        let passphrase = match env_var("KEHRKRAFT_BACKUP_PASSPHRASE") {
            Some(p) => p,
            None => {
                return Err(BackupError::config(
                    "KEHRKRAFT_BACKUP_PASSPHRASE must be set when backups are enabled; refusing to store unencrypted backups",
                ))
            }
        };
        let region = env_var("KEHRKRAFT_BACKUP_REGION").unwrap_or_else(|| "us-east-1".to_string());
        if region.is_empty() {
            return Err(BackupError::config(
                "KEHRKRAFT_BACKUP_REGION must not be empty",
            ));
        }
        let prefix = env_var("KEHRKRAFT_BACKUP_PREFIX").unwrap_or_else(|| "kehrkraft".to_string());
        if !is_valid_prefix(&prefix) {
            return Err(BackupError::config(format!(
                "invalid KEHRKRAFT_BACKUP_PREFIX {prefix:?}: use only ASCII letters, digits, '.', '-' and '_'"
            )));
        }
        let max_age_hours = match env_var("KEHRKRAFT_BACKUP_MAX_AGE_HOURS") {
            None => 26,
            Some(v) => v.parse::<u64>().map_err(|_| {
                BackupError::config(format!(
                    "invalid KEHRKRAFT_BACKUP_MAX_AGE_HOURS {v:?}: expected hours as a number"
                ))
            })?,
        };
        Ok(Some(Config {
            bucket,
            region,
            endpoint: env_var("KEHRKRAFT_BACKUP_ENDPOINT"),
            access_key,
            secret_key,
            passphrase,
            prefix,
            max_age_hours,
        }))
    }
}

fn env_var(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn required_var(name: &str) -> Result<String, BackupError> {
    env_var(name).ok_or_else(|| BackupError::config(format!("{name} must be set")))
}

fn is_valid_prefix(prefix: &str) -> bool {
    !prefix.is_empty()
        && prefix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// The outcome of a single backup run.
#[derive(Debug)]
pub enum RunOutcome {
    Stored { object_name: String },
    Skipped { slot: Slot },
}

/// Runs a single backup attempt at the given time.
pub struct Runner {
    store: Arc<dyn ObjectStore>,
    config: Config,
    pool: Db,
}

impl Runner {
    pub fn new(config: Config, pool: Db) -> Result<Self, BackupError> {
        Ok(Runner {
            store: build_store(&config)?,
            config,
            pool,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_store(config: Config, store: Arc<dyn ObjectStore>, pool: Db) -> Self {
        Runner {
            store,
            config,
            pool,
        }
    }

    /// Perform one backup run at `now`, returning what happened.
    pub async fn run_once(&self, now: DateTime<Utc>) -> Result<RunOutcome, BackupError> {
        match tokio::time::timeout(RUN_TIMEOUT, self.run_once_inner(now)).await {
            Ok(result) => result,
            Err(_) => Err(BackupError::msg(format!(
                "backup timed out after {RUN_TIMEOUT:?}"
            ))),
        }
    }

    async fn run_once_inner(&self, now: DateTime<Utc>) -> Result<RunOutcome, BackupError> {
        let mut conn = self.pool.acquire().await?;
        // Wait briefly if the DB is busy, instead of immediately failing.
        sqlx::query("PRAGMA busy_timeout=3000")
            .execute(&mut *conn)
            .await?;
        // WAL doesn't block concurrent writers during the snapshot. Harmless if
        // the database already runs in WAL mode (e.g. in-memory test DBs).
        sqlx::query("PRAGMA journal_mode=WAL")
            .fetch_optional(&mut *conn)
            .await?;

        // Skip when the current slot was already backed up (e.g. after a
        // restart within the same hour) so the existing backup is not
        // overwritten.
        if let Some(previous) = canary_backed_up_at(&mut conn).await? {
            if object_name(&self.config.prefix, previous) == object_name(&self.config.prefix, now) {
                return Ok(RunOutcome::Skipped { slot: slot_of(now) });
            }
        }

        write_canary(&mut conn, rand_hex(), now).await?;

        let temp_path = env::temp_dir().join(format!("{}-{}.db", self.config.prefix, rand_hex()));
        let quoted = temp_path
            .to_str()
            .ok_or_else(|| BackupError::msg("temporary path is not valid UTF-8"))?
            .replace('\'', "''");

        // VACUUM INTO is a plain SQL statement; sqlx cannot bind a path. The
        // path is fully controlled (temp dir + sanitized prefix + hex nonce),
        // so wrapping it in AssertSqlSafe is safe.
        let vacuum_sql = format!("VACUUM INTO '{quoted}'");
        let snapshot = sqlx::query(sqlx::AssertSqlSafe(vacuum_sql))
            .execute(&mut *conn)
            .await;
        drop(conn);
        if let Err(err) = snapshot {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(err.into());
        }

        let plaintext = match tokio::fs::read(&temp_path).await {
            Ok(bytes) => bytes,
            Err(err) => {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(err.into());
            }
        };
        let _ = tokio::fs::remove_file(&temp_path).await;

        // scrypt is deliberately CPU-heavy; run it off the async runtime.
        let passphrase = self.config.passphrase.clone();
        let ciphertext = tokio::task::spawn_blocking(move || encrypt(&plaintext, &passphrase))
            .await
            .map_err(|e| BackupError::msg(format!("encryption task failed: {e}")))??;

        let name = object_name(&self.config.prefix, now);
        put(self.store.as_ref(), &name, ciphertext).await?;
        let alias = latest_alias_name(&self.config.prefix, slot_of(now));
        put(self.store.as_ref(), &alias, name.as_bytes().to_vec()).await?;

        Ok(RunOutcome::Stored { object_name: name })
    }
}

/// Run backups forever: once immediately, then every hour. Intended to be
/// spawned as a tokio task alongside the HTTP server. Never returns.
pub async fn run_forever(runner: Runner) {
    loop {
        match runner.run_once(Utc::now()).await {
            Ok(RunOutcome::Stored { object_name }) => {
                tracing::info!(object_name, "backup stored")
            }
            Ok(RunOutcome::Skipped { slot }) => {
                tracing::info!(
                    slot = slot.label(),
                    "backup skipped: this slot is already covered"
                )
            }
            Err(err) => tracing::error!(error = %err, "backup failed"),
        }
        tokio::time::sleep(RUN_INTERVAL).await;
    }
}

/// Verify the most recent hourly backup: download the alias, decrypt the
/// backup it points to, run `PRAGMA integrity_check`, and check the canary is
/// fresh. Exit-code friendly for cron; see the `kehrkraft verify` subcommand.
pub async fn verify(
    config: &Config,
    store: &dyn ObjectStore,
    now: DateTime<Utc>,
) -> Result<(), BackupError> {
    let alias = latest_alias_name(&config.prefix, Slot::Hourly);
    let alias_bytes = get(store, &alias).await?;
    let object_name = String::from_utf8(alias_bytes)
        .map_err(|_| BackupError::msg(format!("alias {alias} is not valid UTF-8")))?
        .trim()
        .to_string();
    if object_name.is_empty() {
        return Err(BackupError::msg(format!("alias {alias} is empty")));
    }

    let ciphertext = get(store, &object_name).await?;
    let passphrase = config.passphrase.clone();
    let plaintext = tokio::task::spawn_blocking(move || decrypt(&ciphertext, &passphrase))
        .await
        .map_err(|e| BackupError::msg(format!("decryption task failed: {e}")))??;

    let path = env::temp_dir().join(format!("kehrkraft-verify-{}.db", rand_hex()));
    tokio::fs::write(&path, &plaintext).await?;
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true);
    let result = async {
        let mut conn = SqliteConnection::connect_with(&options).await?;
        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&mut conn)
            .await?;
        if integrity != "ok" {
            return Err(BackupError::msg(format!(
                "integrity check failed for {object_name}: {integrity}"
            )));
        }
        let backed_up_at: Option<String> =
            sqlx::query_scalar("SELECT backed_up_at FROM backup_canary WHERE id = 1")
                .fetch_optional(&mut conn)
                .await?;
        drop(conn);
        Ok::<_, BackupError>(backed_up_at)
    }
    .await;
    let _ = tokio::fs::remove_file(&path).await;
    let backed_up_at = result?;

    let Some(backed_up_at) = backed_up_at else {
        return Err(BackupError::msg(format!(
            "backup {object_name} has no canary row"
        )));
    };
    let timestamp = DateTime::parse_from_rfc3339(&backed_up_at)
        .map_err(|e| {
            BackupError::msg(format!(
                "cannot parse canary timestamp {backed_up_at:?}: {e}"
            ))
        })?
        .with_timezone(&Utc);
    let age = now
        .signed_duration_since(timestamp)
        .max(ChronoDuration::zero());
    if age > ChronoDuration::hours(config.max_age_hours as i64) {
        return Err(BackupError::msg(format!(
            "backup canary too old: {backed_up_at} (max age {}h)",
            config.max_age_hours
        )));
    }

    tracing::info!(object_name, backed_up_at, "backup verified");
    Ok(())
}

/// Read the canary's `backed_up_at`, if present. An unreadable timestamp is
/// logged and treated as "unknown": the protection is best-effort, and a
/// corrupt canary must not stop future backups forever.
async fn canary_backed_up_at(
    conn: &mut SqliteConnection,
) -> Result<Option<DateTime<Utc>>, BackupError> {
    let row: Option<String> =
        sqlx::query_scalar("SELECT backed_up_at FROM backup_canary WHERE id = 1")
            .fetch_optional(&mut *conn)
            .await?;
    let Some(raw) = row else {
        return Ok(None);
    };
    match DateTime::parse_from_rfc3339(&raw) {
        Ok(t) => Ok(Some(t.with_timezone(&Utc))),
        Err(e) => {
            tracing::warn!(canary = %raw, error = %e, "ignoring unreadable canary timestamp");
            Ok(None)
        }
    }
}

/// Upsert the canary row with a fresh `backed_up_at`, mirroring sqlite-vault's
/// schema so backups stay interchangeable with that tooling.
async fn write_canary(
    conn: &mut SqliteConnection,
    job_id: String,
    now: DateTime<Utc>,
) -> Result<(), BackupError> {
    let backed_up_at = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    sqlx::query(
        r#"
        INSERT INTO backup_canary (id, job_id, backed_up_at)
        VALUES (1, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            job_id = excluded.job_id,
            backed_up_at = excluded.backed_up_at
        "#,
    )
    .bind(job_id)
    .bind(backed_up_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Random lowercase-hex string for temp file names and canary job ids (16 bytes).
fn rand_hex() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use object_store::memory::InMemory;
    use sqlx::sqlite::SqlitePoolOptions;

    fn test_config(prefix: &str) -> Config {
        Config {
            bucket: "bucket".to_string(),
            region: "us-east-1".to_string(),
            endpoint: None,
            access_key: "access".to_string(),
            secret_key: "secret".to_string(),
            passphrase: "test-passphrase".to_string(),
            prefix: prefix.to_string(),
            max_age_hours: 26,
        }
    }

    /// A per-test, file-backed SQLite pool. Kehrkraft's DB is a file in
    /// production, and `VACUUM INTO` from a `:memory:` database silently
    /// produces nothing; tests must use a real file to exercise the snapshot
    /// path.
    struct TestDb {
        pool: Db,
        path: std::path::PathBuf,
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            let wal = format!("{}-wal", self.path.display());
            let shm = format!("{}-shm", self.path.display());
            let _ = std::fs::remove_file(&wal);
            let _ = std::fs::remove_file(&shm);
        }
    }

    async fn test_pool() -> TestDb {
        let path = std::env::temp_dir().join(format!("kehrkraft-test-{}.db", rand_hex()));
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("connect file db");
        db::migrate(&pool).await.expect("apply migrations");
        TestDb { pool, path }
    }

    fn test_store() -> Arc<dyn ObjectStore> {
        Arc::new(InMemory::new())
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[tokio::test]
    async fn stores_backup_object_and_alias() {
        let db = test_pool().await;
        let store = test_store();
        let runner = Runner::with_store(test_config("test"), store.clone(), db.pool.clone());
        let now = utc("2026-09-07T10:00:00Z");

        match runner.run_once(now).await.expect("backup succeeds") {
            RunOutcome::Stored { object_name } => {
                assert_eq!(object_name, "test.hourly-10.db.age")
            }
            other => panic!("expected Stored, got {other:?}"),
        }

        // The canary row in the source database was written.
        let backed_up_at: String =
            sqlx::query_scalar("SELECT backed_up_at FROM backup_canary WHERE id = 1")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(backed_up_at, "2026-09-07T10:00:00Z");

        // The alias points at the stored object.
        let alias = get(store.as_ref(), "test.hourly-latest.alias")
            .await
            .unwrap();
        assert_eq!(String::from_utf8(alias).unwrap(), "test.hourly-10.db.age");

        // The stored object decrypts to a valid SQLite database.
        let ciphertext = get(store.as_ref(), "test.hourly-10.db.age").await.unwrap();
        let plaintext = decrypt(&ciphertext, "test-passphrase").unwrap();
        assert_eq!(&plaintext[..16], b"SQLite format 3\x00");
    }

    #[tokio::test]
    async fn skips_already_covered_slot_then_backs_up_next_hour() {
        let db = test_pool().await;
        let store = test_store();
        let runner = Runner::with_store(test_config("test"), store, db.pool.clone());
        let now = utc("2026-09-07T10:00:00Z");

        runner.run_once(now).await.expect("first backup");

        match runner.run_once(now).await.expect("second run") {
            RunOutcome::Skipped { .. } => {}
            other => panic!("expected Skipped, got {other:?}"),
        }

        match runner
            .run_once(now + ChronoDuration::hours(1))
            .await
            .expect("next hour")
        {
            RunOutcome::Stored { object_name } => {
                assert_eq!(object_name, "test.hourly-11.db.age")
            }
            other => panic!("expected Stored, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn verify_accepts_fresh_backup() {
        let db = test_pool().await;
        let store = test_store();
        let runner = Runner::with_store(test_config("test"), store.clone(), db.pool.clone());
        let now = utc("2026-09-07T10:00:00Z");
        runner.run_once(now).await.expect("backup succeeds");

        verify(&test_config("test"), store.as_ref(), now)
            .await
            .expect("verify succeeds");
    }

    #[tokio::test]
    async fn verify_rejects_stale_backup() {
        let db = test_pool().await;
        let store = test_store();
        let runner = Runner::with_store(test_config("test"), store.clone(), db.pool.clone());
        // 48 h before the verification time; the after-canary max-age is 26 h.
        let backup_time = utc("2026-09-05T10:00:00Z");
        runner.run_once(backup_time).await.expect("backup succeeds");

        let err = verify(
            &test_config("test"),
            store.as_ref(),
            utc("2026-09-07T10:00:00Z"),
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("too old"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn backup_requires_passphrase_but_is_optional_without_bucket() {
        // These are the only tests touching the KEHRKRAFT_BACKUP_* variables,
        // so parallel execution within this crate cannot race here.
        std::env::remove_var("KEHRKRAFT_BACKUP_BUCKET");
        std::env::remove_var("KEHRKRAFT_BACKUP_ACCESS_KEY");
        std::env::remove_var("KEHRKRAFT_BACKUP_SECRET_KEY");
        std::env::remove_var("KEHRKRAFT_BACKUP_PASSPHRASE");
        assert!(Config::from_env().unwrap().is_none());

        std::env::set_var("KEHRKRAFT_BACKUP_BUCKET", "b");
        std::env::set_var("KEHRKRAFT_BACKUP_ACCESS_KEY", "ak");
        std::env::set_var("KEHRKRAFT_BACKUP_SECRET_KEY", "sk");
        std::env::remove_var("KEHRKRAFT_BACKUP_PASSPHRASE");
        let err = Config::from_env().unwrap_err();
        assert!(
            err.to_string().contains("unencrypted"),
            "unexpected error: {err}"
        );

        std::env::remove_var("KEHRKRAFT_BACKUP_BUCKET");
        std::env::remove_var("KEHRKRAFT_BACKUP_ACCESS_KEY");
        std::env::remove_var("KEHRKRAFT_BACKUP_SECRET_KEY");
    }
}
