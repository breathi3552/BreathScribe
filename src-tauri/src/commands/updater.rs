use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

use crate::network::NetworkManager;
use crate::settings;

const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/breathi3552/BreathScribe/releases/latest";

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GithubAsset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GithubRelease {
    pub tag_name: String,
    pub html_url: String,
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UpdateCheckResponse {
    UpToDate {
        current_version: String,
    },
    UpdateAvailable {
        current_version: String,
        latest_version: String,
        release_url: String,
        release_notes: Option<String>,
        download_url: Option<String>,
    },
    Error {
        message: String,
    },
}

/// Normalizes a version tag string by stripping leading 'v' or 'V' and trimming whitespace.
pub(crate) fn normalize_version_tag(tag: &str) -> &str {
    let trimmed = tag.trim();
    trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed)
}

/// Parses a version string into a `semver::Version`, falling back to 3-component numeric extraction.
pub(crate) fn parse_version(raw: &str) -> Result<semver::Version, String> {
    let normalized = normalize_version_tag(raw);
    if let Ok(version) = semver::Version::parse(normalized) {
        return Ok(version);
    }

    // Fallback: extract the first 3 dot-separated integers (e.g. 0.1.1-build -> 0.1.1)
    let parts: Vec<&str> = normalized.split('.').collect();
    if parts.len() >= 3 {
        let major = parts[0]
            .parse::<u64>()
            .map_err(|e| format!("Invalid major version '{}': {}", parts[0], e))?;
        let minor = parts[1]
            .parse::<u64>()
            .map_err(|e| format!("Invalid minor version '{}': {}", parts[1], e))?;
        let patch_part = parts[2]
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap_or(parts[2]);
        let patch = patch_part
            .parse::<u64>()
            .map_err(|e| format!("Invalid patch version '{}': {}", patch_part, e))?;
        return Ok(semver::Version::new(major, minor, patch));
    }

    Err(format!("Could not parse version '{}'", raw))
}

/// Selects the best download asset for the current OS platform.
pub(crate) fn pick_best_download_url(assets: &[GithubAsset]) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        // 1. Prefer setup executable (*-setup.exe or *.exe)
        if let Some(asset) = assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with("-setup.exe"))
        {
            return Some(asset.browser_download_url.clone());
        }
        if let Some(asset) = assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(".exe"))
        {
            return Some(asset.browser_download_url.clone());
        }
        // 2. Fall back to MSI installer
        if let Some(asset) = assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(".msi"))
        {
            return Some(asset.browser_download_url.clone());
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(asset) = assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(".dmg"))
        {
            return Some(asset.browser_download_url.clone());
        }
        if let Some(asset) = assets.iter().find(|a| {
            a.name.to_lowercase().ends_with(".tar.gz") || a.name.to_lowercase().ends_with(".zip")
        }) {
            return Some(asset.browser_download_url.clone());
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(asset) = assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(".appimage"))
        {
            return Some(asset.browser_download_url.clone());
        }
        if let Some(asset) = assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(".deb"))
        {
            return Some(asset.browser_download_url.clone());
        }
    }

    // Generic fallback: look for zip or installer
    None
}

/// Evaluates whether a release represents an update compared to the current application version.
pub(crate) fn evaluate_release(
    current_version: &str,
    release: &GithubRelease,
) -> Result<UpdateCheckResponse, String> {
    let current_semver = parse_version(current_version)?;
    let latest_semver = parse_version(&release.tag_name)?;

    let clean_latest_version = normalize_version_tag(&release.tag_name).to_string();

    if latest_semver > current_semver {
        let download_url = pick_best_download_url(&release.assets);
        Ok(UpdateCheckResponse::UpdateAvailable {
            current_version: current_version.to_string(),
            latest_version: clean_latest_version,
            release_url: release.html_url.clone(),
            release_notes: release.body.clone(),
            download_url,
        })
    } else {
        Ok(UpdateCheckResponse::UpToDate {
            current_version: current_version.to_string(),
        })
    }
}

/// Checks GitHub Releases for the latest BreathScribe version using the configured NetworkManager client.
#[tauri::command]
#[specta::specta]
pub async fn check_for_updates(app: AppHandle) -> Result<UpdateCheckResponse, String> {
    let settings = settings::get_settings(&app);
    if !settings::update_checks_effectively_enabled(&settings) {
        return Ok(UpdateCheckResponse::Error {
            message: "Update checks are disabled in application settings or system environment."
                .to_string(),
        });
    }

    let current_version = app.package_info().version.to_string();

    let network_manager = app
        .try_state::<Arc<NetworkManager>>()
        .ok_or_else(|| "Network manager not initialized".to_string())?;

    let client = network_manager.client().await;

    let user_agent = format!("BreathScribe/{}", current_version);

    let response = match client
        .get(GITHUB_LATEST_RELEASE_URL)
        .header(reqwest::header::USER_AGENT, user_agent)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
    {
        Ok(res) => res,
        Err(err) => {
            let msg = format!("Failed to reach GitHub Releases: {}", err);
            log::warn!("Update check network error: {}", msg);
            return Ok(UpdateCheckResponse::Error { message: msg });
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable body>".to_string());
        let msg = if status == reqwest::StatusCode::FORBIDDEN
            && body.to_lowercase().contains("rate limit")
        {
            "GitHub API rate limit exceeded. Please try again later or visit GitHub Releases directly.".to_string()
        } else {
            format!("GitHub Releases API returned HTTP {}: {}", status, body)
        };
        log::warn!("Update check HTTP error: {}", msg);
        return Ok(UpdateCheckResponse::Error { message: msg });
    }

    let release: GithubRelease = match response.json().await {
        Ok(rel) => rel,
        Err(err) => {
            let msg = format!("Failed to parse GitHub Releases payload: {}", err);
            log::warn!("Update check JSON parse error: {}", msg);
            return Ok(UpdateCheckResponse::Error { message: msg });
        }
    };

    match evaluate_release(&current_version, &release) {
        Ok(result) => Ok(result),
        Err(err) => {
            log::warn!("Failed to evaluate release versions: {}", err);
            Ok(UpdateCheckResponse::Error { message: err })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_version_tag() {
        assert_eq!(normalize_version_tag("v0.1.2"), "0.1.2");
        assert_eq!(normalize_version_tag("V0.1.2"), "0.1.2");
        assert_eq!(normalize_version_tag(" 0.1.2 "), "0.1.2");
        assert_eq!(normalize_version_tag("v1.0.0-rc.1"), "1.0.0-rc.1");
    }

    #[test]
    fn test_parse_version() {
        assert_eq!(
            parse_version("0.1.1").unwrap(),
            semver::Version::new(0, 1, 1)
        );
        assert_eq!(
            parse_version("v0.1.1").unwrap(),
            semver::Version::new(0, 1, 1)
        );
        assert_eq!(
            parse_version("1.2.3-beta.1").unwrap(),
            semver::Version::parse("1.2.3-beta.1").unwrap()
        );
    }

    #[test]
    fn test_evaluate_release_newer_version() {
        let release = GithubRelease {
            tag_name: "v0.1.2".to_string(),
            html_url: "https://github.com/breathi3552/BreathScribe/releases/tag/v0.1.2".to_string(),
            body: Some("Bug fixes and improvements".to_string()),
            assets: vec![
                GithubAsset {
                    name: "BreathScribe_0.1.2_x64-setup.exe".to_string(),
                    browser_download_url: "https://github.com/download/setup.exe".to_string(),
                },
                GithubAsset {
                    name: "BreathScribe_0.1.2_x64_en-US.msi".to_string(),
                    browser_download_url: "https://github.com/download/app.msi".to_string(),
                },
            ],
        };

        let result = evaluate_release("0.1.1", &release).unwrap();
        match result {
            UpdateCheckResponse::UpdateAvailable {
                current_version,
                latest_version,
                release_url,
                release_notes,
                download_url,
            } => {
                assert_eq!(current_version, "0.1.1");
                assert_eq!(latest_version, "0.1.2");
                assert_eq!(
                    release_url,
                    "https://github.com/breathi3552/BreathScribe/releases/tag/v0.1.2"
                );
                assert_eq!(release_notes.as_deref(), Some("Bug fixes and improvements"));
                #[cfg(target_os = "windows")]
                assert_eq!(
                    download_url.as_deref(),
                    Some("https://github.com/download/setup.exe")
                );
            }
            other => panic!("Expected UpdateAvailable, got {:?}", other),
        }
    }

    #[test]
    fn test_evaluate_release_up_to_date() {
        let release = GithubRelease {
            tag_name: "v0.1.1".to_string(),
            html_url: "https://github.com/breathi3552/BreathScribe/releases/tag/v0.1.1".to_string(),
            body: None,
            assets: vec![],
        };

        let result = evaluate_release("0.1.1", &release).unwrap();
        assert_eq!(
            result,
            UpdateCheckResponse::UpToDate {
                current_version: "0.1.1".to_string()
            }
        );
    }

    #[test]
    fn test_evaluate_release_older_version() {
        let release = GithubRelease {
            tag_name: "v0.1.0".to_string(),
            html_url: "https://github.com/breathi3552/BreathScribe/releases/tag/v0.1.0".to_string(),
            body: None,
            assets: vec![],
        };

        let result = evaluate_release("0.1.1", &release).unwrap();
        assert_eq!(
            result,
            UpdateCheckResponse::UpToDate {
                current_version: "0.1.1".to_string()
            }
        );
    }

    #[test]
    fn test_semver_numeric_ordering() {
        let release = GithubRelease {
            tag_name: "v0.1.10".to_string(),
            html_url: "https://github.com/release".to_string(),
            body: None,
            assets: vec![],
        };

        // 0.1.10 must be strictly greater than 0.1.9 (lexicographical would fail)
        let result = evaluate_release("0.1.9", &release).unwrap();
        assert!(matches!(
            result,
            UpdateCheckResponse::UpdateAvailable { .. }
        ));
    }

    #[test]
    fn test_pick_best_download_url_windows() {
        let assets = vec![
            GithubAsset {
                name: "BreathScribe_0.1.1_x64_en-US.msi".to_string(),
                browser_download_url: "https://download/msi".to_string(),
            },
            GithubAsset {
                name: "BreathScribe_0.1.1_x64-setup.exe".to_string(),
                browser_download_url: "https://download/setup.exe".to_string(),
            },
        ];

        #[cfg(target_os = "windows")]
        {
            assert_eq!(
                pick_best_download_url(&assets),
                Some("https://download/setup.exe".to_string())
            );
        }
    }
}
