import React from "react";
import { useTranslation } from "react-i18next";
import { Globe } from "lucide-react";
import { useSettingsStore } from "@/stores/settingsStore";

interface NetworkProxyIndicatorProps {
  onClick?: () => void;
  className?: string;
}

export const NetworkProxyIndicator: React.FC<NetworkProxyIndicatorProps> = ({
  onClick,
  className = "",
}) => {
  const { t } = useTranslation();
  const proxy = useSettingsStore((state) => state.settings?.proxy);

  const mode = proxy?.mode || "system";
  const proxyLabel =
    mode === "direct"
      ? t("footer.network.direct")
      : mode === "manual"
        ? t("footer.network.manual")
        : t("footer.network.system");

  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-mid-gray/10 hover:bg-mid-gray/20 text-xs text-text/70 transition-colors border border-mid-gray/20 cursor-pointer ${className}`}
      title={t("settings.advanced.proxy.title")}
    >
      <Globe className="w-3.5 h-3.5 text-text/60 shrink-0" />
      <span className="font-medium text-[11px] whitespace-nowrap">
        {proxyLabel}
      </span>
    </button>
  );
};

export default NetworkProxyIndicator;
