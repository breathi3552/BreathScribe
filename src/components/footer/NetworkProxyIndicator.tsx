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
      ? t("footer.network.direct", "网络: 直连")
      : mode === "manual"
        ? t("footer.network.manual", "网络: 手动代理")
        : t("footer.network.system", "网络: 系统代理");

  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-mid-gray/10 hover:bg-mid-gray/20 text-xs text-text/70 transition-colors border border-mid-gray/20 cursor-pointer ${className}`}
      title={t("settings.advanced.proxy.title", "网络代理")}
    >
      <Globe className="w-3.5 h-3.5 text-text/60 shrink-0" />
      <span className="font-medium text-[11px] whitespace-nowrap">
        {proxyLabel}
      </span>
    </button>
  );
};

export default NetworkProxyIndicator;
