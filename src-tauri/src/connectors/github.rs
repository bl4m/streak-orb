use chrono::{Duration, NaiveDate, Utc};
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;

use super::{get_text, service_url, Activity};

pub fn parse_contribution_html(html: &str) -> Result<Vec<NaiveDate>, String> {
    let mut days = Vec::new();
    let mut found = 0usize;
    for chunk in html.split("<td").skip(1) {
        let cell = chunk.split("</td>").next().unwrap_or(chunk);
        let cell = &cell[..cell.len().min(800)];
        let Some(date_str) = attr(cell, "data-date") else {
            continue;
        };
        let Some(level_str) = attr(cell, "data-level") else {
            continue;
        };
        found += 1;
        let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").map_err(|_| {
            format!("GitHub returned a date I couldn't read ({date_str}).")
        })?;
        let level: u32 = level_str.parse().unwrap_or(0);
        if level > 0 {
            days.push(date);
        }
    }
    if found == 0 {
        return Err("GitHub's page didn't include a contribution calendar.".into());
    }
    days.sort();
    days.dedup();
    Ok(days)
}

pub fn parse_contribution_graphql(body: &str) -> Result<Vec<NaiveDate>, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| "GitHub returned something that wasn't JSON.".to_string())?;
    if let Some(message) = value
        .get("message")
        .and_then(|message| message.as_str())
    {
        if message.to_ascii_lowercase().contains("bad credentials") {
            return Err("GitHub didn't accept that token.".into());
        }
        return Err(message.to_string());
    }
    if let Some(errors) = value.get("errors").and_then(|errors| errors.as_array()) {
        let message = errors
            .first()
            .and_then(|err| err.get("message"))
            .and_then(|message| message.as_str())
            .unwrap_or("GitHub rejected the request.");
        let lower = message.to_ascii_lowercase();
        if lower.contains("token") || lower.contains("credential") || lower.contains("auth") {
            return Err("GitHub didn't accept that token.".into());
        }
        return Err(message.to_string());
    }
    if value["data"]["user"].is_null() {
        return Err("No GitHub user with that name.".into());
    }
    let weeks = value["data"]["user"]["contributionsCollection"]["contributionCalendar"]["weeks"]
        .as_array()
        .ok_or("GitHub didn't return a contribution calendar. Check the username.")?;
    let mut days = Vec::new();
    for week in weeks {
        let Some(cells) = week.get("contributionDays").and_then(|days| days.as_array()) else {
            continue;
        };
        for cell in cells {
            let Some(date_str) = cell.get("date").and_then(|date| date.as_str()) else {
                continue;
            };
            let count = cell
                .get("contributionCount")
                .and_then(|count| count.as_u64())
                .unwrap_or(0);
            if count == 0 {
                continue;
            }
            let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").map_err(|_| {
                format!("GitHub returned a date I couldn't read ({date_str}).")
            })?;
            days.push(date);
        }
    }
    days.sort();
    days.dedup();
    Ok(days)
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

pub async fn check(client: &Client, username: &str, token: Option<&str>) -> Result<Activity, String> {
    let days = match token.map(str::trim).filter(|token| !token.is_empty()) {
        Some(token) => fetch_graphql(client, username, token).await?,
        None => fetch_html(client, username).await?,
    };
    Ok(Activity {
        days,
        reported_streak: None,
        last_active: None,
        service_url: service_url("github", username),
    })
}

async fn fetch_html(client: &Client, username: &str) -> Result<Vec<NaiveDate>, String> {
    let url = format!("https://github.com/users/{username}/contributions");
    let body = get_text(client, &url, true).await?;
    parse_contribution_html(&body)
}

async fn fetch_graphql(client: &Client, username: &str, token: &str) -> Result<Vec<NaiveDate>, String> {
    let to = Utc::now() + Duration::days(1);
    let from = Utc::now() - Duration::days(400);
    let payload = Gql {
        query: "query($login: String!, $from: DateTime!, $to: DateTime!) { user(login: $login) { contributionsCollection(from: $from, to: $to) { contributionCalendar { weeks { contributionDays { date contributionCount } } } } } }",
        variables: Vars {
            login: username,
            from: from.to_rfc3339(),
            to: to.to_rfc3339(),
        },
    };
    let response = client
        .post("https://api.github.com/graphql")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "streak-orb")
        .header("Accept", "application/vnd.github+json")
        .json(&payload)
        .send()
        .await
        .map_err(|_| "The GitHub request didn't go through.".to_string())?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|_| "GitHub's response couldn't be read.".to_string())?;
    if status.as_u16() == 401 {
        return Err("GitHub didn't accept that token.".into());
    }
    if !status.is_success() && !body.contains("errors") {
        return Err(format!("GitHub responded with {status}."));
    }
    parse_contribution_graphql(&body)
}

#[derive(Serialize)]
struct Gql<'a> {
    query: &'a str,
    variables: Vars<'a>,
}

#[derive(Serialize)]
struct Vars<'a> {
    login: &'a str,
    from: String,
    to: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_calendar_keeps_days_with_a_level() {
        let html = r#"
            <td data-date="2026-10-10" data-level="0" class="ContributionCalendar-day"></td>
            <td data-level="2" id="contribution-day-component-0-1" data-date="2026-10-11" class="ContributionCalendar-day"></td>
        "#;
        let days = parse_contribution_html(html).unwrap();
        assert_eq!(days, vec![NaiveDate::from_ymd_opt(2026, 10, 11).unwrap()]);
    }

    #[test]
    fn html_without_a_calendar_is_an_error() {
        assert!(parse_contribution_html("<html></html>").is_err());
    }

    #[test]
    fn graphql_calendar_uses_real_counts() {
        let body = r#"{"data":{"user":{"contributionsCollection":{"contributionCalendar":{"weeks":[{"contributionDays":[{"date":"2026-10-10","contributionCount":0},{"date":"2026-10-11","contributionCount":3}]}]}}}}}"#;
        let days = parse_contribution_graphql(body).unwrap();
        assert_eq!(days, vec![NaiveDate::from_ymd_opt(2026, 10, 11).unwrap()]);
    }

    #[test]
    fn a_bad_token_is_called_out() {
        let err = parse_contribution_graphql(r#"{"message":"Bad credentials"}"#).unwrap_err();
        assert!(err.contains("token"));
    }
}
