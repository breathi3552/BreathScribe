use crate::network::{self, NetworkManager};
use crate::settings::ProxySettings;
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
