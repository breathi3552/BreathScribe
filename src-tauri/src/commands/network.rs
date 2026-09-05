use crate::network::{self, NetworkManager};
use crate::settings::{get_settings, write_settings, ProxySettings};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[tauri::command]
#[specta::specta]
pub async fn test_proxy_connectivity(
    app: AppHandle,
    settings: Option<ProxySettings>,
) -> Result<u64, String> {
    if let Some(candidate) = settings {
        let test_client = network::build_reqwest_client(&candidate)?;
        network::test_connectivity(&test_client).await
    } else {
        let network_manager = app
            .try_state::<Arc<NetworkManager>>()
            .ok_or_else(|| "网络管理器未初始化".to_string())?;
        let client = network_manager.client().await;
        network::test_connectivity(&client).await
    }
}

#[tauri::command]
#[specta::specta]
pub async fn update_proxy_settings(app: AppHandle, settings: ProxySettings) -> Result<(), String> {
    let network_manager = app
        .try_state::<Arc<NetworkManager>>()
        .ok_or_else(|| "网络管理器未初始化".to_string())?;
    network_manager
        .update_proxy_settings(settings.clone())
        .await?;

    let mut current = get_settings(&app);
    current.proxy = settings;
    write_settings(&app, current);

    Ok(())
}
