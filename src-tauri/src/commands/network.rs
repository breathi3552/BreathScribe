use crate::network::{self, NetworkManager};
use crate::settings::{get_settings, try_write_settings, ProxySettings};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

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
    update_proxy_settings_with_persistence(
        network_manager.inner().as_ref(),
        get_settings(&app),
        settings,
        |settings| try_write_settings(&app, settings),
    )
    .await
}

pub(crate) async fn update_proxy_settings_with_persistence<F>(
    network_manager: &NetworkManager,
    mut current: crate::settings::AppSettings,
    settings: ProxySettings,
    persist: F,
) -> Result<(), String>
where
    F: FnOnce(crate::settings::AppSettings) -> Result<(), String>,
{
    let settings = network::normalize_proxy_settings(settings)?;
    let new_client = network::build_reqwest_client(&settings)?;
    current.proxy = settings.clone();
    // Persist first. Installing the new client only after this succeeds keeps
    // the in-memory and on-disk effective settings in step.
    persist(current)?;
    network_manager
        .install_proxy_settings(settings, new_client)
        .await;
    log::info!("NetworkManager: proxy client successfully reloaded");

    Ok(())
}
