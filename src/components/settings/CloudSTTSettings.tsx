import React, { useState, useEffect, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Cloud,
  CheckCircle2,
  XCircle,
  Loader2,
  Eye,
  EyeOff,
  ExternalLink,
  ChevronDown,
  ChevronRight,
  Sparkles,
  ShieldCheck,
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

  const providerId = "gemini";
  const storedProviderConfig =
    settings?.cloud_stt_providers?.[providerId] ??
    DEFAULT_CLOUD_STT_PROVIDER_SETTINGS;
  const storedApiKey = settings?.cloud_stt_api_keys?.[providerId] ?? "";

  const [apiKeyDraft, setApiKeyDraft] = useState(storedApiKey);
  const [showApiKey, setShowApiKey] = useState(false);
  const [selectedModel, setSelectedModel] = useState(
    storedProviderConfig.model_id || DEFAULT_CLOUD_MODEL_ID,
  );
  const [customBaseUrlDraft, setCustomBaseUrlDraft] = useState(
    storedProviderConfig.custom_base_url ?? "",
  );
  const [isAdvancedOpen, setIsAdvancedOpen] = useState(
    Boolean(storedProviderConfig.custom_base_url?.trim()),
  );

  const modelOptions: DropdownOption[] = useMemo(
    () => [
      {
        value: "gemini-3.5-transcribe-live",
        label: "Gemini 3.5 Transcribe Live",
        description: t(
          "settings.models.cloud.models.transcribeLiveDesc",
          "专用实时流式识别大模型，边说边出字，毫秒级极速打字（推荐）",
        ),
      },
      {
        value: "gemini-3.5-transcribe",
        label: "Gemini 3.5 Transcribe",
        description: t(
          "settings.models.cloud.models.transcribeDesc",
          "专用语音识别大模型，智能过滤语气词与标点规整化（批处理高精度）",
        ),
      },
      {
        value: "gemini-3.6-flash",
        label: "Gemini 3.6 Flash",
        description: t(
          "settings.models.cloud.models.flash36Desc",
          "最新一代通用多模态大模型，复杂语境与推理能力出色",
        ),
      },
      {
        value: "gemini-3.5-flash",
        label: "Gemini 3.5 Flash",
        description: t(
          "settings.models.cloud.models.flash35Desc",
          "轻量高性价比通用大模型，低延迟响应",
        ),
      },
    ],
    [t],
  );

  const providerOptions: DropdownOption[] = useMemo(
    () => [
      {
        value: "gemini",
        label: "Google Gemini",
        description: t(
          "settings.models.cloud.providerGoogleDesc",
          "Google Gemini 3.5 Transcribe & Flash",
        ),
      },
    ],
    [t],
  );

  const isKeyFormatValid = (key: string): boolean => {
    const trimmed = key.trim();
    if (!trimmed) return true;
    return (
      /^AIzaSy[A-Za-z0-9_-]{33}$/.test(trimmed) ||
      trimmed.startsWith("AQ.") ||
      trimmed.startsWith("ya29.") ||
      trimmed.length >= 20
    );
  };

  const [isValidating, setIsValidating] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [validationResult, setValidationResult] = useState<{
    success: boolean;
    error?: string;
  } | null>(null);

  // Sync state when store updates
  useEffect(() => {
    if (storedApiKey !== undefined) {
      setApiKeyDraft(storedApiKey);
    }
  }, [storedApiKey]);

  useEffect(() => {
    if (storedProviderConfig) {
      setSelectedModel(storedProviderConfig.model_id || DEFAULT_CLOUD_MODEL_ID);
      setCustomBaseUrlDraft(storedProviderConfig.custom_base_url ?? "");
    }
  }, [storedProviderConfig]);

  const handleModelChange = useCallback(
    async (modelId: string) => {
      setSelectedModel(modelId);
      try {
        await setCloudSttProviderSettings({
          ...storedProviderConfig,
          provider_id: providerId,
          model_id: modelId,
        });
        if (settings?.transcription_mode?.type === "cloud") {
          await setTranscriptionMode({
            type: "cloud",
            config: {
              provider_id: providerId,
              model_id: modelId,
            },
          });
        }
        toast.success(
          t("settings.models.cloud.modelUpdated", "已切换云端模型: {{model}}", {
            model: modelId,
          }),
        );
      } catch (err) {
        console.error("Failed to update cloud model:", err);
        toast.error(
          t(
            "settings.models.cloud.errors.modelUpdateFailed",
            "切换模型失败，请重试",
          ),
        );
      }
    },
    [
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
      await setCloudSttApiKey(providerId, apiKeyDraft.trim());
      toast.success(
        t("settings.models.cloud.keySaved", "Gemini API Key 已保存"),
      );
    } catch (err) {
      console.error("Failed to save API key:", err);
      toast.error(
        t("settings.models.cloud.errors.saveFailed", "保存失败，请检查设置"),
      );
    } finally {
      setIsSaving(false);
    }
  }, [apiKeyDraft, setCloudSttApiKey, t]);

  const handleValidateAndSave = useCallback(async () => {
    const key = apiKeyDraft.trim();
    if (!key) {
      toast.error(
        t("settings.models.cloud.errors.emptyKey", "请先填写 Gemini API Key"),
      );
      return;
    }

    if (!isKeyFormatValid(key) && !customBaseUrlDraft.trim()) {
      toast.warning(
        t(
          "settings.models.cloud.warnings.keyFormatHint",
          "提示：标准的 Google Gemini API Key 通常以 'AIzaSy' 或 'AQ.' 开头",
        ),
      );
    }
    setIsValidating(true);
    setValidationResult(null);

    try {
      await testCloudSttConnection(
        providerId,
        key,
        customBaseUrlDraft.trim() || undefined,
      );
      await setCloudSttApiKey(providerId, key);
      setValidationResult({ success: true });
      toast.success(
        t(
          "settings.models.cloud.validatedAndSaved",
          "凭据验证通过并已成功保存！",
        ),
      );
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      setValidationResult({ success: false, error: msg });
      toast.error(
        t(
          "settings.models.cloud.errors.validationFailed",
          "验证失败: {{error}}",
          { error: msg },
        ),
      );
    } finally {
      setIsValidating(false);
    }
  }, [
    apiKeyDraft,
    customBaseUrlDraft,
    setCloudSttApiKey,
    testCloudSttConnection,
    t,
  ]);

  const handleSaveCustomBaseUrl = useCallback(async () => {
    const trimmed = customBaseUrlDraft.trim();
    try {
      await setCloudSttProviderSettings({
        ...storedProviderConfig,
        provider_id: providerId,
        custom_base_url: trimmed ? trimmed : null,
      });
      toast.success(
        t("settings.models.cloud.baseUrlSaved", "自定义 API 地址已更新"),
      );
    } catch (err) {
      console.error("Failed to update custom base URL:", err);
      toast.error(
        t(
          "settings.models.cloud.errors.baseUrlSaveFailed",
          "保存自定义地址失败",
        ),
      );
    }
  }, [
    customBaseUrlDraft,
    setCloudSttProviderSettings,
    storedProviderConfig,
    t,
  ]);

  const handleOpenAiStudio = async () => {
    try {
      await openUrl(GOOGLE_AI_STUDIO_URL);
    } catch (error) {
      console.error("Failed to open Google AI Studio:", error);
    }
  };

  const content = (
    <div className="space-y-4">
      {/* Provider Info Card */}
      <div className="rounded-xl border border-mid-gray/40 bg-mid-gray/10 p-4">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <div className="p-2.5 rounded-lg bg-background-ui/20 text-text border border-background-ui/30">
              <Sparkles className="w-5 h-5 text-logo-primary" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <span className="font-semibold text-sm">Google Gemini 3.5</span>
                <span className="px-2 py-0.5 text-[10px] font-medium rounded-full bg-logo-primary/15 text-text border border-logo-primary/30">
                  {t("settings.models.cloud.cloudSttBadge", "云端大模型")}
                </span>
              </div>
              <p className="text-xs text-text/60 mt-0.5">
                {t(
                  "settings.models.cloud.providerDescription",
                  "无需本地显卡，依托全球网络连接池提供高准确率实时语音识别",
                )}
              </p>
            </div>
          </div>
          <Button
            variant="secondary"
            size="sm"
            onClick={handleOpenAiStudio}
            className="flex items-center gap-1.5 self-start sm:self-auto text-xs"
          >
            <span>
              {t("settings.models.cloud.getApiKey", "获取免费 API Key")}
            </span>
            <ExternalLink className="w-3 h-3" />
          </Button>
        </div>
      </div>
      {/* Provider Selection */}
      <SettingContainer
        title={t("settings.models.cloud.providerSelectTitle", "服务提供商")}
        description={t(
          "settings.models.cloud.providerSelectDesc",
          "选择接入的云端大模型服务商（首阶段支持 Google Gemini）",
        )}
      >
        <div className="w-64">
          <Dropdown
            options={providerOptions}
            selectedValue={providerId}
            onSelect={() => {}}
          />
        </div>
      </SettingContainer>

      {/* Model Selection */}
      <SettingContainer
        title={t("settings.models.cloud.modelSelectTitle", "云端模型版本")}
        description={t(
          "settings.models.cloud.modelSelectDesc",
          "推荐使用 2.5 Flash 获取毫秒级转写响应；需要强推理或罕见专有名词建议选用 Pro",
        )}
      >
        <div className="w-64">
          <Dropdown
            options={modelOptions}
            selectedValue={selectedModel}
            onSelect={handleModelChange}
          />
        </div>
      </SettingContainer>
      {/* API Key Input */}
      <SettingContainer
        title={t("settings.models.cloud.apiKeyTitle", "Google Gemini API Key")}
        description={t(
          "settings.models.cloud.apiKeyDesc",
          "在本地配置文件中安全存储并自动脱敏，绝不上传第三方服务器",
        )}
      >
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
                placeholder="AIzaSy..."
                className="w-full pr-10 font-mono text-xs"
              />
              <button
                type="button"
                onClick={() => setShowApiKey(!showApiKey)}
                className="absolute right-2.5 top-1/2 -translate-y-1/2 text-text/50 hover:text-text transition-colors p-1"
                title={
                  showApiKey
                    ? t("settings.models.cloud.hideKey", "隐藏明文")
                    : t("settings.models.cloud.showKey", "显示明文")
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
                  <span>
                    {t("settings.models.cloud.validating", "验证中...")}
                  </span>
                </>
              ) : (
                <span>
                  {t("settings.models.cloud.validateAndSave", "验证并保存")}
                </span>
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
                ? t("settings.models.cloud.saving", "保存中...")
                : t("settings.models.cloud.saveOnly", "保存")}
            </Button>
          </div>

          {apiKeyDraft.trim() &&
            !isKeyFormatValid(apiKeyDraft) &&
            !customBaseUrlDraft.trim() && (
              <p className="text-[11px] text-amber-500/90 font-medium">
                {t(
                  "settings.models.cloud.warnings.keyFormatNote",
                  "格式提示：标准的 Google Gemini API Key 通常为以 'AIzaSy' 或 'AQ.' 开头的字符",
                )}
              </p>
            )}

          {/* Validation Feedback */}
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
                      {t(
                        "settings.models.cloud.connectSuccess",
                        "连通性验证成功",
                      )}
                    </p>
                    <p className="text-[11px] opacity-80">
                      {t(
                        "settings.models.cloud.connectSuccessDesc",
                        "Gemini REST 接口与凭据握手正常，已就绪供转写调用",
                      )}
                    </p>
                  </div>
                </>
              ) : (
                <>
                  <XCircle className="w-4 h-4 shrink-0 mt-0.5" />
                  <div className="space-y-0.5">
                    <p className="font-semibold">
                      {t("settings.models.cloud.connectFailed", "验证未通过")}
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

      {/* Advanced Section: Custom Base URL */}
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
            <span>
              {t(
                "settings.models.cloud.advancedCustomUrl",
                "高级配置：自定义 API 代理地址 (可选)",
              )}
            </span>
          </div>
          <span className="text-[11px] text-text/50 font-normal">
            {customBaseUrlDraft
              ? customBaseUrlDraft
              : t("settings.models.cloud.defaultOfficialUrl", "官方默认地址")}
          </span>
        </button>

        {isAdvancedOpen && (
          <div className="p-4 bg-mid-gray/5 border-t border-mid-gray/20 space-y-3">
            <SettingContainer
              title={t(
                "settings.models.cloud.customBaseUrlTitle",
                "API Base URL",
              )}
              description={t(
                "settings.models.cloud.customBaseUrlDesc",
                "适用于自建反代网关、Cloudflare AI Gateway 或特定中转路由。留空则直连官方接口",
              )}
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
                  {t("settings.models.cloud.saveBaseUrl", "更新地址")}
                </Button>
                {customBaseUrlDraft && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      setCustomBaseUrlDraft("");
                      setCloudSttProviderSettings({
                        ...storedProviderConfig,
                        provider_id: providerId,
                        custom_base_url: null,
                      });
                    }}
                    className="shrink-0 text-xs text-text/60 hover:text-text"
                  >
                    {t("settings.models.cloud.resetBaseUrl", "重置")}
                  </Button>
                )}
              </div>
            </SettingContainer>
          </div>
        )}
      </div>

      {/* Fail-Safe and Security Guarantee Footer */}
      <div className="flex items-start gap-2.5 p-3 rounded-lg bg-logo-primary/5 border border-logo-primary/15 text-xs text-text/70">
        <ShieldCheck className="w-4 h-4 text-logo-primary shrink-0 mt-0.5" />
        <div className="space-y-0.5">
          <p className="font-medium text-text">
            {t(
              "settings.models.cloud.failSafeTitle",
              "确定性预期与录音安全保证",
            )}
          </p>
          <p className="text-[11px] leading-relaxed">
            {t(
              "settings.models.cloud.failSafeDesc",
              "云端识别遇网络超时或凭据错误时立即弹出明确提示，绝不静默回退至本地模型造成卡顿。所有原始录音完整保留在历史记录中，配置好网络后可随时在历史界面一键重试。",
            )}
          </p>
        </div>
      </div>
    </div>
  );

  if (grouped) {
    return (
      <SettingsGroup
        title={t("settings.models.cloud.groupTitle", "云端语音识别设置")}
        description={t(
          "settings.models.cloud.groupDescription",
          "配置云端大模型语音识别服务商、模型与鉴权凭据",
        )}
      >
        {content}
      </SettingsGroup>
    );
  }

  return content;
};
