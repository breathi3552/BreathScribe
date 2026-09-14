import React, { useState, useEffect, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  CheckCircle2,
  XCircle,
  Loader2,
  Eye,
  EyeOff,
  ExternalLink,
  ChevronDown,
  ChevronRight,
} from "lucide-react";
import { toast } from "sonner";

import { useSettingsStore } from "@/stores/settingsStore";
import { SettingContainer } from "../ui/SettingContainer";
import { SettingsGroup } from "../ui/SettingsGroup";
import { Dropdown, type DropdownOption } from "../ui/Dropdown";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import {
  DEFAULT_CLOUD_MODEL_ID,
  DEFAULT_CLOUD_STT_PROVIDER_SETTINGS,
} from "@/bindings";

interface CloudSTTSettingsProps {
  grouped?: boolean;
}

const GOOGLE_AI_STUDIO_URL = "https://aistudio.google.com/app/apikey";
export interface CloudSttModelOption {
  value: string;
  label: string;
  descriptionKey: string;
}

export interface CloudSttProviderConfig {
  id: string;
  label: string;
  descriptionKey?: string;
  defaultDescription?: string;
  apiKeyUrl?: string;
  defaultModelId: string;
  placeholder?: string;
  models: CloudSttModelOption[];
  isKeyFormatValid?: (key: string) => boolean;
  keyFormatHintKey?: string;
  keyFormatNoteKey?: string;
}

export const CLOUD_STT_PROVIDERS: Record<string, CloudSttProviderConfig> = {
  gemini: {
    id: "gemini",
    label: "Google Gemini",
    descriptionKey: "settings.models.cloud.providerGoogleDesc",
    defaultDescription: "Google Gemini 3.5 Transcribe",
    apiKeyUrl: GOOGLE_AI_STUDIO_URL,
    defaultModelId: "gemini-3.5-transcribe",
    placeholder: "AIzaSy...",
    models: [
      {
        value: "gemini-3.5-transcribe-live",
        label: "Gemini 3.5 Transcribe Live",
        descriptionKey: "settings.models.cloud.models.transcribeLiveDesc",
      },
      {
        value: "gemini-3.5-transcribe",
        label: "Gemini 3.5 Transcribe",
        descriptionKey: "settings.models.cloud.models.transcribeDesc",
      },
    ],
    isKeyFormatValid: (key: string): boolean => {
      const trimmed = key.trim();
      if (!trimmed) return true;
      return (
        /^AIzaSy[A-Za-z0-9_-]{33}$/.test(trimmed) ||
        trimmed.startsWith("AQ.") ||
        trimmed.startsWith("ya29.") ||
        trimmed.length >= 20
      );
    },
    keyFormatHintKey: "settings.models.cloud.warnings.keyFormatHint",
    keyFormatNoteKey: "settings.models.cloud.warnings.keyFormatNote",
  },
};

export const CloudSTTSettings: React.FC<CloudSTTSettingsProps> = ({
  grouped = true,
}) => {
  const { t } = useTranslation();
  const settings = useSettingsStore((state) => state.settings);
  const setCloudSttApiKey = useSettingsStore(
    (state) => state.setCloudSttApiKey,
  );
  const setCloudSttProviderSettings = useSettingsStore(
    (state) => state.setCloudSttProviderSettings,
  );
  const setTranscriptionMode = useSettingsStore(
    (state) => state.setTranscriptionMode,
  );
  const testCloudSttConnection = useSettingsStore(
    (state) => state.testCloudSttConnection,
  );

  const activeCloudProviderId =
    settings?.transcription_mode?.type === "cloud"
      ? settings.transcription_mode.config.provider_id
      : null;

  const [selectedProviderId, setSelectedProviderId] = useState<string>(
    activeCloudProviderId || "gemini",
  );

  useEffect(() => {
    if (activeCloudProviderId && CLOUD_STT_PROVIDERS[activeCloudProviderId]) {
      setSelectedProviderId(activeCloudProviderId);
    }
  }, [activeCloudProviderId]);

  const currentProvider =
    CLOUD_STT_PROVIDERS[selectedProviderId] ?? CLOUD_STT_PROVIDERS.gemini;

  const storedProviderConfig =
    settings?.cloud_stt_providers?.[selectedProviderId] ??
    DEFAULT_CLOUD_STT_PROVIDER_SETTINGS;
  const storedApiKey = settings?.cloud_stt_api_keys?.[selectedProviderId] ?? "";

  const [apiKeyDraft, setApiKeyDraft] = useState(storedApiKey);
  const [showApiKey, setShowApiKey] = useState(false);
  const [selectedModel, setSelectedModel] = useState(
    storedProviderConfig.model_id ||
      currentProvider.defaultModelId ||
      DEFAULT_CLOUD_MODEL_ID,
  );
  const [customBaseUrlDraft, setCustomBaseUrlDraft] = useState(
    storedProviderConfig.custom_base_url ?? "",
  );
  const [isAdvancedOpen, setIsAdvancedOpen] = useState(
    Boolean(storedProviderConfig.custom_base_url?.trim()),
  );

  const modelOptions: DropdownOption[] = useMemo(
    () =>
      currentProvider.models.map((model) => ({
        value: model.value,
        label: model.label,
        description: t(model.descriptionKey),
      })),
    [currentProvider, t],
  );

  const providerOptions: DropdownOption[] = useMemo(
    () =>
      Object.values(CLOUD_STT_PROVIDERS).map((provider) => ({
        value: provider.id,
        label: provider.label,
        description: provider.descriptionKey
          ? t(provider.descriptionKey, {
              defaultValue: provider.defaultDescription,
            })
          : provider.defaultDescription,
      })),
    [t],
  );

  const isKeyFormatValid = useCallback(
    (key: string): boolean => {
      return currentProvider.isKeyFormatValid
        ? currentProvider.isKeyFormatValid(key)
        : true;
    },
    [currentProvider],
  );

  const [isValidating, setIsValidating] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [validationResult, setValidationResult] = useState<{
    success: boolean;
    error?: string;
  } | null>(null);

  useEffect(() => {
    setApiKeyDraft(storedApiKey);
  }, [storedApiKey]);

  useEffect(() => {
    setSelectedModel(
      storedProviderConfig.model_id ||
        currentProvider.defaultModelId ||
        DEFAULT_CLOUD_MODEL_ID,
    );
    setCustomBaseUrlDraft(storedProviderConfig.custom_base_url ?? "");
  }, [storedProviderConfig, currentProvider]);

  const handleModelChange = useCallback(
    async (modelId: string) => {
      setSelectedModel(modelId);
      try {
        await setCloudSttProviderSettings({
          ...storedProviderConfig,
          provider_id: selectedProviderId,
          model_id: modelId,
        });
        if (settings?.transcription_mode?.type === "cloud") {
          await setTranscriptionMode({
            type: "cloud",
            config: {
              provider_id: selectedProviderId,
              model_id: modelId,
            },
          });
        }
        toast.success(
          t("settings.models.cloud.modelUpdated", {
            model: modelId,
          }),
        );
      } catch (err) {
        console.error("Failed to update cloud model:", err);
        toast.error(t("settings.models.cloud.errors.modelUpdateFailed"));
      }
    },
    [
      selectedProviderId,
      setCloudSttProviderSettings,
      setTranscriptionMode,
      settings?.transcription_mode,
      storedProviderConfig,
      t,
    ],
  );

  const handleSaveApiKey = useCallback(async () => {
    setIsSaving(true);
    try {
      await setCloudSttApiKey(selectedProviderId, apiKeyDraft.trim());
      toast.success(t("settings.models.cloud.keySaved"));
    } catch (err) {
      console.error("Failed to save API key:", err);
      toast.error(t("settings.models.cloud.errors.saveFailed"));
    } finally {
      setIsSaving(false);
    }
  }, [apiKeyDraft, selectedProviderId, setCloudSttApiKey, t]);

  const handleValidateAndSave = useCallback(async () => {
    const key = apiKeyDraft.trim();
    if (!key) {
      toast.error(t("settings.models.cloud.errors.emptyKey"));
      return;
    }

    if (
      currentProvider.isKeyFormatValid &&
      !currentProvider.isKeyFormatValid(key) &&
      !customBaseUrlDraft.trim()
    ) {
      toast.warning(
        t(
          currentProvider.keyFormatHintKey ??
            "settings.models.cloud.warnings.keyFormatHint",
        ),
      );
    }
    setIsValidating(true);
    setValidationResult(null);

    try {
      await testCloudSttConnection(
        selectedProviderId,
        key,
        customBaseUrlDraft.trim() || undefined,
      );
      await setCloudSttApiKey(selectedProviderId, key);
      setValidationResult({ success: true });
      toast.success(t("settings.models.cloud.validatedAndSaved"));
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      setValidationResult({ success: false, error: msg });
      toast.error(
        t("settings.models.cloud.errors.validationFailed", { error: msg }),
      );
    } finally {
      setIsValidating(false);
    }
  }, [
    apiKeyDraft,
    currentProvider,
    customBaseUrlDraft,
    selectedProviderId,
    setCloudSttApiKey,
    testCloudSttConnection,
    t,
  ]);

  const handleSaveCustomBaseUrl = useCallback(async () => {
    const trimmed = customBaseUrlDraft.trim();
    try {
      await setCloudSttProviderSettings({
        ...storedProviderConfig,
        provider_id: selectedProviderId,
        custom_base_url: trimmed ? trimmed : null,
      });
      toast.success(t("settings.models.cloud.baseUrlSaved"));
    } catch (err) {
      console.error("Failed to update custom base URL:", err);
      toast.error(t("settings.models.cloud.errors.baseUrlSaveFailed"));
    }
  }, [
    customBaseUrlDraft,
    selectedProviderId,
    setCloudSttProviderSettings,
    storedProviderConfig,
    t,
  ]);

  const handleProviderChange = useCallback(
    async (newProviderId: string) => {
      setSelectedProviderId(newProviderId);
      setValidationResult(null);

      const targetProvider =
        CLOUD_STT_PROVIDERS[newProviderId] ?? CLOUD_STT_PROVIDERS.gemini;
      const targetConfig =
        settings?.cloud_stt_providers?.[newProviderId] ??
        DEFAULT_CLOUD_STT_PROVIDER_SETTINGS;
      const targetModel =
        targetConfig.model_id || targetProvider.defaultModelId;

      if (settings?.transcription_mode?.type === "cloud") {
        try {
          await setTranscriptionMode({
            type: "cloud",
            config: {
              provider_id: newProviderId,
              model_id: targetModel,
            },
          });
        } catch (err) {
          console.error(
            "Failed to update transcription mode on provider change:",
            err,
          );
        }
      }
    },
    [
      settings?.cloud_stt_providers,
      settings?.transcription_mode,
      setTranscriptionMode,
    ],
  );

  const handleGetApiKey = useCallback(async () => {
    if (!currentProvider?.apiKeyUrl) return;
    try {
      await openUrl(currentProvider.apiKeyUrl);
    } catch (error) {
      console.error(
        `Failed to open API key URL for ${currentProvider.id}:`,
        error,
      );
    }
  }, [currentProvider]);

  const content = (
    <div className="space-y-4">
      <SettingContainer title={t("settings.models.cloud.providerSelectTitle")}>
        <div className="flex items-center gap-2">
          <div className="w-64">
            <Dropdown
              options={providerOptions}
              selectedValue={selectedProviderId}
              onSelect={handleProviderChange}
            />
          </div>
          {currentProvider?.apiKeyUrl && (
            <Button
              variant="secondary"
              size="sm"
              onClick={handleGetApiKey}
              className="flex items-center gap-1.5 text-xs shrink-0"
              title={t("settings.models.cloud.getApiKey")}
            >
              <span>{t("settings.models.cloud.getApiKey")}</span>
              <ExternalLink className="w-3 h-3" />
            </Button>
          )}
        </div>
      </SettingContainer>

      <SettingContainer title={t("settings.models.cloud.modelSelectTitle")}>
        <div className="w-64">
          <Dropdown
            options={modelOptions}
            selectedValue={selectedModel}
            onSelect={handleModelChange}
          />
        </div>
      </SettingContainer>
      <SettingContainer title={t("settings.models.cloud.apiKeyTitle")}>
        <div className="space-y-2 w-full max-w-md">
          <div className="flex items-center gap-2">
            <div className="relative flex-1">
              <Input
                type={showApiKey ? "text" : "password"}
                value={apiKeyDraft}
                onChange={(e) => {
                  setApiKeyDraft(e.target.value);
                  setValidationResult(null);
                }}
                placeholder={currentProvider.placeholder ?? "AIzaSy..."}
                className="w-full pr-10 font-mono text-xs"
              />
              <button
                type="button"
                onClick={() => setShowApiKey(!showApiKey)}
                className="absolute right-2.5 top-1/2 -translate-y-1/2 text-text/50 hover:text-text transition-colors p-1"
                title={
                  showApiKey
                    ? t("settings.models.cloud.hideKey")
                    : t("settings.models.cloud.showKey")
                }
              >
                {showApiKey ? (
                  <EyeOff className="w-3.5 h-3.5" />
                ) : (
                  <Eye className="w-3.5 h-3.5" />
                )}
              </button>
            </div>
            <Button
              variant="primary"
              size="sm"
              onClick={handleValidateAndSave}
              disabled={isValidating || !apiKeyDraft.trim()}
              className="shrink-0 flex items-center gap-1.5 text-xs"
            >
              {isValidating ? (
                <>
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  <span>{t("settings.models.cloud.validating")}</span>
                </>
              ) : (
                <span>{t("settings.models.cloud.validateAndSave")}</span>
              )}
            </Button>
            <Button
              variant="secondary"
              size="sm"
              onClick={handleSaveApiKey}
              disabled={isSaving || apiKeyDraft === storedApiKey}
              className="shrink-0 text-xs"
            >
              {isSaving
                ? t("settings.models.cloud.saving")
                : t("settings.models.cloud.saveOnly")}
            </Button>
          </div>

          {apiKeyDraft.trim() &&
            !isKeyFormatValid(apiKeyDraft) &&
            !customBaseUrlDraft.trim() && (
              <p className="text-[11px] text-amber-500/90 font-medium">
                {t(
                  currentProvider.keyFormatNoteKey ??
                    "settings.models.cloud.warnings.keyFormatNote",
                )}
              </p>
            )}

          {validationResult && (
            <div
              className={`flex items-start gap-2 p-2.5 rounded-lg text-xs ${
                validationResult.success
                  ? "bg-green-500/10 border border-green-500/20 text-green-600 dark:text-green-400"
                  : "bg-red-500/10 border border-red-500/20 text-red-600 dark:text-red-400"
              }`}
            >
              {validationResult.success ? (
                <>
                  <CheckCircle2 className="w-4 h-4 shrink-0 mt-0.5" />
                  <div>
                    <p className="font-semibold">
                      {t("settings.models.cloud.connectSuccess")}
                    </p>
                    <p className="text-[11px] opacity-80">
                      {t("settings.models.cloud.connectSuccessDesc")}
                    </p>
                  </div>
                </>
              ) : (
                <>
                  <XCircle className="w-4 h-4 shrink-0 mt-0.5" />
                  <div className="space-y-0.5">
                    <p className="font-semibold">
                      {t("settings.models.cloud.connectFailed")}
                    </p>
                    <p className="text-[11px] opacity-90 break-all font-mono">
                      {validationResult.error}
                    </p>
                  </div>
                </>
              )}
            </div>
          )}
        </div>
      </SettingContainer>

      <div className="border border-mid-gray/30 rounded-xl overflow-hidden">
        <button
          type="button"
          onClick={() => setIsAdvancedOpen(!isAdvancedOpen)}
          className="w-full flex items-center justify-between p-3.5 bg-mid-gray/5 hover:bg-mid-gray/10 text-start text-xs font-semibold transition-colors"
        >
          <div className="flex items-center gap-2">
            {isAdvancedOpen ? (
              <ChevronDown className="w-4 h-4 text-text/60" />
            ) : (
              <ChevronRight className="w-4 h-4 text-text/60" />
            )}
            <span>{t("settings.models.cloud.advancedCustomUrl")}</span>
          </div>
          {customBaseUrlDraft ? (
            <span className="text-[11px] text-text/50 font-normal truncate max-w-xs">
              {customBaseUrlDraft}
            </span>
          ) : null}
        </button>

        {isAdvancedOpen && (
          <div className="p-4 bg-mid-gray/5 border-t border-mid-gray/20 space-y-3">
            <SettingContainer
              title={t("settings.models.cloud.customBaseUrlTitle")}
              description={t("settings.models.cloud.customBaseUrlDesc")}
            >
              <div className="flex items-center gap-2 w-full max-w-md">
                <Input
                  type="text"
                  value={customBaseUrlDraft}
                  onChange={(e) => setCustomBaseUrlDraft(e.target.value)}
                  placeholder="https://generativelanguage.googleapis.com"
                  className="flex-1 font-mono text-xs"
                />
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={handleSaveCustomBaseUrl}
                  className="shrink-0 text-xs"
                >
                  {t("settings.models.cloud.saveBaseUrl")}
                </Button>
                {customBaseUrlDraft && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      setCustomBaseUrlDraft("");
                      setCloudSttProviderSettings({
                        ...storedProviderConfig,
                        provider_id: selectedProviderId,
                        custom_base_url: null,
                      });
                    }}
                    className="shrink-0 text-xs text-text/60 hover:text-text"
                  >
                    {t("settings.models.cloud.resetBaseUrl")}
                  </Button>
                )}
              </div>
            </SettingContainer>
          </div>
        )}
      </div>
    </div>
  );

  if (grouped) {
    return (
      <SettingsGroup
        title={t("settings.models.cloud.groupTitle")}
        description={t("settings.models.cloud.groupDescription")}
      >
        {content}
      </SettingsGroup>
    );
  }

  return content;
};
