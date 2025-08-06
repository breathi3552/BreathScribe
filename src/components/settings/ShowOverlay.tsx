import React from "react";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";

interface ShowOverlayProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const overlayOptions = [
  { value: "none", label: "Do not show" },
  { value: "bottom", label: "On bottom" },
  { value: "top", label: "On top" }
];

export const ShowOverlay: React.FC<ShowOverlayProps> = ({
  descriptionMode = "tooltip",
  grouped = false
}) => {
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const selectedPosition = getSetting("overlay_position") || "bottom";

  return (
    <SettingContainer
      title="Show Overlay"
      description="Display visual feedback overlay during recording and transcription"
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Dropdown
        options={overlayOptions}
        selectedValue={selectedPosition}
        onSelect={(value) => updateSetting("overlay_position", value)}
        disabled={isUpdating("overlay_position")}
      />
    </SettingContainer>
  );
};
