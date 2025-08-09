import React from "react";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { ResetButton } from "../ui/ResetButton";
import { useSettings } from "../../hooks/useSettings";

interface MicrophoneSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const MicrophoneSelector: React.FC<MicrophoneSelectorProps> = React.memo(({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const {
    getSetting,
    updateSetting,
    resetSetting,
    isUpdating,
    isLoading,
    audioDevices,
    refreshAudioDevices,
  } = useSettings();

  const selectedMicrophone = getSetting("selected_microphone") || "Default";

  const handleMicrophoneSelect = async (deviceName: string) => {
    await updateSetting("selected_microphone", deviceName);
  };

  const handleReset = async () => {
    await resetSetting("selected_microphone");
  };

  const microphoneOptions = [
    { value: "Default", label: "Default" },
    ...audioDevices.map(device => ({
      value: device.name,
      label: device.name
    }))
  ];

  return (
    <SettingContainer
      title="Microphone"
      description="Select your preferred microphone device"
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div className="flex items-center space-x-1">
        <Dropdown
          options={microphoneOptions}
          selectedValue={selectedMicrophone}
          onSelect={handleMicrophoneSelect}
          placeholder={isLoading ? "Loading..." : ""}
          disabled={isUpdating("selected_microphone") || isLoading}
          onRefresh={refreshAudioDevices}
        />
        <ResetButton
          onClick={handleReset}
          disabled={isUpdating("selected_microphone") || isLoading}
        />
      </div>
    </SettingContainer>
  );
});
