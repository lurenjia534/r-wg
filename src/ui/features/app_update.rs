use std::process::Command;
use std::time::Duration;

use gpui::{
    div, prelude::FluentBuilder, AnyWindowHandle, App, AppContext, IntoElement, ParentElement,
    Styled,
};
use gpui_component::{
    button::{Button, ButtonVariant},
    dialog::DialogButtonProps,
    ActiveTheme as _, WindowExt,
};
use http_body_util::{BodyExt as _, Empty};
use hyper::body::Bytes;
use hyper::{Request, StatusCode};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use serde::Deserialize;
use tokio::time::timeout;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/lurenjia534/r-wg/releases/latest";
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Debug)]
struct UpdateInfo {
    version: String,
    release_url: String,
    title: String,
    body: Option<String>,
}

#[derive(Debug)]
enum UpdateCheckError {
    Transport(String),
    Status(StatusCode),
    Parse(String),
    Timeout,
}

impl std::fmt::Display for UpdateCheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(message) => write!(f, "{message}"),
            Self::Status(status) => write!(f, "GitHub returned {status}"),
            Self::Parse(message) => write!(f, "{message}"),
            Self::Timeout => write!(f, "GitHub update check timed out"),
        }
    }
}

#[derive(Deserialize)]
struct GithubReleaseResponse {
    tag_name: String,
    html_url: String,
    name: Option<String>,
    body: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VersionParts {
    major: u64,
    minor: u64,
    patch: u64,
}

pub(crate) fn check_for_updates_on_startup(window_handle: AnyWindowHandle, cx: &mut App) {
    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async move { latest_update(CURRENT_VERSION).await })
            .await;

        match result {
            Ok(Some(update)) => {
                let _ = window_handle.update(cx, move |_root, window, cx| {
                    open_update_dialog(update.clone(), window, cx);
                });
            }
            Ok(None) => {}
            Err(err) => tracing::debug!("application update check failed: {err}"),
        }
    })
    .detach();
}

async fn latest_update(current_version: &str) -> Result<Option<UpdateInfo>, UpdateCheckError> {
    let release = fetch_latest_release().await?;
    if !is_newer_version(&release.tag_name, current_version) {
        return Ok(None);
    }

    Ok(Some(UpdateInfo {
        version: normalized_version_label(&release.tag_name),
        release_url: release.html_url,
        title: release.name.unwrap_or(release.tag_name),
        body: release.body.and_then(normalize_release_body),
    }))
}

async fn fetch_latest_release() -> Result<GithubReleaseResponse, UpdateCheckError> {
    let https = HttpsConnectorBuilder::new()
        .with_native_roots()
        .map_err(|error| UpdateCheckError::Transport(error.to_string()))?
        .https_only()
        .enable_http1()
        .build();
    let client = Client::builder(TokioExecutor::new()).build::<_, Empty<Bytes>>(https);
    let request = Request::get(GITHUB_LATEST_RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", concat!("r-wg/", env!("CARGO_PKG_VERSION")))
        .body(Empty::<Bytes>::new())
        .expect("static GitHub update request must be valid");

    timeout(UPDATE_CHECK_TIMEOUT, async {
        let response = client
            .request(request)
            .await
            .map_err(|error| UpdateCheckError::Transport(error.to_string()))?;

        if response.status() != StatusCode::OK {
            return Err(UpdateCheckError::Status(response.status()));
        }

        let body = response
            .into_body()
            .collect()
            .await
            .map_err(|error| UpdateCheckError::Transport(error.to_string()))?
            .to_bytes();

        serde_json::from_slice(&body).map_err(|error| UpdateCheckError::Parse(error.to_string()))
    })
    .await
    .map_err(|_| UpdateCheckError::Timeout)?
}

fn open_update_dialog(update: UpdateInfo, window: &mut gpui::Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _window, dlg_cx| {
        let release_url = update.release_url.clone();
        dialog
            .title(div().text_lg().child("Update available"))
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Open release")
                    .ok_variant(ButtonVariant::Primary)
                    .cancel_text("Later"),
            )
            .child(
                div()
                    .text_sm()
                    .child(format!("r-wg {} is available.", update.version)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(dlg_cx.theme().muted_foreground)
                    .child(format!("Current version: v{CURRENT_VERSION}")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(dlg_cx.theme().muted_foreground)
                    .child(update.title.clone()),
            )
            .when_some(update.body.clone(), |dialog, body| {
                dialog.child(
                    div()
                        .text_xs()
                        .text_color(dlg_cx.theme().foreground)
                        .child(body),
                )
            })
            .footer(move |_ok, _cancel, _window, _cx| {
                let open_button = Button::new("update-dialog-open-release")
                    .label("Open release")
                    .on_click({
                        let release_url = release_url.clone();
                        move |_, window, cx| {
                            if let Err(err) = open_release_url(&release_url) {
                                tracing::warn!("failed to open release URL: {err}");
                            }
                            window.close_dialog(cx);
                        }
                    });
                let later_button = Button::new("update-dialog-later").label("Later").on_click(
                    move |_, window, cx| {
                        window.close_dialog(cx);
                    },
                );
                vec![
                    later_button.into_any_element(),
                    open_button.into_any_element(),
                ]
            })
    });
}

fn open_release_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

fn normalize_release_body(body: String) -> Option<String> {
    let body = body.trim();
    if body.is_empty() {
        return None;
    }

    Some(body.lines().take(8).collect::<Vec<_>>().join("\n"))
}

fn is_newer_version(candidate: &str, current: &str) -> bool {
    let Some(candidate) = parse_version(candidate) else {
        return false;
    };
    let Some(current) = parse_version(current) else {
        return false;
    };
    (candidate.major, candidate.minor, candidate.patch)
        > (current.major, current.minor, current.patch)
}

fn parse_version(version: &str) -> Option<VersionParts> {
    let version = version
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('V');
    let version = version
        .split_once('-')
        .map_or(version, |(version, _)| version);
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(VersionParts {
        major,
        minor,
        patch,
    })
}

fn normalized_version_label(version: &str) -> String {
    let version = version.trim();
    if version.starts_with('v') || version.starts_with('V') {
        version.to_string()
    } else {
        format!("v{version}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semantic_versions() {
        assert!(is_newer_version("v0.3.2", "0.3.1"));
        assert!(is_newer_version("1.0.0", "0.9.9"));
        assert!(!is_newer_version("v0.3.1", "0.3.1"));
        assert!(!is_newer_version("v0.3.0", "0.3.1"));
    }

    #[test]
    fn parses_short_and_prefixed_versions() {
        assert_eq!(
            parse_version("v1.2"),
            Some(VersionParts {
                major: 1,
                minor: 2,
                patch: 0,
            })
        );
        assert_eq!(
            parse_version("1.2.3-beta.1"),
            Some(VersionParts {
                major: 1,
                minor: 2,
                patch: 3,
            })
        );
        assert_eq!(parse_version("release-1.2.3"), None);
    }
}
