use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

use crate::network::NetworkManager;
use crate::settings::{get_settings, write_settings, CloudSttProviderSettings, TranscriptionMode};

#[tauri::command]
#[specta::specta]
pub fn set_transcription_mode(app: AppHandle, mode: TranscriptionMode) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.transcription_mode = mode;
    write_settings(&app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "transcription_mode",
        }),
    );
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_cloud_stt_api_key(
    app: AppHandle,
    provider_id: String,
    api_key: String,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.cloud_stt_api_keys.insert(provider_id, api_key);
    write_settings(&app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "cloud_stt_api_keys",
        }),
    );
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_cloud_stt_provider_settings(
    app: AppHandle,
    settings_input: CloudSttProviderSettings,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings
        .cloud_stt_providers
        .insert(settings_input.provider_id.clone(), settings_input);
    write_settings(&app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "cloud_stt_providers",
        }),
    );
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn test_cloud_stt_connection(
    app: AppHandle,
    provider_id: String,
    api_key: Option<String>,
    custom_base_url: Option<String>,
) -> Result<(), String> {
    let settings = get_settings(&app);
    let key = match api_key.as_deref().map(|k| k.trim()).filter(|k| !k.is_empty()) {
        Some(k) => k.to_string(),
        None => settings
            .cloud_stt_api_keys
            .get(&provider_id)
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .ok_or_else(|| "API Key 为空，请输入 API Key 进行测试".to_string())?,
    };

    let base_url = custom_base_url
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            settings
                .cloud_stt_providers
                .get(&provider_id)
                .and_then(|p| p.custom_base_url.as_deref())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("https://generativelanguage.googleapis.com");

    let network_manager = app
        .try_state::<Arc<NetworkManager>>()
        .ok_or_else(|| "网络管理器未初始化".to_string())?;
    let client = network_manager.client().await;

    let clean_base = base_url.trim_end_matches('/');
    let test_url = if clean_base.ends_with("/v1beta") {
        format!("{}/models?key={}", clean_base, key)
    } else {
        format!("{}/v1beta/models?key={}", clean_base, key)
    };

    let response = client
        .get(&test_url)
        .send()
        .await
        .map_err(|e| format!("网络请求发送失败: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        if let Ok(err_json) = serde_json::from_str::<serde_json::Value>(&error_text) {
            if let Some(msg) = err_json["error"]["message"].as_str() {
                return Err(format!("验证失败 (HTTP {}): {}", status, msg));
            }
        }
        return Err(format!("验证失败 HTTP {}: {}", status, error_text));
    }

    Ok(())
}
