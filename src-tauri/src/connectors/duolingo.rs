use chrono::NaiveDate;
use reqwest::Client;
use serde_json::Value;

use super::{get_text, service_url, Activity};

pub fn parse_duolingo(body: &str, username: &str) -> Result<Activity, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| "Duolingo returned something that wasn't a profile.".to_string())?;
    let users = value
        .get("users")
        .and_then(|users| users.as_array())
        .ok_or("Duolingo's profile response didn't include users.")?;
    let user = users
        .iter()
        .find(|user| {
            user.get("username")
                .and_then(|name| name.as_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(username))
        })
        .or_else(|| users.first())
        .ok_or_else(|| format!("No Duolingo profile named {username}."))?;

    let current = &user["streakData"]["currentStreak"];
    if current.is_null() {
        let streak = user.get("streak").and_then(|value| value.as_u64()).unwrap_or(0) as u32;
        if streak == 0 {
            return Ok(Activity {
                days: Vec::new(),
                reported_streak: Some(0),
                last_active: None,
                service_url: service_url("duolingo", username),
            });
        }
    }

    let length = current
        .get("length")
        .and_then(|value| value.as_u64())
        .or_else(|| user.get("streak").and_then(|value| value.as_u64()))
        .unwrap_or(0) as u32;
    let last_active = current.get("endDate").and_then(parse_duo_date);
    if length > 0 && last_active.is_none() {
        return Err("Duolingo didn't include the day this streak was last extended.".into());
    }
    Ok(Activity {
        days: Vec::new(),
        reported_streak: Some(length),
        last_active,
        service_url: service_url("duolingo", username),
    })
}

fn parse_duo_date(value: &Value) -> Option<NaiveDate> {
    if let Some(text) = value.as_str() {
        if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
            return Some(date);
        }
        if let Ok(ts) = text.parse::<i64>() {
            return crate::streak::date_from_unix(ts, chrono_tz::UTC);
        }
    }
    value
        .as_i64()
        .and_then(|ts| crate::streak::date_from_unix(ts, chrono_tz::UTC))
}

pub async fn check(client: &Client, username: &str) -> Result<Activity, String> {
    let url = format!(
        "https://www.duolingo.com/2017-06-30/users?username={}",
        urlencoding::encode(username)
    );
    let body = get_text(client, &url, true).await?;
    parse_duolingo(&body, username)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn reads_the_public_streak_end_date() {
        let body = r#"{"users":[{"username":"ada","streak":5,"streakData":{"currentStreak":{"length":5,"startDate":"2026-10-07","endDate":"2026-10-11"}}}]}"#;
        let got = parse_duolingo(body, "ada").unwrap();
        assert_eq!(got.reported_streak, Some(5));
        assert_eq!(
            got.last_active,
            Some(NaiveDate::from_ymd_opt(2026, 10, 11).unwrap())
        );
    }

    #[test]
    fn a_null_streak_is_idle() {
        let body = r#"{"users":[{"username":"ada","streak":0,"streakData":{"currentStreak":null}}]}"#;
        let got = parse_duolingo(body, "ada").unwrap();
        assert_eq!(got.reported_streak, Some(0));
        assert_eq!(got.last_active, None);
    }

    #[test]
    fn missing_user_is_an_error() {
        let err = parse_duolingo(r#"{"users":[]}"#, "ada").unwrap_err();
        assert!(err.contains("ada"));
    }
}
