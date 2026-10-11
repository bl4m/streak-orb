use chrono_tz::Tz;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;

use super::{service_url, Activity};
use crate::streak::date_from_unix;

pub fn parse_leetcode(body: &str, username: &str, tz: Tz) -> Result<Activity, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| "LeetCode returned something that wasn't JSON.".to_string())?;
    if let Some(errors) = value.get("errors").and_then(|errors| errors.as_array()) {
        let message = errors
            .first()
            .and_then(|err| err.get("message"))
            .and_then(|message| message.as_str())
            .unwrap_or("LeetCode rejected the request.");
        return Err(message.to_string());
    }
    let matched = &value["data"]["matchedUser"];
    if matched.is_null() {
        return Err(format!("No LeetCode user named {username}."));
    }
    let calendar = &matched["userCalendar"]["submissionCalendar"];
    let map = match calendar {
        Value::String(text) => serde_json::from_str::<Value>(text)
            .map_err(|_| "LeetCode's solve calendar couldn't be read.".to_string())?,
        other => other.clone(),
    };
    let mut days = Vec::new();
    if let Some(object) = map.as_object() {
        for (key, count) in object {
            let count = count.as_u64().unwrap_or(0);
            if count == 0 {
                continue;
            }
            let ts: i64 = key
                .parse()
                .map_err(|_| format!("LeetCode returned a day I couldn't read ({key})."))?;
            let Some(date) = date_from_unix(ts, tz) else {
                continue;
            };
            days.push(date);
        }
    }
    days.sort();
    days.dedup();
    Ok(Activity {
        days,
        reported_streak: None,
        last_active: None,
        service_url: service_url("leetcode", username),
    })
}

pub async fn check(client: &Client, username: &str, tz: Tz) -> Result<Activity, String> {
    let payload = Query {
        query: "query userCalendar($username: String!) { matchedUser(username: $username) { userCalendar { streak submissionCalendar } } }",
        variables: Vars { username },
    };
    let response = client
        .post("https://leetcode.com/graphql")
        .header("Content-Type", "application/json")
        .header("Referer", format!("https://leetcode.com/{username}/"))
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
        )
        .json(&payload)
        .send()
        .await
        .map_err(|_| "The LeetCode request didn't go through.".to_string())?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|_| "LeetCode's response couldn't be read.".to_string())?;
    if !status.is_success() {
        return Err(format!("LeetCode responded with {status}."));
    }
    parse_leetcode(&body, username, tz)
}

#[derive(Serialize)]
struct Query<'a> {
    query: &'a str,
    variables: Vars<'a>,
}

#[derive(Serialize)]
struct Vars<'a> {
    username: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_timestamps_become_utc_days() {
        let body = r#"{"data":{"matchedUser":{"userCalendar":{"streak":1,"submissionCalendar":"{\"1760227200\": 2, \"1760313600\": 0}"}}}}"#;
        let days = parse_leetcode(body, "ada", chrono_tz::UTC).unwrap().days;
        assert_eq!(days.len(), 1);
        let expected = date_from_unix(1760227200, chrono_tz::UTC).unwrap();
        assert_eq!(days[0], expected);
    }

    #[test]
    fn unknown_user_is_an_error() {
        let err = parse_leetcode(r#"{"data":{"matchedUser":null}}"#, "ada", chrono_tz::UTC).unwrap_err();
        assert!(err.contains("ada"));
    }
}
