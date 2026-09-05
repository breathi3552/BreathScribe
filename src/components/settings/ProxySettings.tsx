import React, { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsStore } from "@/stores/settingsStore";
import type {
  ProxySettings as ProxySettingsType,
  ProxyMode,
  ProxyProtocol,
} from "@/bindings";
import { DEFAULT_PROXY_SETTINGS } from "@/bindings";
import { SettingContainer } from "../ui/SettingContainer";
import { Dropdown, type DropdownOption } from "../ui/Dropdown";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { toast } from "sonner";
import { CheckCircle2, XCircle, Loader2, Globe } from "lucide-react";

interface ProxySettingsProps {
  grouped?: boolean;
}

export const ProxySettings: React.FC<ProxySettingsProps> = ({
  grouped = true,
}) => {
  const { t } = useTranslation();
  const settings = useSettingsStore((state) => state.settings);
  const updateProxySettings = useSettingsStore(
    (state) => state.updateProxySettings,
  );
  const testProxyConnectivity = useSettingsStore(
    (state) => state.testProxyConnectivity,
  );

  const [draft, setDraft] = useState<ProxySettingsType>(
    settings?.proxy ?? DEFAULT_PROXY_SETTINGS,
  );
  const [isTesting, setIsTesting] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [testResult, setTestResult] = useState<{
    success: boolean;
    rtt?: number;
    error?: string;
  } | null>(null);

  // Sync draft whenever remote/stored settings change
  useEffect(() => {
    if (settings?.proxy) {
      setDraft(settings.proxy);
    }
  }, [settings?.proxy]);

  const modeOptions: DropdownOption[] = [
    {
      value: "system",
      label: t("settings.advanced.proxy.mode.system"),
    },
    {
      value: "manual",
      label: t("settings.advanced.proxy.mode.manual"),
    },
    {
      value: "direct",
      label: t("settings.advanced.proxy.mode.direct"),
    },
  ];

  const protocolOptions: DropdownOption[] = [
    {
      value: "http",
      label: t("settings.advanced.proxy.protocol.http"),
    },
    {
      value: "socks5",
      label: t("settings.advanced.proxy.protocol.socks5"),
    },
  ];

  const handleModeChange = useCallback(
    async (modeStr: string) => {
      const newMode = modeStr as ProxyMode;
      const updated: ProxySettingsType = {
        ...draft,
        mode: newMode,
      };
      setDraft(updated);
      setTestResult(null);

      // Automatically save system and direct modes immediately
      if (newMode === "system" || newMode === "direct") {
        try {
          await updateProxySettings(updated);
          toast.success(t("settings.advanced.proxy.save.saved"));
        } catch (err: unknown) {
          const msg = err instanceof Error ? err.message : String(err);
          toast.error(msg);
        }
      }
    },
    [draft, t, updateProxySettings],
  );

  const handleSaveManual = useCallback(async () => {
    const trimmedHost = draft.host.trim();
    if (!trimmedHost) {
      toast.error(t("settings.advanced.proxy.save.emptyHost"));
      return;
    }

    const portNum = Number(draft.port);
    if (!Number.isInteger(portNum) || portNum < 1 || portNum > 65535) {
      toast.error(t("settings.advanced.proxy.save.invalidPort"));
      return;
    }

    const payload: ProxySettingsType = {
      ...draft,
      host: trimmedHost,
      port: portNum,
    };

    setIsSaving(true);
    try {
      await updateProxySettings(payload);
      toast.success(t("settings.advanced.proxy.save.saved"));
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      toast.error(msg);
    } finally {
      setIsSaving(false);
    }
  }, [draft, t, updateProxySettings]);

  const handleTestConnectivity = useCallback(async () => {
    setIsTesting(true);
    setTestResult(null);
    try {
      // Test with current draft configuration
      const rtt = await testProxyConnectivity(draft);
      setTestResult({ success: true, rtt });
      toast.success(t("settings.advanced.proxy.test.success", { ms: rtt }));
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      setTestResult({ success: false, error: msg });
      toast.error(t("settings.advanced.proxy.test.failed", { error: msg }));
    } finally {
      setIsTesting(false);
    }
  }, [draft, t, testProxyConnectivity]);

  return (
    <div className="space-y-3">
      {/* 1. Proxy Mode Selector */}
      <SettingContainer
        title={t("settings.advanced.proxy.mode.title")}
        description={t("settings.advanced.proxy.mode.description")}
        grouped={grouped}
      >
        <Dropdown
          options={modeOptions}
          selectedValue={draft.mode}
          onSelect={handleModeChange}
          className="w-56"
        />
      </SettingContainer>

      {/* 2. Manual Configuration Form (Expanded when mode == manual) */}
      {draft.mode === "manual" && (
        <div className="mx-4 p-4 rounded-lg bg-mid-gray/5 border border-mid-gray/20 space-y-4 transition-all">
          <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
            {/* Protocol */}
            <div className="space-y-1">
              <label className="text-xs font-medium text-text/80">
                {t("settings.advanced.proxy.protocol.title")}
              </label>
              <Dropdown
                options={protocolOptions}
                selectedValue={draft.protocol}
                onSelect={(val) =>
                  setDraft((prev) => ({
                    ...prev,
                    protocol: val as ProxyProtocol,
                  }))
                }
                className="w-full"
              />
            </div>

            {/* Host */}
            <div className="space-y-1 md:col-span-2">
              <label className="text-xs font-medium text-text/80">
                {t("settings.advanced.proxy.host.label")}
              </label>
              <Input
                value={draft.host}
                placeholder={t("settings.advanced.proxy.host.placeholder")}
                onChange={(e) =>
                  setDraft((prev) => ({ ...prev, host: e.target.value }))
                }
                className="w-full"
              />
            </div>
          </div>

          {/* Port */}
          <div className="space-y-1">
            <label className="text-xs font-medium text-text/80">
              {t("settings.advanced.proxy.port.label")}
            </label>
            <Input
              type="number"
              min={1}
              max={65535}
              value={draft.port || ""}
              placeholder={t("settings.advanced.proxy.port.placeholder")}
              onChange={(e) =>
                setDraft((prev) => ({
                  ...prev,
                  port: parseInt(e.target.value, 10) || 0,
                }))
              }
              className="w-48"
            />
          </div>

          {/* Authentication Checkbox */}
          <div className="pt-2 border-t border-mid-gray/10 space-y-3">
            <label className="flex items-center gap-2 cursor-pointer text-xs font-medium text-text/90">
              <input
                type="checkbox"
                checked={draft.auth_enabled}
                onChange={(e) =>
                  setDraft((prev) => ({
                    ...prev,
                    auth_enabled: e.target.checked,
                  }))
                }
                className="rounded border-mid-gray/40 text-logo-primary focus:ring-logo-primary"
              />
              <span>{t("settings.advanced.proxy.auth.label")}</span>
            </label>

            {draft.auth_enabled && (
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3 pl-5 border-l-2 border-logo-primary/30">
                <div className="space-y-1">
                  <label className="text-xs text-text/70">
                    {t("settings.advanced.proxy.auth.username")}
                  </label>
                  <Input
                    value={draft.username ?? ""}
                    placeholder={t(
                      "settings.advanced.proxy.auth.usernamePlaceholder",
                    )}
                    onChange={(e) =>
                      setDraft((prev) => ({
                        ...prev,
                        username: e.target.value || null,
                      }))
                    }
                    className="w-full"
                  />
                </div>
                <div className="space-y-1">
                  <label className="text-xs text-text/70">
                    {t("settings.advanced.proxy.auth.password")}
                  </label>
                  <Input
                    type="password"
                    value={draft.password ?? ""}
                    placeholder={t(
                      "settings.advanced.proxy.auth.passwordPlaceholder",
                    )}
                    onChange={(e) =>
                      setDraft((prev) => ({
                        ...prev,
                        password: e.target.value || null,
                      }))
                    }
                    className="w-full"
                  />
                </div>
              </div>
            )}
          </div>

          {/* Save Button for Manual Mode */}
          <div className="flex justify-end pt-2">
            <Button
              variant="primary"
              size="sm"
              disabled={isSaving}
              onClick={handleSaveManual}
            >
              {isSaving ? (
                <div className="flex items-center gap-2">
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  <span>{t("settings.advanced.proxy.save.button")}</span>
                </div>
              ) : (
                t("settings.advanced.proxy.save.button")
              )}
            </Button>
          </div>
        </div>
      )}

      {/* 3. Connectivity Test Action Bar */}
      <div className="px-4 py-3 bg-mid-gray/5 border-t border-mid-gray/10 flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2 text-xs">
          {testResult ? (
            testResult.success ? (
              <div className="flex items-center gap-1.5 text-emerald-500 font-medium">
                <CheckCircle2 className="w-4 h-4" />
                <span>
                  {t("settings.advanced.proxy.test.success", {
                    ms: testResult.rtt,
                  })}
                </span>
              </div>
            ) : (
              <div className="flex items-center gap-1.5 text-rose-500 font-medium max-w-md truncate">
                <XCircle className="w-4 h-4 shrink-0" />
                <span title={testResult.error}>
                  {t("settings.advanced.proxy.test.failed", {
                    error: testResult.error,
                  })}
                </span>
              </div>
            )
          ) : (
            <div className="flex items-center gap-1.5 text-text/50">
              <Globe className="w-3.5 h-3.5" />
              <span>{t("settings.advanced.proxy.description")}</span>
            </div>
          )}
        </div>

        <Button
          variant="secondary"
          size="sm"
          disabled={isTesting}
          onClick={handleTestConnectivity}
        >
          {isTesting ? (
            <div className="flex items-center gap-1.5">
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
              <span>{t("settings.advanced.proxy.test.testing")}</span>
            </div>
          ) : (
            t("settings.advanced.proxy.test.button")
          )}
        </Button>
      </div>
    </div>
  );
};
