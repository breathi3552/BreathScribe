import React from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Download, ExternalLink } from "lucide-react";
import { Dialog } from "../ui/Dialog";
import { Button } from "../ui/Button";

export interface AvailableUpdate {
  currentVersion: string;
  latestVersion: string;
  releaseUrl: string;
  releaseNotes: string | null;
  downloadUrl: string | null;
}

interface UpdateModalProps {
  open: boolean;
  update: AvailableUpdate;
  onDismiss: () => void;
}

export const formatVersionTag = (version: string): string => {
  const trimmed = version.trim();
  return trimmed.startsWith("v") || trimmed.startsWith("V")
    ? trimmed
    : `v${trimmed}`;
};

export const UpdateModal: React.FC<UpdateModalProps> = ({
  open,
  update,
  onDismiss,
}) => {
  const { t } = useTranslation();

  const handleOpenRelease = async () => {
    try {
      const targetUrl = update.downloadUrl || update.releaseUrl;
      await openUrl(targetUrl);
    } catch (error) {
      console.error("Failed to open update URL:", error);
    }
  };

  const hasDirectDownload = Boolean(update.downloadUrl);

  return (
    <Dialog
      open={open}
      title={t("footer.newVersionAvailable", {
        version: formatVersionTag(update.latestVersion),
      })}
      description={t("footer.versionComparison", {
        current: formatVersionTag(update.currentVersion),
        latest: formatVersionTag(update.latestVersion),
      })}
      closeLabel={t("common.close")}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) onDismiss();
      }}
      footer={
        <div className="flex justify-end gap-2.5">
          <Button variant="secondary" onClick={onDismiss}>
            {t("common.close")}
          </Button>
          <Button
            variant="primary"
            onClick={handleOpenRelease}
            className="flex items-center gap-1.5"
          >
            {hasDirectDownload ? (
              <>
                <Download className="h-4 w-4" />
                {t("footer.downloadUpdate")}
              </>
            ) : (
              <>
                <ExternalLink className="h-4 w-4" />
                {t("footer.viewRelease")}
              </>
            )}
          </Button>
        </div>
      }
    >
      <div className="space-y-3">
        {update.releaseNotes ? (
          <div className="space-y-1.5">
            <h4 className="text-xs font-semibold uppercase tracking-wider text-text/60">
              {t("footer.releaseNotes")}
            </h4>
            <div className="max-h-60 overflow-y-auto rounded border border-mid-gray/20 bg-background-alt/50 p-3 text-xs text-text/80 whitespace-pre-wrap font-mono leading-relaxed">
              {update.releaseNotes}
            </div>
          </div>
        ) : (
          <p className="text-xs text-text/70">
            {t("footer.noReleaseNotesAvailable")}
          </p>
        )}
      </div>
    </Dialog>
  );
};
