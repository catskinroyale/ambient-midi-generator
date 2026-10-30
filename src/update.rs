//! Checks GitHub for a newer release. Only the public "latest release" API is
//! contacted, and nothing about the user is sent.

use serde::Deserialize;

pub const REPO: &str = "catskinroyale/ambient-midi-generator";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateStatus {
    Checking,
    UpToDate,
    Available { version: String, url: String },
    Failed(String),
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
}

/// Ask GitHub for the latest release and compare it with this build.
pub fn check() -> UpdateStatus {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = ureq::get(&url)
        .set("User-Agent", &format!("ambient-midi-generator/{VERSION}"))
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .call();
    let body = match resp {
        Ok(r) => match r.into_string() {
            Ok(b) => b,
            Err(e) => return UpdateStatus::Failed(e.to_string()),
        },
        Err(ureq::Error::Status(404, _)) => return UpdateStatus::UpToDate, // no releases yet
        Err(e) => return UpdateStatus::Failed(short_error(&e.to_string())),
    };
    match serde_json::from_str::<Release>(&body) {
        Ok(r) => match (parse_version(&r.tag_name), parse_version(VERSION)) {
            (Some(latest), Some(ours)) if latest > ours => {
                UpdateStatus::Available { version: r.tag_name.trim_start_matches('v').to_string(), url: r.html_url }
            }
            (Some(_), Some(_)) => UpdateStatus::UpToDate,
            _ => UpdateStatus::Failed(format!("unrecognised version {}", r.tag_name)),
        },
        Err(e) => UpdateStatus::Failed(e.to_string()),
    }
}

fn short_error(e: &str) -> String {
    e.lines().next().unwrap_or(e).chars().take(120).collect()
}

/// "v1.2.3" or "1.2" → (1, 2, 3). Pre-release suffixes are ignored.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u32>());
    let major = it.next()?.ok()?;
    let minor = it.next().unwrap_or(Ok(0)).ok()?;
    let patch = it.next().unwrap_or(Ok(0)).ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("v2.0.1-beta"), Some((2, 0, 1)));
        assert!(parse_version("v0.10.0") > parse_version("v0.9.9"));
        assert_eq!(parse_version("latest"), None);
        assert!(parse_version(VERSION).is_some());
    }
}

#[cfg(test)]
mod live {
    /// Contacts GitHub, so it only runs on request: cargo test -- --ignored live_check
    #[test]
    #[ignore]
    fn live_check() {
        println!("{:?}", super::check());
    }
}
