//! Shared HTTP helpers: GitHub-API authentication used by the latest-version
//! and artifact-resolver paths so unauthenticated CI runners do not blow the
//! 60-requests-per-hour shared rate limit.

use std::env;

use reqwest::blocking::RequestBuilder;

const GITHUB_API_HOST: &str = "api.github.com";

/// If the request targets `api.github.com` and a `GITHUB_TOKEN` is exported
/// (the standard CI secret), attach `Authorization: Bearer <token>` to the
/// request so we get the 5000-requests-per-hour authenticated quota instead
/// of the unauthenticated 60-per-hour cap.
pub(crate) fn maybe_apply_github_auth(builder: RequestBuilder, url: &str) -> RequestBuilder {
    if !is_github_api_url(url) {
        return builder;
    }
    let Ok(token) = env::var("GITHUB_TOKEN") else {
        return builder;
    };
    if token.trim().is_empty() {
        return builder;
    }
    builder.header("Authorization", format!("Bearer {}", token.trim()))
}

fn is_github_api_url(url: &str) -> bool {
    url.starts_with("https://api.github.com/")
        || url.starts_with(&format!("https://{GITHUB_API_HOST}/"))
}

#[cfg(test)]
mod tests {
    use super::is_github_api_url;

    #[test]
    fn detects_github_api_urls() {
        assert!(is_github_api_url(
            "https://api.github.com/repos/cfillion/reapack/releases/latest"
        ));
        assert!(!is_github_api_url(
            "https://github.com/jcsteh/osara/releases/download/snapshots/osara_2026.zip"
        ));
        assert!(!is_github_api_url("https://www.reaper.fm/download.php"));
    }
}
