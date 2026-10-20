// Reviews of a build: a while after a build was first started here (the
// survey the owner set in testers.json, account.rs), Mochi asks how it is —
// one to five stars, what to add, what to take away. A review is a comment
// of the signed-in GitHub account on the build's issue ("Reviews: build N",
// label `reviews`, opened by the all-in-one workflow) in Disa153/coucou; the
// owner and the beta testers read them all in Settings → Reviews.
//
// Each build is asked about once; "Later" asks again the next day. Nothing is
// sent without the click on Send, and nothing here runs until the user signs in.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;

const ISSUES: &str = "https://api.github.com/repos/Disa153/coucou/issues";
const LABEL: &str = "reviews";
const MARK: &str = "<!-- coucou-review:";
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const LIMIT: usize = 1024 * 1024;
/// A review's text fields, at most.
pub const TEXT_MAX: usize = 1000;
/// Builds shown in Settings → Reviews.
const BUILDS_SHOWN: usize = 10;

pub fn issue_title(build: u64) -> String {
    format!("Reviews: build {build}")
}

// ── A review as written in a comment ──────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    #[serde(default)]
    pub login: String,
    pub stars: u8,
    #[serde(default)]
    pub add: String,
    #[serde(default)]
    pub remove: String,
    /// Unix seconds (the comment's date).
    #[serde(default)]
    pub at: u64,
}

fn clip(s: &str) -> String {
    s.trim().chars().take(TEXT_MAX).collect()
}

/// The comment: a hidden line Coucou reads back, then the same for people on GitHub.
pub fn comment_body(stars: u8, add: &str, remove: &str) -> String {
    let stars = stars.clamp(1, 5);
    let data = json!({ "v": 1, "stars": stars, "add": clip(add), "remove": clip(remove) });
    // "--" would end the HTML comment: written as an escape, which JSON reads back as is.
    let data = data.to_string().replace("--", "-\\u002d");
    let quote = |s: &str| -> String {
        let s = clip(s);
        if s.is_empty() { "—".to_string() } else { s.lines().collect::<Vec<_>>().join("<br>") }
    };
    format!(
        "{MARK}{data} -->\n{} ({stars}/5)\n\n**➕** {}\n\n**➖** {}\n",
        "★".repeat(stars as usize) + &"☆".repeat(5 - stars as usize),
        quote(add),
        quote(remove)
    )
}

/// The review in a comment Coucou wrote; None for any other comment.
pub fn parse_comment(body: &str) -> Option<Review> {
    let start = body.find(MARK)? + MARK.len();
    let end = start + body[start..].find("-->")?;
    let v: Value = serde_json::from_str(body[start..end].trim()).ok()?;
    let stars = v["stars"].as_u64().filter(|n| (1..=5).contains(n))? as u8;
    Some(Review {
        login: String::new(),
        stars,
        add: clip(v["add"].as_str().unwrap_or("")),
        remove: clip(v["remove"].as_str().unwrap_or("")),
        at: 0,
    })
}

/// One review per person: their last one counts.
pub fn latest_per_person(mut reviews: Vec<Review>) -> Vec<Review> {
    reviews.sort_by_key(|r| std::cmp::Reverse(r.at));
    let mut seen: Vec<String> = Vec::new();
    reviews.retain(|r| {
        let key = r.login.to_ascii_lowercase();
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    reviews
}

// ── When to ask ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Asked {
    /// Unix ms the build was first started here.
    pub first: u64,
    /// Answered (or the user said not to ask about this one).
    pub done: bool,
    /// "Later": not before this (unix ms).
    pub later: u64,
}

/// The review is due: long enough since the build first started, not answered, not put off.
pub fn due(asked: &Asked, days: u32, now_ms: u64) -> bool {
    !asked.done && asked.first > 0 && now_ms >= asked.first + days as u64 * DAY_MS && now_ms >= asked.later
}

fn stored_path() -> PathBuf {
    crate::settings::local_dir().join("reviews.json")
}

static ASKED: Mutex<Option<BTreeMap<u64, Asked>>> = Mutex::new(None);

fn with_asked<T>(f: impl FnOnce(&mut BTreeMap<u64, Asked>) -> T) -> T {
    let mut guard = ASKED.lock().unwrap();
    let map = guard.get_or_insert_with(|| {
        std::fs::read_to_string(stored_path())
            .ok()
            .and_then(|t| serde_json::from_str::<BTreeMap<u64, Asked>>(&t).ok())
            .unwrap_or_default()
    });
    let out = f(map);
    // Only the last few builds matter.
    while map.len() > 20 {
        let first = *map.keys().next().unwrap();
        map.remove(&first);
    }
    if let Ok(text) = serde_json::to_string(&*map) {
        let path = stored_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
    out
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// At launch: the build running is noted the first time it starts.
pub fn setup() {
    let build = crate::updater::build();
    if build == 0 {
        return;
    }
    with_asked(|m| {
        let a = m.entry(build).or_default();
        if a.first == 0 {
            a.first = now_ms();
        }
    });
}

/// This account is asked: signed in, and a tester (or the owner) unless the owner asks everyone.
fn asked_here(app: &AppHandle) -> bool {
    if !crate::account::signed_in(app) {
        return false;
    }
    crate::account::survey(app).who == "everyone" || crate::account::beta_allowed(app)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Due {
    pub build: u64,
}

/// The build to ask about now, if any (the island asks every hour).
#[tauri::command]
pub fn review_due(app: AppHandle) -> Option<Due> {
    let build = crate::updater::build();
    if build == 0 || !asked_here(&app) {
        return None;
    }
    let days = crate::account::survey(&app).days;
    let now = now_ms();
    with_asked(|m| m.get(&build).is_some_and(|a| due(a, days, now))).then_some(Due { build })
}

#[tauri::command]
pub fn review_later(build: u64) {
    with_asked(|m| m.entry(build).or_default().later = now_ms() + DAY_MS);
}

#[tauri::command]
pub fn review_skip(build: u64) {
    with_asked(|m| m.entry(build).or_default().done = true);
}

// ── GitHub ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewError {
    NotSignedIn,
    /// The sign-in cannot write here (made before reviews, or the app is not installed on the repository).
    SignInAgain,
    /// Only the owner and the testers read them.
    NotAllowed,
    NoStars,
    /// An attachment over the size the chat takes, or too many of them.
    TooBig,
    Network,
}

pub(crate) async fn call(method: reqwest::Method, url: &str, token: &str, body: Option<Value>) -> Result<(u16, Vec<u8>), ReviewError> {
    let url = reqwest::Url::parse(url).map_err(|_| ReviewError::Network)?;
    let client = crate::updater::client(Duration::from_secs(30)).map_err(|_| ReviewError::Network)?;
    let mut req = client
        .request(method, url)
        .header("User-Agent", "Coucou")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .bearer_auth(token);
    if let Some(body) = body {
        req = req.json(&body);
    }
    let response = req.send().await.map_err(|_| ReviewError::Network)?;
    let status = response.status().as_u16();
    let bytes = crate::net::read_capped(response, LIMIT).await.map_err(|_| ReviewError::Network)?;
    Ok((status, bytes))
}

/// The build number in a review issue's title.
pub fn build_of(title: &str) -> Option<u64> {
    title.strip_prefix("Reviews: build ")?.trim().parse().ok()
}

/// The review issues, newest first: (build, issue number, its page). Found by
/// title: one opened from Coucou by a tester has no label (only the repository sets labels).
async fn issues(token: &str) -> Result<Vec<(u64, u64, String)>, ReviewError> {
    let url = format!("{ISSUES}?state=all&per_page=100&sort=created&direction=desc");
    let (status, body) = call(reqwest::Method::GET, &url, token, None).await?;
    if status != 200 {
        return Err(if status == 401 { ReviewError::SignInAgain } else { ReviewError::Network });
    }
    let v: Value = serde_json::from_slice(&body).map_err(|_| ReviewError::Network)?;
    let mut out: Vec<(u64, u64, String)> = v
        .as_array()
        .into_iter()
        .flatten()
        .filter(|i| i.get("pull_request").is_none())
        .filter_map(|i| {
            let build = build_of(i["title"].as_str()?)?;
            let number = i["number"].as_u64()?;
            let page = i["html_url"].as_str().filter(|u| u.starts_with("https://github.com/Disa153/coucou/")).unwrap_or("").to_string();
            Some((build, number, page))
        })
        .collect();
    // Newest build first; for a build opened twice, the oldest issue (the workflow's) wins.
    out.sort_by_key(|(b, n, _)| (std::cmp::Reverse(*b), *n));
    out.dedup_by_key(|(b, _, _)| *b);
    Ok(out)
}

/// The issue of `build`; opened here (without the label, which only the
/// repository can set) if the workflow did not.
async fn issue_for(token: &str, build: u64) -> Result<u64, ReviewError> {
    if let Some((_, n, _)) = issues(token).await?.into_iter().find(|(b, _, _)| *b == build) {
        return Ok(n);
    }
    let body = json!({
        "title": issue_title(build),
        "body": format!("Reviews of Coucou build {build}, sent from Coucou. One comment per review."),
        "labels": [LABEL],
    });
    let (status, created) = call(reqwest::Method::POST, ISSUES, token, Some(body)).await?;
    match status {
        201 => {}
        401 | 403 | 404 | 410 => return Err(ReviewError::SignInAgain),
        _ => return Err(ReviewError::Network),
    }
    let v: Value = serde_json::from_slice(&created).map_err(|_| ReviewError::Network)?;
    v["number"].as_u64().ok_or(ReviewError::Network)
}

#[tauri::command]
pub async fn review_send(app: AppHandle, build: u64, stars: u8, add: String, remove: String) -> Result<(), ReviewError> {
    if !(1..=5).contains(&stars) {
        return Err(ReviewError::NoStars);
    }
    let token = crate::account::user_token().ok_or(ReviewError::NotSignedIn)?;
    if !crate::account::signed_in(&app) {
        return Err(ReviewError::NotSignedIn);
    }
    let issue = issue_for(&token, build).await?;
    let body = json!({ "body": comment_body(stars, &add, &remove) });
    let (status, _) = call(reqwest::Method::POST, &format!("{ISSUES}/{issue}/comments"), &token, Some(body)).await?;
    match status {
        201 => {}
        401 | 403 | 404 => return Err(ReviewError::SignInAgain),
        _ => return Err(ReviewError::Network),
    }
    with_asked(|m| m.entry(build).or_default().done = true);
    crate::log::line(format!("reviews: a review of build {build} was sent"));
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildReviews {
    pub build: u64,
    pub page: String,
    pub reviews: Vec<Review>,
}

/// The reviews of the last builds (owner and testers).
#[tauri::command]
pub async fn review_list(app: AppHandle) -> Result<Vec<BuildReviews>, ReviewError> {
    let token = crate::account::user_token().ok_or(ReviewError::NotSignedIn)?;
    if !crate::account::beta_allowed(&app) {
        return Err(ReviewError::NotAllowed);
    }
    let mut out = Vec::new();
    for (build, number, page) in issues(&token).await?.into_iter().take(BUILDS_SHOWN) {
        let (status, body) =
            call(reqwest::Method::GET, &format!("{ISSUES}/{number}/comments?per_page=100"), &token, None).await?;
        if status != 200 {
            continue;
        }
        let v: Value = serde_json::from_slice(&body).map_err(|_| ReviewError::Network)?;
        let reviews: Vec<Review> = v
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                let mut r = parse_comment(c["body"].as_str()?)?;
                r.login = c["user"]["login"].as_str().unwrap_or("").to_string();
                r.at = c["created_at"].as_str().and_then(crate::updater::unix_from_iso).unwrap_or(0);
                Some(r)
            })
            .collect();
        out.push(BuildReviews { build, page, reviews: latest_per_person(reviews) });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_review_survives_the_trip_through_a_comment() {
        let body = comment_body(4, "  dark mode for the mixer  ", "the glow -- too bright --> at night");
        assert!(body.starts_with(MARK));
        let hidden = body.lines().next().unwrap();
        assert!(hidden.ends_with(" -->"));
        assert_eq!(hidden.matches("-->").count(), 1, "the user's text cannot close the hidden line: {hidden}");
        let r = parse_comment(&body).unwrap();
        assert_eq!(r.stars, 4);
        assert_eq!(r.add, "dark mode for the mixer");
        assert_eq!(r.remove, "the glow -- too bright --> at night");
        assert!(body.contains("★★★★☆ (4/5)"));
    }

    #[test]
    fn other_comments_and_odd_stars_are_not_reviews() {
        assert_eq!(parse_comment("Nice build!"), None);
        assert_eq!(parse_comment(&format!("{MARK}{{\"stars\":9}} -->")), None);
        assert_eq!(parse_comment(&format!("{MARK}not json -->")), None);
        // stars are kept within 1–5 when written
        assert_eq!(parse_comment(&comment_body(0, "", "")).unwrap().stars, 1);
        assert_eq!(parse_comment(&comment_body(9, "", "")).unwrap().stars, 5);
        let long = "x".repeat(5000);
        assert_eq!(parse_comment(&comment_body(3, &long, "")).unwrap().add.len(), TEXT_MAX);
    }

    #[test]
    fn each_person_counts_once_with_their_last_review() {
        let r = |login: &str, stars, at| Review { login: login.into(), stars, add: String::new(), remove: String::new(), at };
        let kept = latest_per_person(vec![r("anna", 2, 10), r("Sergey", 5, 30), r("ANNA", 4, 20)]);
        assert_eq!(kept, vec![r("Sergey", 5, 30), r("ANNA", 4, 20)]);
    }

    #[test]
    fn asked_once_a_build_after_the_days_set() {
        let a = Asked { first: 1_000, done: false, later: 0 };
        assert!(!due(&a, 7, 1_000 + 6 * DAY_MS));
        assert!(due(&a, 7, 1_000 + 7 * DAY_MS));
        assert!(!due(&Asked { done: true, ..a.clone() }, 7, 1_000 + 30 * DAY_MS));
        assert!(!due(&Asked { later: 1_000 + 9 * DAY_MS, ..a.clone() }, 7, 1_000 + 8 * DAY_MS));
        assert!(!due(&Asked::default(), 7, u64::MAX / 2), "a build never seen starting is not asked about");
    }

    #[test]
    fn the_issue_title_names_the_build() {
        assert_eq!(build_of(&issue_title(63)), Some(63));
        assert_eq!(build_of("Reviews: build x"), None);
        assert_eq!(build_of("Bug: build 63"), None);
    }
}
