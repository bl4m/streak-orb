use chrono::NaiveDate;
use chrono_tz::Tz;
use reqwest::Client;
use serde_json::Value;

use super::{service_url, Activity};
use crate::streak::{date_from_unix, days_between};

#[derive(Debug)]
pub struct StatusPage {
    pub days: Vec<NaiveDate>,
    pub count: usize,
    pub oldest: Option<NaiveDate>,
}

pub fn parse_codeforces_page(body: &str, tz: Tz) -> Result<StatusPage, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| "Codeforces returned something that wasn't JSON.".to_string())?;
    let status = value
        .get("status")
        .and_then(|status| status.as_str())
        .unwrap_or("");
    if status != "OK" {
        let comment = value
            .get("comment")
            .and_then(|comment| comment.as_str())
            .unwrap_or("Codeforces couldn't look up that handle.");
        if comment.to_ascii_lowercase().contains("not found") {
            return Err("No Codeforces user with that handle.".into());
        }
        return Err(comment.to_string());
    }
    let rows = value
        .get("result")
        .and_then(|result| result.as_array())
        .ok_or("Codeforces didn't return a submission list.")?;
    let mut days = Vec::new();
    let mut oldest = None;
    for row in rows {
        let Some(ts) = row.get("creationTimeSeconds").and_then(|ts| ts.as_i64()) else {
            continue;
        };
        let Some(date) = date_from_unix(ts, tz) else {
            continue;
        };
        oldest = Some(oldest.map_or(date, |prev: NaiveDate| prev.min(date)));
        if row.get("verdict").and_then(|verdict| verdict.as_str()) == Some("OK") {
            days.push(date);
        }
    }
    days.sort();
    days.dedup();
    Ok(StatusPage {
        days,
        count: rows.len(),
        oldest,
    })
}

pub async fn check(client: &Client, username: &str, tz: Tz) -> Result<Activity, String> {
    let mut days = Vec::new();
    let mut from = 1usize;
    let today = crate::streak::now_in(tz).date_naive();
    for _ in 0..6 {
        let url = format!(
            "https://codeforces.com/api/user.status?handle={}&from={from}&count=100",
            urlencoding::encode(username)
        );
        let response = client
            .get(&url)
            .header("User-Agent", "streak-orb")
            .send()
            .await
            .map_err(|_| "The Codeforces request didn't go through.".to_string())?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|_| "Codeforces' response couldn't be read.".to_string())?;
        if !status.is_success() && !body.contains("FAILED") {
            return Err(format!("Codeforces responded with {status}."));
        }
        let page = parse_codeforces_page(&body, tz)?;
        days.extend(page.days);
        let far_enough = page
            .oldest
            .is_some_and(|oldest| days_between(oldest, today) > 400);
        if page.count < 100 || far_enough {
            break;
        }
        from += 100;
    }
    days.sort();
    days.dedup();
    Ok(Activity {
        days,
        reported_streak: None,
        last_active: None,
        service_url: service_url("codeforces", username),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_accepted_submissions_count() {
        let body = r#"{
            "status": "OK",
            "result": [
                {"verdict": "OK", "creationTimeSeconds": 1760227200},
                {"verdict": "WRONG_ANSWER", "creationTimeSeconds": 1760313600}
            ]
        }"#;
        let page = parse_codeforces_page(body, chrono_tz::UTC).unwrap();
        assert_eq!(page.count, 2);
        assert_eq!(page.days.len(), 1);
        assert_eq!(
            page.days[0],
            date_from_unix(1760227200, chrono_tz::UTC).unwrap()
        );
    }

    #[test]
    fn a_missing_handle_is_an_error() {
        let body = r#"{"status":"FAILED","comment":"handle: User with handle ada not found"}"#;
        let err = parse_codeforces_page(body, chrono_tz::UTC).unwrap_err();
        assert!(err.contains("No Codeforces user"));
    }
}
