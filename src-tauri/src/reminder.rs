use chrono::{TimeZone, Utc};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, time::Duration};
use tauri::{
    async_runtime::{self, spawn_blocking},
    AppHandle, Emitter, Manager,
};
use tokio::time::sleep;
use uuid::Uuid;

pub const REMINDER_FIRED_EVENT: &str = "reminder:fired";
pub const REMINDER_UPDATED_EVENT: &str = "reminder:updated";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderRecord {
    pub id: String,
    pub title: String,
    pub message: Option<String>,
    pub remind_at: i64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReminderInput {
    pub title: String,
    pub message: Option<String>,
    pub remind_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderIdPayload {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReminderSnoozePayload {
    pub id: String,
    pub remind_at: i64,
}

pub fn init_database(app: &AppHandle) -> Result<(), String> {
    let path = database_path(app)?;
    let conn = Connection::open(path).map_err(|err| err.to_string())?;
    apply_migrations(&conn).map_err(|err| err.to_string())
}

pub fn start_scheduler(app: AppHandle) {
    async_runtime::spawn(async move {
        loop {
            if let Err(err) = poll_due_reminders(&app).await {
                eprintln!("[reminder] 调度失败: {err}");
            }
            sleep(Duration::from_secs(30)).await;
        }
    });
}

pub async fn create_reminder(app: AppHandle, input: ReminderInput) -> Result<ReminderRecord, String> {
    let db_path = database_path(&app)?;
    let record = spawn_blocking(move || -> Result<ReminderRecord, String> {
        let conn = Connection::open(db_path).map_err(|err| err.to_string())?;
        apply_migrations(&conn).map_err(|err| err.to_string())?;
        let now = current_ts();
        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO reminders (id, title, message, remind_at, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?5)",
            params![id, input.title, input.message, input.remind_at, now],
        )
        .map_err(|err| err.to_string())?;

        Ok(ReminderRecord {
            id,
            title: input.title,
            message: input.message,
            remind_at: input.remind_at,
            status: "pending".into(),
            created_at: now,
            updated_at: now,
        })
    })
    .await
    .map_err(|err| err.to_string())??;

    let _ = app.emit(REMINDER_UPDATED_EVENT, &record);
    Ok(record)
}

pub async fn list_reminders(app: AppHandle) -> Result<Vec<ReminderRecord>, String> {
    let db_path = database_path(&app)?;
    spawn_blocking(move || -> Result<Vec<ReminderRecord>, String> {
        let conn = Connection::open(db_path).map_err(|err| err.to_string())?;
        apply_migrations(&conn).map_err(|err| err.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, title, message, remind_at, status, created_at, updated_at
                 FROM reminders
                 WHERE status != 'deleted'
                 ORDER BY remind_at ASC",
            )
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], |row| to_record(row))
            .map_err(|err| err.to_string())?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(|err| err.to_string())?);
        }
        Ok(records)
    })
    .await
    .map_err(|err| err.to_string())?
}

pub async fn complete_reminder(app: AppHandle, payload: ReminderIdPayload) -> Result<bool, String> {
    update_status(app, &payload.id, "completed").await
}

pub async fn delete_reminder(app: AppHandle, payload: ReminderIdPayload) -> Result<bool, String> {
    update_status(app, &payload.id, "deleted").await
}

pub async fn snooze_reminder(
    app: AppHandle,
    payload: ReminderSnoozePayload,
) -> Result<ReminderRecord, String> {
    let db_path = database_path(&app)?;
    let record = spawn_blocking(move || -> Result<ReminderRecord, String> {
        let conn = Connection::open(db_path).map_err(|err| err.to_string())?;
        apply_migrations(&conn).map_err(|err| err.to_string())?;
        let now = current_ts();
        conn.execute(
            "UPDATE reminders SET remind_at = ?1, status = 'pending', updated_at = ?2 WHERE id = ?3",
            params![payload.remind_at, now, payload.id],
        )
        .map_err(|err| err.to_string())?;
        fetch_by_id(&conn, &payload.id)
    })
    .await
    .map_err(|err| err.to_string())??;

    let _ = app.emit(REMINDER_UPDATED_EVENT, &record);
    Ok(record)
}

async fn update_status(app: AppHandle, id: &str, status: &str) -> Result<bool, String> {
    let db_path = database_path(&app)?;
    let status = status.to_string();
    let id_value = id.to_string();
    let emit_id = id_value.clone();
    let updated = spawn_blocking(move || -> Result<bool, String> {
        let conn = Connection::open(db_path).map_err(|err| err.to_string())?;
        apply_migrations(&conn).map_err(|err| err.to_string())?;
        let now = current_ts();
        let rows = conn
            .execute(
                "UPDATE reminders SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![status, now, id_value],
            )
            .map_err(|err| err.to_string())?;
        Ok(rows > 0)
    })
    .await
    .map_err(|err| err.to_string())??;

    if updated {
        let _ = app.emit(
            REMINDER_UPDATED_EVENT,
            &ReminderIdPayload { id: emit_id },
        );
    }
    Ok(updated)
}

fn to_record(row: &Row) -> rusqlite::Result<ReminderRecord> {
    Ok(ReminderRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        message: row.get(2)?,
        remind_at: row.get(3)?,
        status: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub async fn poll_due_reminders(app: &AppHandle) -> Result<(), String> {
    let db_path = database_path(app)?;
    let fired = spawn_blocking(move || -> Result<Vec<ReminderRecord>, String> {
        let conn = Connection::open(db_path).map_err(|err| err.to_string())?;
        apply_migrations(&conn).map_err(|err| err.to_string())?;
        let now = current_ts();
        let mut stmt = conn
            .prepare(
                "SELECT id, title, message, remind_at, status, created_at, updated_at
                 FROM reminders
                 WHERE status = 'pending' AND remind_at <= ?1
                 ORDER BY remind_at ASC",
            )
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map(params![now], |row| to_record(row))
            .map_err(|err| err.to_string())?;
        let mut due = Vec::new();
        for row in rows {
            due.push(row.map_err(|err| err.to_string())?);
        }
        for reminder in &due {
            conn.execute(
                "UPDATE reminders SET status = 'notified', updated_at = ?1 WHERE id = ?2",
                params![now, reminder.id],
            )
            .map_err(|err| err.to_string())?;
        }
        Ok(due)
    })
    .await
    .map_err(|err| err.to_string())??;

    if fired.is_empty() {
        return Ok(());
    }

    for reminder in fired {
        let _ = app.emit(REMINDER_FIRED_EVENT, &reminder);
    }
    Ok(())
}

fn database_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut dir = app
        .path()
        .app_local_data_dir()
        .map_err(|err| err.to_string())?;
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    dir.push("reminders.db");
    Ok(dir)
}

fn apply_migrations(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS reminders (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            message TEXT,
            remind_at INTEGER NOT NULL,
            status TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_reminders_remind_at ON reminders (remind_at);
        ",
    )
}

fn fetch_by_id(conn: &Connection, id: &str) -> Result<ReminderRecord, String> {
    conn.query_row(
        "SELECT id, title, message, remind_at, status, created_at, updated_at FROM reminders WHERE id = ?1",
        params![id],
        |row| to_record(row),
    )
    .map_err(|err| err.to_string())
}

fn current_ts() -> i64 {
    Utc::now().timestamp_millis()
}

#[allow(dead_code)]
pub fn format_timestamp(millis: i64) -> String {
    let dt = Utc.timestamp_millis_opt(millis).single().unwrap_or_else(|| Utc::now());
    dt.to_rfc3339()
}
