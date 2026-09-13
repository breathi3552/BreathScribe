use crate::network::{self, NetworkManager};
use crate::settings::ProxySettings;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[cfg(all(feature = "acceptance48_test", debug_assertions))]
use futures_util::{SinkExt, StreamExt};
#[cfg(all(feature = "acceptance48_test", debug_assertions))]
use tokio_tungstenite::tungstenite::Message;

#[tauri::command]
#[specta::specta]
pub async fn test_proxy_connectivity(
    app: AppHandle,
    settings: Option<ProxySettings>,
) -> Result<u64, String> {
    if let Some(candidate) = settings {
        return test_candidate_proxy_connectivity(candidate).await;
    }

    let network_manager = app
        .try_state::<Arc<NetworkManager>>()
        .ok_or_else(|| "Network manager not initialized".to_string())?;
    let client = network_manager.client().await;
    network::test_connectivity(&client).await
}

pub(crate) async fn test_candidate_proxy_connectivity(
    settings: ProxySettings,
) -> Result<u64, String> {
    let test_client = network::build_reqwest_client(&settings)?;
    network::test_connectivity(&test_client).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_proxy_settings(app: AppHandle, settings: ProxySettings) -> Result<(), String> {
    let network_manager = app
        .try_state::<Arc<NetworkManager>>()
        .ok_or_else(|| "Network manager not initialized".to_string())?;
    let app_for_persist = app.clone();
    update_proxy_settings_with_persistence(
        network_manager.inner().as_ref(),
        settings,
        move |settings| {
            let app = app_for_persist.clone();
            async move {
                crate::settings::try_update_settings(&app, |current| {
                    current.proxy = settings;
                })
            }
        },
    )
    .await
}

pub(crate) async fn update_proxy_settings_with_persistence<F, Fut>(
    network_manager: &NetworkManager,
    settings: ProxySettings,
    persist: F,
) -> Result<(), String>
where
    F: FnOnce(ProxySettings) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    network_manager
        .update_proxy_settings_with_persistence(settings, persist)
        .await
}

/// Private acceptance-only observation of the shared NetworkManager. The UI
/// still exercises the production commands; this only supplies a local,
/// deterministic WebSocket observation without adding a product command.
#[cfg(all(feature = "acceptance48_test", debug_assertions))]
#[derive(serde::Serialize, specta::Type)]
pub struct Acceptance48NetworkProbeResult {
    pub http_rtt_ms: u64,
    pub websocket_response: String,
}

#[cfg(all(feature = "acceptance48_test", debug_assertions))]
#[tauri::command]
#[specta::specta]
pub async fn acceptance48_probe_network(
    app: AppHandle,
    websocket_url: String,
) -> Result<Acceptance48NetworkProbeResult, String> {
    let network_manager = app
        .try_state::<Arc<NetworkManager>>()
        .ok_or_else(|| "Network manager not initialized".to_string())?;
    let client = network_manager.client().await;
    let http_rtt_ms = network::test_connectivity(&client).await?;
    let mut websocket = network_manager.connect_websocket(&websocket_url).await?;

    websocket
        .send(Message::Text("acceptance48-probe".into()))
        .await
        .map_err(|error| format!("WebSocket probe send failed: {error}"))?;
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), websocket.next())
        .await
        .map_err(|_| "WebSocket probe timed out".to_string())?
        .ok_or_else(|| "WebSocket probe closed without a response".to_string())?
        .map_err(|error| format!("WebSocket probe receive failed: {error}"))?;

    let websocket_response = match response {
        Message::Text(text) => text.to_string(),
        _ => return Err("WebSocket probe received an unexpected response".to_string()),
    };

    Ok(Acceptance48NetworkProbeResult {
        http_rtt_ms,
        websocket_response,
    })
}
