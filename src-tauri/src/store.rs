use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;

use crate::streak::{parse_clock, NoticeGate};

pub struct Store {
    conn: Connection,
}

#[derive(Clone, Debug)]
pub struct Account {
    pub id: String,
    pub connector_id: String,
    pub label: String,
    pub username: String,
    pub lead_hours: u32,
    pub enabled: bool,
    pub timezone: String,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub streak: u32,
    pub done_today: bool,
    pub status: String,
    pub detail: String,
    pub error: Option<String>,
    pub checked_at: Option<String>,
    pub service_url: Option<String>,
    pub last_active: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub quiet_start: Option<String>,
    pub quiet_end: Option<String>,
    pub launch_at_login: bool,
}

impl Settings {
    pub fn quiet_bounds(&self) -> Result<(Option<NaiveTime>, Option<NaiveTime>), String> {
        match (&self.quiet_start, &self.quiet_end) {
            (Some(start), Some(end)) if !start.is_empty() && !end.is_empty() => {
                Ok((Some(parse_clock(start)?), Some(parse_clock(end)?)))
            }
            _ => Ok((None, None)),
        }
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let conn = Connection::open(path).map_err(|err| err.to_string())?;
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS accounts (
                id TEXT PRIMARY KEY,
                connector_id TEXT NOT NULL,
                label TEXT NOT NULL,
                username TEXT NOT NULL,
                lead_hours INTEGER NOT NULL,
                enabled INTEGER NOT NULL,
                timezone TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS snapshots (
                account_id TEXT PRIMARY KEY,
                streak INTEGER NOT NULL,
                done_today INTEGER NOT NULL,
                status TEXT NOT NULL,
                detail TEXT NOT NULL,
                error TEXT,
                checked_at TEXT,
                service_url TEXT,
                last_active TEXT,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS reminders (
                account_id TEXT PRIMARY KEY,
                risk_dismissed_on TEXT,
                risk_shown_on TEXT,
                snooze_until TEXT,
                ended_notified_for TEXT,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                quiet_start TEXT,
                quiet_end TEXT,
                launch_at_login INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO settings (id, quiet_start, quiet_end, launch_at_login)
            VALUES (1, NULL, NULL, 1);
            ",
        )
        .map_err(|err| err.to_string())?;
        Ok(Self { conn })
    }

    pub fn list_accounts(&self) -> Result<Vec<Account>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, connector_id, label, username, lead_hours, enabled, timezone
                 FROM accounts ORDER BY created_at ASC",
            )
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Account {
                    id: row.get(0)?,
                    connector_id: row.get(1)?,
                    label: row.get(2)?,
                    username: row.get(3)?,
                    lead_hours: row.get::<_, i64>(4)? as u32,
                    enabled: row.get::<_, i64>(5)? != 0,
                    timezone: row.get(6)?,
                })
            })
            .map_err(|err| err.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.to_string())
    }

    pub fn get_account(&self, id: &str) -> Result<Option<Account>, String> {
        Ok(self
            .list_accounts()?
            .into_iter()
            .find(|account| account.id == id))
    }

    pub fn upsert_account(&self, account: &Account) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO accounts (id, connector_id, label, username, lead_hours, enabled, timezone, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                    connector_id = excluded.connector_id,
                    label = excluded.label,
                    username = excluded.username,
                    lead_hours = excluded.lead_hours,
                    enabled = excluded.enabled,
                    timezone = excluded.timezone",
                params![
                    account.id,
                    account.connector_id,
                    account.label,
                    account.username,
                    account.lead_hours,
                    account.enabled as i64,
                    account.timezone,
                    now,
                ],
            )
            .map_err(|err| err.to_string())?;
        self.conn
            .execute(
                "INSERT OR IGNORE INTO reminders (account_id) VALUES (?1)",
                [&account.id],
            )
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        let changed = self
            .conn
            .execute(
                "UPDATE accounts SET enabled = ?1 WHERE id = ?2",
                params![enabled as i64, id],
            )
            .map_err(|err| err.to_string())?;
        if changed == 0 {
            return Err("That streak is no longer saved.".into());
        }
        Ok(())
    }

    pub fn delete_account(&self, id: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM accounts WHERE id = ?1", [id])
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn snapshot(&self, id: &str) -> Result<Option<Snapshot>, String> {
        self.conn
            .query_row(
                "SELECT streak, done_today, status, detail, error, checked_at, service_url, last_active
                 FROM snapshots WHERE account_id = ?1",
                [id],
                |row| {
                    Ok(Snapshot {
                        streak: row.get::<_, i64>(0)? as u32,
                        done_today: row.get::<_, i64>(1)? != 0,
                        status: row.get(2)?,
                        detail: row.get(3)?,
                        error: row.get(4)?,
                        checked_at: row.get(5)?,
                        service_url: row.get(6)?,
                        last_active: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(|err| err.to_string())
    }

    pub fn save_snapshot(&self, id: &str, snapshot: &Snapshot) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO snapshots (
                    account_id, streak, done_today, status, detail, error, checked_at, service_url, last_active
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(account_id) DO UPDATE SET
                    streak = excluded.streak,
                    done_today = excluded.done_today,
                    status = excluded.status,
                    detail = excluded.detail,
                    error = excluded.error,
                    checked_at = excluded.checked_at,
                    service_url = excluded.service_url,
                    last_active = excluded.last_active",
                params![
                    id,
                    snapshot.streak,
                    snapshot.done_today as i64,
                    snapshot.status,
                    snapshot.detail,
                    snapshot.error,
                    snapshot.checked_at,
                    snapshot.service_url,
                    snapshot.last_active,
                ],
            )
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn gate(&self, id: &str) -> Result<NoticeGate, String> {
        let row = self
            .conn
            .query_row(
                "SELECT risk_dismissed_on, risk_shown_on, snooze_until, ended_notified_for
                 FROM reminders WHERE account_id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|err| err.to_string())?;
        let Some((dismissed, shown, snooze, ended)) = row else {
            return Ok(NoticeGate::default());
        };
        Ok(NoticeGate {
            risk_dismissed_on: parse_date(dismissed)?,
            risk_shown_on: parse_date(shown)?,
            snooze_until: parse_utc(snooze)?,
            ended_notified_for: parse_date(ended)?,
        })
    }

    pub fn save_gate(&self, id: &str, gate: &NoticeGate) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO reminders (account_id, risk_dismissed_on, risk_shown_on, snooze_until, ended_notified_for)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(account_id) DO UPDATE SET
                    risk_dismissed_on = excluded.risk_dismissed_on,
                    risk_shown_on = excluded.risk_shown_on,
                    snooze_until = excluded.snooze_until,
                    ended_notified_for = excluded.ended_notified_for",
                params![
                    id,
                    fmt_date(gate.risk_dismissed_on),
                    fmt_date(gate.risk_shown_on),
                    gate.snooze_until.map(|t| t.to_rfc3339()),
                    fmt_date(gate.ended_notified_for),
                ],
            )
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn settings(&self) -> Result<Settings, String> {
        self.conn
            .query_row(
                "SELECT quiet_start, quiet_end, launch_at_login FROM settings WHERE id = 1",
                [],
                |row| {
                    Ok(Settings {
                        quiet_start: row.get(0)?,
                        quiet_end: row.get(1)?,
                        launch_at_login: row.get::<_, i64>(2)? != 0,
                    })
                },
            )
            .map_err(|err| err.to_string())
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE settings SET quiet_start = ?1, quiet_end = ?2, launch_at_login = ?3 WHERE id = 1",
                params![
                    settings.quiet_start,
                    settings.quiet_end,
                    settings.launch_at_login as i64,
                ],
            )
            .map_err(|err| err.to_string())?;
        Ok(())
    }
}

fn fmt_date(day: Option<NaiveDate>) -> Option<String> {
    day.map(|d| d.format("%Y-%m-%d").to_string())
}

fn parse_date(value: Option<String>) -> Result<Option<NaiveDate>, String> {
    match value {
        Some(text) => NaiveDate::parse_from_str(&text, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| format!("Stored date \"{text}\" is unreadable.")),
        None => Ok(None),
    }
}

fn parse_utc(value: Option<String>) -> Result<Option<DateTime<Utc>>, String> {
    match value {
        Some(text) => DateTime::parse_from_rfc3339(&text)
            .map(|dt| Some(dt.with_timezone(&Utc)))
            .map_err(|_| format!("Stored time \"{text}\" is unreadable.")),
        None => Ok(None),
    }
}
