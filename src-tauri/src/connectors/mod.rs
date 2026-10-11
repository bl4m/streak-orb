use chrono::NaiveDate;
use chrono_tz::Tz;
use reqwest::Client;
use serde::Serialize;

mod codeforces;
mod duolingo;
mod github;
mod leetcode;

const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

#[derive(Clone, Debug)]
pub struct Activity {
    pub days: Vec<NaiveDate>,
    pub reported_streak: Option<u32>,
    pub last_active: Option<NaiveDate>,
    pub service_url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldSpec {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub required: bool,
    pub placeholder: String,
    pub help: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorMeta {
    pub id: String,
    pub name: String,
    pub blurb: String,
    pub default_timezone: String,
    pub fields: Vec<FieldSpec>,
}

pub fn catalog() -> Vec<ConnectorMeta> {
    vec![
        ConnectorMeta {
            id: "duolingo".into(),
            name: "Duolingo".into(),
            blurb: "Public profile streak. Duolingo has no official streak API, so this can break if their page changes.".into(),
            default_timezone: String::new(),
            fields: vec![],
        },
        ConnectorMeta {
            id: "github".into(),
            name: "GitHub".into(),
            blurb: "Consecutive days with at least one contribution. A token is only needed for private contributions.".into(),
            default_timezone: String::new(),
            fields: vec![FieldSpec {
                key: "token".into(),
                label: "Personal access token".into(),
                kind: "password".into(),
                required: false,
                placeholder: "Leave blank for public contributions".into(),
                help: "Optional. Stored in the macOS Keychain, never in the app database. A token that can see your repositories also counts private contributions.".into(),
            }],
        },
        ConnectorMeta {
            id: "leetcode".into(),
            name: "LeetCode".into(),
            blurb: "Public solve calendar. LeetCode days roll over at midnight UTC, and the API is unofficial.".into(),
            default_timezone: "UTC".into(),
            fields: vec![],
        },
        ConnectorMeta {
            id: "codeforces".into(),
            name: "Codeforces".into(),
            blurb: "Consecutive days with an accepted submission. Codeforces has no built-in streak, so this one is counted here.".into(),
            default_timezone: String::new(),
            fields: vec![],
        },
    ]
}

pub fn find_meta(id: &str) -> Option<ConnectorMeta> {
    catalog().into_iter().find(|meta| meta.id == id)
}

pub fn service_url(connector_id: &str, username: &str) -> String {
    match connector_id {
        "duolingo" => format!("https://www.duolingo.com/profile/{username}"),
        "github" => format!("https://github.com/{username}"),
        "leetcode" => format!("https://leetcode.com/{username}"),
        "codeforces" => format!("https://codeforces.com/profile/{username}"),
        _ => String::new(),
    }
}

pub fn validate_username(username: &str) -> Result<String, String> {
    let username = username.trim();
    if username.is_empty() || username.len() > 39 {
        return Err("Enter the public username.".into());
    }
    if !username
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    {
        return Err("Use the public username, without spaces.".into());
    }
    Ok(username.to_string())
}

pub async fn check(
    client: &Client,
    connector_id: &str,
    username: &str,
    token: Option<&str>,
    tz: Tz,
) -> Result<Activity, String> {
    let username = validate_username(username)?;
    match connector_id {
        "duolingo" => duolingo::check(client, &username).await,
        "github" => github::check(client, &username, token).await,
        "leetcode" => leetcode::check(client, &username, tz).await,
        "codeforces" => codeforces::check(client, &username, tz).await,
        _ => Err("That connector isn't available.".into()),
    }
}

pub(crate) async fn get_text(client: &Client, url: &str, browser: bool) -> Result<String, String> {
    let mut request = client.get(url);
    if browser {
        request = request.header("User-Agent", BROWSER_UA);
    }
    let response = request
        .send()
        .await
        .map_err(|_| "The network request didn't go through.".to_string())?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|_| "The response couldn't be read.".to_string())?;
    if !status.is_success() {
        return Err(format!("The site responded with {status}."));
    }
    Ok(body)
}
