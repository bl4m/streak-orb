//! Day-boundary and reminder rules. No network, so the tests can pin them down.

use chrono::{DateTime, Days, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};
use chrono_tz::Tz;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Waiting,
    AtRisk,
    Ended,
    Idle,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Waiting => "waiting",
            Status::AtRisk => "at_risk",
            Status::Ended => "ended",
            Status::Idle => "idle",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub status: Status,
    pub streak: u32,
    pub done_today: bool,
    pub last_active: Option<NaiveDate>,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoticeGate {
    pub risk_dismissed_on: Option<NaiveDate>,
    pub risk_shown_on: Option<NaiveDate>,
    pub snooze_until: Option<DateTime<Utc>>,
    pub ended_notified_for: Option<NaiveDate>,
}

impl Default for NoticeGate {
    fn default() -> Self {
        Self {
            risk_dismissed_on: None,
            risk_shown_on: None,
            snooze_until: None,
            ended_notified_for: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    AtRisk,
    Ended,
}

pub fn system_tz() -> Tz {
    let name = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string());
    name.parse().unwrap_or(Tz::UTC)
}

pub fn parse_tz(name: &str) -> Result<Tz, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Ok(system_tz());
    }
    trimmed.parse::<Tz>().map_err(|_| {
        format!("Unknown time zone \"{trimmed}\". Try UTC or America/New_York.")
    })
}

pub fn now_in(tz: Tz) -> DateTime<Tz> {
    Utc::now().with_timezone(&tz)
}

pub fn in_lead_window(now: DateTime<Tz>, lead_hours: u32) -> bool {
    if lead_hours == 0 {
        return false;
    }
    if lead_hours >= 24 {
        return true;
    }
    let start_minutes = (24 - lead_hours) * 60;
    let now_minutes = now.time().hour() * 60 + now.time().minute();
    now_minutes >= start_minutes
}

pub fn in_quiet(now: NaiveTime, start: NaiveTime, end: NaiveTime) -> bool {
    if start == end {
        return false;
    }
    if start < end {
        now >= start && now < end
    } else {
        now >= start || now < end
    }
}

pub fn minutes_until_midnight(now: DateTime<Tz>) -> i64 {
    let Some(tomorrow) = now.date_naive().checked_add_days(Days::new(1)) else {
        return 0;
    };
    let Some(midnight) = tomorrow.and_hms_opt(0, 0, 0) else {
        return 0;
    };
    let Some(end) = now.timezone().from_local_datetime(&midnight).earliest() else {
        return 0;
    };
    (end - now).num_minutes().max(0)
}

pub fn format_time_left(mins: i64) -> String {
    if mins < 1 {
        return "under a minute left".to_string();
    }
    if mins < 60 {
        return format!("{mins} min left");
    }
    let hours = (mins + 59) / 60;
    if hours == 1 {
        "1 hour left".to_string()
    } else {
        format!("{hours} hours left")
    }
}

pub fn resolve(
    days: &[NaiveDate],
    reported_streak: Option<u32>,
    explicit_last: Option<NaiveDate>,
    today: NaiveDate,
    in_lead: bool,
) -> Resolution {
    let mut days_owned = days.to_vec();
    days_owned.sort();
    days_owned.dedup();
    let last_active = explicit_last.or_else(|| days_owned.iter().copied().max());
    if let Some(reported) = reported_streak {
        return resolve_reported(reported, last_active, today, in_lead);
    }
    resolve_days(&days_owned, today, in_lead)
}

fn resolve_reported(
    reported: u32,
    last_active: Option<NaiveDate>,
    today: NaiveDate,
    in_lead: bool,
) -> Resolution {
    let yesterday = today.checked_sub_days(Days::new(1));
    let done_today = last_active == Some(today);
    let alive = match last_active {
        Some(day) => Some(day) == yesterday || day == today,
        None => reported > 0,
    };
    if !alive {
        let broke = last_active.is_some() && (reported > 0 || last_active.is_some());
        return Resolution {
            status: if broke { Status::Ended } else { Status::Idle },
            streak: 0,
            done_today: false,
            last_active,
            detail: if broke {
                ended_detail(last_active, today)
            } else {
                "No streak yet".to_string()
            },
        };
    }
    let status = if done_today {
        Status::Ok
    } else if in_lead {
        Status::AtRisk
    } else {
        Status::Waiting
    };
    Resolution {
        status,
        streak: reported,
        done_today,
        last_active,
        detail: if done_today {
            "Done today".to_string()
        } else {
            "Still open today".to_string()
        },
    }
}

fn resolve_days(days: &[NaiveDate], today: NaiveDate, in_lead: bool) -> Resolution {
    let yesterday = today.checked_sub_days(Days::new(1));
    let done_today = days.contains(&today);
    let start = if done_today {
        Some(today)
    } else if yesterday.is_some_and(|day| days.contains(&day)) {
        yesterday
    } else {
        None
    };
    let mut streak = 0u32;
    if let Some(mut cursor) = start {
        loop {
            if !days.contains(&cursor) {
                break;
            }
            streak = streak.saturating_add(1);
            match cursor.checked_sub_days(Days::new(1)) {
                Some(prev) => cursor = prev,
                None => break,
            }
        }
    }
    let last_active = days.iter().copied().max();
    let status = if streak > 0 && done_today {
        Status::Ok
    } else if streak > 0 && in_lead {
        Status::AtRisk
    } else if streak > 0 {
        Status::Waiting
    } else if last_active.is_some() {
        Status::Ended
    } else {
        Status::Idle
    };
    let detail = match status {
        Status::Ok => "Done today".to_string(),
        Status::Waiting | Status::AtRisk => "Still open today".to_string(),
        Status::Ended => ended_detail(last_active, today),
        Status::Idle => "No streak yet".to_string(),
    };
    Resolution {
        status,
        streak,
        done_today,
        last_active,
        detail,
    }
}

fn ended_detail(last_active: Option<NaiveDate>, today: NaiveDate) -> String {
    match last_active {
        Some(day) if !fresh_break(Some(day), today) => "No current streak".to_string(),
        Some(_) => "Streak ended".to_string(),
        None => "No streak yet".to_string(),
    }
}

pub fn fresh_break(last_active: Option<NaiveDate>, today: NaiveDate) -> bool {
    let Some(day) = last_active else {
        return false;
    };
    let age = today.signed_duration_since(day).num_days();
    (1..=7).contains(&age)
}

pub fn notice_for(
    resolution: &Resolution,
    today: NaiveDate,
    now_utc: DateTime<Utc>,
    quiet: bool,
    gate: &NoticeGate,
) -> Option<NoticeKind> {
    if quiet {
        return None;
    }
    match resolution.status {
        Status::AtRisk => {
            if gate.risk_dismissed_on == Some(today) {
                return None;
            }
            if let Some(until) = gate.snooze_until {
                if now_utc < until {
                    return None;
                }
                return Some(NoticeKind::AtRisk);
            }
            if gate.risk_shown_on == Some(today) {
                return None;
            }
            Some(NoticeKind::AtRisk)
        }
        Status::Ended => {
            let last = resolution.last_active?;
            if !fresh_break(Some(last), today) {
                return None;
            }
            if gate.ended_notified_for == Some(last) {
                return None;
            }
            Some(NoticeKind::Ended)
        }
        _ => None,
    }
}

pub fn mark_shown(gate: &mut NoticeGate, kind: NoticeKind, today: NaiveDate, last_active: Option<NaiveDate>) {
    match kind {
        NoticeKind::AtRisk => {
            gate.risk_shown_on = Some(today);
            gate.snooze_until = None;
        }
        NoticeKind::Ended => {
            gate.ended_notified_for = last_active;
            let _ = today;
        }
    }
}

pub fn reminder_message(resolution: &Resolution, kind: NoticeKind, mins_left: i64) -> String {
    match kind {
        NoticeKind::AtRisk => {
            let left = format_time_left(mins_left);
            if resolution.streak > 0 {
                format!("{}-day streak · {left}", resolution.streak)
            } else {
                format!("Today still counts · {left}")
            }
        }
        NoticeKind::Ended => match resolution.last_active {
            Some(day) => format!("Last day was {}", day.format("%b %-d")),
            None => "The daily run stopped.".to_string(),
        },
    }
}

pub fn date_from_unix(ts: i64, tz: Tz) -> Option<NaiveDate> {
    let secs = if ts.abs() > 10_000_000_000 { ts / 1000 } else { ts };
    tz.timestamp_opt(secs, 0).single().map(|dt| dt.date_naive())
}

pub fn parse_clock(value: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(value.trim(), "%H:%M")
        .map_err(|_| format!("Use a time like 22:00, not \"{value}\"."))
}

/// How loud the tray icon should be. Higher wins.
pub fn tray_rank(status: &str, fresh: bool) -> u8 {
    match status {
        "error" => 4,
        "at_risk" => 3,
        "ended" if fresh => 2,
        _ => 1,
    }
}

pub fn quiet_now(now: DateTime<Tz>, start: Option<NaiveTime>, end: Option<NaiveTime>) -> bool {
    match (start, end) {
        (Some(start), Some(end)) => in_quiet(now.time(), start, end),
        _ => false,
    }
}

pub fn days_between(from: NaiveDate, to: NaiveDate) -> i64 {
    to.signed_duration_since(from).num_days()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn at(h: u32, min: u32) -> DateTime<Tz> {
        chrono_tz::America::New_York
            .with_ymd_and_hms(2026, 10, 11, h, min, 0)
            .unwrap()
    }

    #[test]
    fn streak_includes_today() {
        let today = day(2026, 10, 11);
        let days = [day(2026, 10, 9), day(2026, 10, 10), day(2026, 10, 11)];
        let got = resolve(&days, None, None, today, true);
        assert_eq!(got.streak, 3);
        assert!(got.done_today);
        assert_eq!(got.status, Status::Ok);
    }

    #[test]
    fn streak_holds_through_yesterday_until_the_lead_window() {
        let today = day(2026, 10, 11);
        let days = [day(2026, 10, 9), day(2026, 10, 10)];
        let waiting = resolve(&days, None, None, today, false);
        assert_eq!(waiting.streak, 2);
        assert!(!waiting.done_today);
        assert_eq!(waiting.status, Status::Waiting);
        let due = resolve(&days, None, None, today, true);
        assert_eq!(due.status, Status::AtRisk);
    }

    #[test]
    fn a_gap_yesterday_ends_the_streak() {
        let today = day(2026, 10, 11);
        let days = [day(2026, 10, 8), day(2026, 10, 9)];
        let got = resolve(&days, None, None, today, true);
        assert_eq!(got.streak, 0);
        assert_eq!(got.status, Status::Ended);
        assert!(fresh_break(got.last_active, today));
    }

    #[test]
    fn an_old_gap_is_not_a_fresh_break() {
        let today = day(2026, 10, 11);
        let days = [day(2026, 9, 1)];
        let got = resolve(&days, None, None, today, true);
        assert_eq!(got.status, Status::Ended);
        assert_eq!(got.detail, "No current streak");
        assert!(!fresh_break(got.last_active, today));
    }

    #[test]
    fn no_days_is_idle() {
        let got = resolve(&[], None, None, day(2026, 10, 11), true);
        assert_eq!(got.status, Status::Idle);
        assert_eq!(got.streak, 0);
    }

    #[test]
    fn reported_streak_uses_the_service_end_date() {
        let today = day(2026, 10, 11);
        let done = resolve(&[], Some(3970), Some(today), today, true);
        assert_eq!(done.status, Status::Ok);
        assert_eq!(done.streak, 3970);

        let yesterday = day(2026, 10, 10);
        let due = resolve(&[], Some(12), Some(yesterday), today, true);
        assert_eq!(due.status, Status::AtRisk);
        assert_eq!(due.streak, 12);
        assert!(!due.done_today);

        let early = resolve(&[], Some(12), Some(yesterday), today, false);
        assert_eq!(early.status, Status::Waiting);

        let broke = resolve(&[], Some(12), Some(day(2026, 10, 9)), today, true);
        assert_eq!(broke.status, Status::Ended);
        assert_eq!(broke.streak, 0);
    }

    #[test]
    fn lead_window_starts_the_configured_hours_before_midnight() {
        assert!(!in_lead_window(at(20, 59), 3));
        assert!(in_lead_window(at(21, 0), 3));
        assert!(in_lead_window(at(8, 0), 24));
        assert!(!in_lead_window(at(23, 0), 0));
    }

    #[test]
    fn quiet_hours_can_cross_midnight() {
        let start = NaiveTime::from_hms_opt(22, 0, 0).unwrap();
        let end = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        assert!(in_quiet(NaiveTime::from_hms_opt(23, 0, 0).unwrap(), start, end));
        assert!(in_quiet(NaiveTime::from_hms_opt(7, 30, 0).unwrap(), start, end));
        assert!(!in_quiet(NaiveTime::from_hms_opt(12, 0, 0).unwrap(), start, end));
        assert!(!in_quiet(start, start, start));
    }

    #[test]
    fn time_left_rounds_up_to_the_hour() {
        assert_eq!(minutes_until_midnight(at(22, 0)), 120);
        assert_eq!(format_time_left(120), "2 hours left");
        assert_eq!(format_time_left(61), "2 hours left");
        assert_eq!(format_time_left(60), "1 hour left");
        assert_eq!(format_time_left(45), "45 min left");
        assert_eq!(format_time_left(0), "under a minute left");
    }

    #[test]
    fn at_risk_orb_shows_once_then_respects_snooze_and_dismiss() {
        let today = day(2026, 10, 11);
        let resolution = resolve(
            &[day(2026, 10, 10)],
            None,
            None,
            today,
            true,
        );
        assert_eq!(resolution.status, Status::AtRisk);
        let now = Utc.with_ymd_and_hms(2026, 10, 12, 1, 0, 0).unwrap();
        let mut gate = NoticeGate::default();
        assert_eq!(
            notice_for(&resolution, today, now, false, &gate),
            Some(NoticeKind::AtRisk)
        );
        mark_shown(&mut gate, NoticeKind::AtRisk, today, resolution.last_active);
        assert_eq!(notice_for(&resolution, today, now, false, &gate), None);

        gate.snooze_until = Some(now + Duration::minutes(30));
        assert_eq!(notice_for(&resolution, today, now, false, &gate), None);
        let later = now + Duration::minutes(31);
        assert_eq!(
            notice_for(&resolution, today, later, false, &gate),
            Some(NoticeKind::AtRisk)
        );

        gate.snooze_until = None;
        gate.risk_dismissed_on = Some(today);
        assert_eq!(notice_for(&resolution, today, later, false, &gate), None);
        assert_eq!(notice_for(&resolution, today, later, true, &NoticeGate::default()), None);
    }

    #[test]
    fn ended_notice_fires_once_for_a_fresh_break() {
        let today = day(2026, 10, 11);
        let resolution = resolve(&[day(2026, 10, 9)], None, None, today, true);
        let now = Utc.with_ymd_and_hms(2026, 10, 11, 18, 0, 0).unwrap();
        let mut gate = NoticeGate::default();
        assert_eq!(
            notice_for(&resolution, today, now, false, &gate),
            Some(NoticeKind::Ended)
        );
        mark_shown(&mut gate, NoticeKind::Ended, today, resolution.last_active);
        assert_eq!(notice_for(&resolution, today, now, false, &gate), None);

        let stale = resolve(&[day(2026, 1, 1)], None, None, today, true);
        assert_eq!(notice_for(&stale, today, now, false, &NoticeGate::default()), None);
    }

    #[test]
    fn reminder_copy_names_the_streak_and_the_time_left() {
        let today = day(2026, 10, 11);
        let due = resolve(&[day(2026, 10, 10), day(2026, 10, 9)], None, None, today, true);
        assert_eq!(
            reminder_message(&due, NoticeKind::AtRisk, 90),
            "2-day streak · 2 hours left"
        );
        let ended = resolve(&[day(2026, 10, 9)], None, None, today, true);
        assert_eq!(
            reminder_message(&ended, NoticeKind::Ended, 0),
            "Last day was Oct 9"
        );
    }

    #[test]
    fn tray_rank_puts_a_saveable_streak_above_an_old_miss() {
        assert!(tray_rank("at_risk", true) > tray_rank("ended", true));
        assert!(tray_rank("error", false) > tray_rank("at_risk", true));
        assert_eq!(tray_rank("ended", false), 1);
        assert_eq!(tray_rank("ok", false), 1);
    }
}
