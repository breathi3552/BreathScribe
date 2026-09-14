import React, { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { commands, type UpdateCheckResponse } from "../../bindings";
import { useSettings } from "../../hooks/useSettings";
import { UpdateModal, type AvailableUpdate } from "./UpdateModal";

interface UpdateCheckerProps {
  className?: string;
}

const UpdateChecker: React.FC<UpdateCheckerProps> = ({ className = "" }) => {
  const { t } = useTranslation();
  const [isChecking, setIsChecking] = useState(false);
  const [showUpToDate, setShowUpToDate] = useState(false);
  const [showCheckFailed, setShowCheckFailed] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [updateInfo, setUpdateInfo] = useState<AvailableUpdate | null>(null);
  const [isModalOpen, setIsModalOpen] = useState(false);

  const { settings, isLoading, updateChecksLocked } = useSettings();

  const settingsLoaded =
    !isLoading && settings !== null && updateChecksLocked !== null;

  const updateChecksEnabled =
    (settings?.update_checks_enabled ?? false) && updateChecksLocked === false;

  const feedbackTimeoutRef = useRef<number | undefined>(undefined);
  const isManualCheckRef = useRef(false);

  const setTemporaryFeedback = (
    setter: (val: boolean) => void,
    delayMs: number,
  ) => {
    setter(true);
    clearTimeout(feedbackTimeoutRef.current);
    feedbackTimeoutRef.current = window.setTimeout(() => {
      setter(false);
    }, delayMs);
  };

  const handleCheckError = (message: string) => {
    console.error("Failed to check for updates:", message);
    setErrorMessage(message);
    if (isManualCheckRef.current) {
      setTemporaryFeedback(setShowCheckFailed, 3500);
    }
  };

  useEffect(() => {
    if (!settingsLoaded) return;

    if (!updateChecksEnabled) {
      clearTimeout(feedbackTimeoutRef.current);
      setIsChecking(false);
      setShowUpToDate(false);
      setShowCheckFailed(false);
      setUpdateInfo(null);
      setIsModalOpen(false);
      return;
    }

    // Run initial background update check on load
    checkForUpdates();

    // Listen for external trigger (e.g. system tray menu click)
    const updateUnlisten = listen("check-for-updates", () => {
      handleManualUpdateCheck();
    });

    return () => {
      clearTimeout(feedbackTimeoutRef.current);
      updateUnlisten.then((fn) => fn());
    };
  }, [settingsLoaded, updateChecksEnabled]);

  const checkForUpdates = async () => {
    if (!updateChecksEnabled || isChecking) return;

    try {
      setIsChecking(true);
      setShowUpToDate(false);
      setShowCheckFailed(false);
      setErrorMessage(null);

      const res = await commands.checkForUpdates();

      if (res.status === "error") {
        handleCheckError(res.error);
        return;
      }

      const data: UpdateCheckResponse = res.data;

      switch (data.status) {
        case "update_available": {
          const available: AvailableUpdate = {
            currentVersion: data.current_version,
            latestVersion: data.latest_version,
            releaseUrl: data.release_url,
            releaseNotes: data.release_notes,
            downloadUrl: data.download_url,
          };
          setUpdateInfo(available);
          setShowUpToDate(false);
          setShowCheckFailed(false);
          if (isManualCheckRef.current) {
            setIsModalOpen(true);
          }
          break;
        }

        case "up_to_date": {
          setUpdateInfo(null);
          setShowCheckFailed(false);
          if (isManualCheckRef.current) {
            setTemporaryFeedback(setShowUpToDate, 3000);
          }
          break;
        }

        case "error": {
          handleCheckError(data.message);
          break;
        }
      }
    } catch (error) {
      const errStr = error instanceof Error ? error.message : String(error);
      handleCheckError(errStr);
    } finally {
      setIsChecking(false);
      isManualCheckRef.current = false;
    }
  };

  const handleManualUpdateCheck = () => {
    if (!updateChecksEnabled || isChecking) return;
    isManualCheckRef.current = true;
    checkForUpdates();
  };

  const getUpdateStatusText = () => {
    if (!updateChecksEnabled) {
      return t("footer.updateCheckingDisabled");
    }
    if (isChecking) return t("footer.checkingUpdates");
    if (showCheckFailed) return t("footer.checkFailed");
    if (showUpToDate) return t("footer.upToDate");
    if (updateInfo !== null) return t("footer.updateAvailableShort");
    return t("footer.checkForUpdates");
  };

  const getUpdateStatusAction = () => {
    if (!updateChecksEnabled) return undefined;
    if (updateInfo !== null) return () => setIsModalOpen(true);
    if (!isChecking) return handleManualUpdateCheck;
    return undefined;
  };

  const isUpdateDisabled = !updateChecksEnabled || isChecking;
  const isUpdateClickable =
    !isUpdateDisabled &&
    (updateInfo !== null || (!isChecking && !showUpToDate));

  return (
    <>
      <div className={`flex items-center gap-3 ${className}`}>
        {isUpdateClickable ? (
          <button
            onClick={getUpdateStatusAction()}
            disabled={isUpdateDisabled}
            title={errorMessage ?? undefined}
            className={`transition-colors disabled:opacity-50 tabular-nums ${
              updateInfo !== null
                ? "text-logo-primary hover:text-logo-primary/80 font-medium"
                : showCheckFailed
                  ? "text-warning hover:text-warning/80"
                  : "text-text/60 hover:text-text/80"
            }`}
          >
            {getUpdateStatusText()}
          </button>
        ) : (
          <span
            className={`tabular-nums ${
              showCheckFailed ? "text-warning" : "text-text/60"
            }`}
            title={errorMessage ?? undefined}
          >
            {getUpdateStatusText()}
          </span>
        )}
      </div>

      {updateInfo && (
        <UpdateModal
          open={isModalOpen}
          update={updateInfo}
          onDismiss={() => setIsModalOpen(false)}
        />
      )}
    </>
  );
};

export default UpdateChecker;
