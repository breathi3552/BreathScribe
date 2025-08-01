import React from "react";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface ShowOverlayProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ShowOverlay: React.FC<ShowOverlayProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const showOverlayEnabled = getSetting("show_overlay") ?? true;

  return (
    <ToggleSwitch
      checked={showOverlayEnabled}
      onChange={(enabled) => updateSetting("show_overlay", enabled)}
      isUpdating={isUpdating("show_overlay")}
      label="Show Overlay"
      description="Display visual feedback overlay during recording and transcription"
      descriptionMode={descriptionMode}
      grouped={grouped}
    />
  );
};
