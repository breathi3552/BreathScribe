use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

use crate::network::NetworkManager;
use crate::settings::{
    get_settings, write_settings, AppSettings, CloudSttProviderSettings, TranscriptionMode,
};

#[tauri::command]
#[specta::specta]
pub fn set_transcription_mode(app: AppHandle, mode: TranscriptionMode) -> Result<(), String> {
    let mut settings = get_settings(&app);
    let previous_mode = settings.transcription_mode.clone();
    settings.transcription_mode = mode.clone();
    write_settings(&app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "transcription_mode",
        }),
    );

    if mode == TranscriptionMode::Local && previous_mode != TranscriptionMode::Local {
        let current_settings = get_settings(&app);
        if !current_settings.selected_model.is_empty() {
            if let Some(tm) =
                app.try_state::<Arc<crate::managers::transcription::TranscriptionManager>>()
            {
                tm.initiate_model_load();
            }
        }
    }
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
    let key = match api_key
        .as_deref()
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
    {
        Some(k) => k.to_string(),
        None => settings
            .cloud_stt_api_keys
            .get(&provider_id)
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .ok_or_else(|| "API Key 为空，请输入 API Key 进行测试".to_string())?,
    };

    let custom_base = custom_base_url
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
        });

    let network_manager = app
        .try_state::<Arc<NetworkManager>>()
        .ok_or_else(|| "网络管理器未初始化".to_string())?;
    let client = network_manager.client().await;

    match provider_id.as_str() {
        "gemini" => {
            crate::providers::gemini::GeminiProvider::test_connection(&client, &key, custom_base)
                .await
        }
        unknown => Err(format!("未知的云端转写提供商: {}", unknown)),
    }
}

#[tauri::command]
#[specta::specta]
pub fn complete_onboarding_cloud(app: AppHandle) -> Result<(), String> {
    let mut settings = get_settings(&app);
    apply_complete_onboarding_cloud(&mut settings);
    write_settings(&app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "onboarding_completed",
        }),
    );
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "transcription_mode",
        }),
    );
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "selected_model",
        }),
    );
    Ok(())
}

pub(crate) fn apply_complete_onboarding_cloud(settings: &mut AppSettings) {
    settings.transcription_mode = settings.resolve_cloud_transcription_mode();
    settings.onboarding_completed = true;
    settings.selected_model = String::new();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    #[test]
    fn test_apply_complete_onboarding_cloud_from_initial_state() {
        let mut settings = get_default_settings();
        assert!(!settings.onboarding_completed);
        assert_eq!(settings.transcription_mode, TranscriptionMode::Local);

        apply_complete_onboarding_cloud(&mut settings);

        assert!(settings.onboarding_completed);
        assert_eq!(settings.selected_model, "");
        match settings.transcription_mode {
            TranscriptionMode::Cloud {
                provider_id,
                model_id,
            } => {
                assert_eq!(provider_id, "gemini");
                assert_eq!(model_id, "gemini-3.5-transcribe");
            }
            _ => panic!("Expected Cloud transcription mode"),
        }
    }

    #[test]
    fn test_apply_complete_onboarding_cloud_preserves_custom_cloud_config() {
        let mut settings = get_default_settings();
        settings.transcription_mode = TranscriptionMode::Cloud {
            provider_id: "gemini".to_string(),
            model_id: "gemini-2.5-pro".to_string(),
        };
        settings.selected_model = "parakeet-tdt-0.6b-v3".to_string();
        settings.onboarding_completed = false;

        apply_complete_onboarding_cloud(&mut settings);

        assert!(settings.onboarding_completed);
        assert_eq!(settings.selected_model, "");
        match settings.transcription_mode {
            TranscriptionMode::Cloud {
                provider_id,
                model_id,
            } => {
                assert_eq!(provider_id, "gemini");
                assert_eq!(model_id, "gemini-2.5-pro");
            }
            _ => panic!("Expected Cloud transcription mode"),
        }
    }
}
