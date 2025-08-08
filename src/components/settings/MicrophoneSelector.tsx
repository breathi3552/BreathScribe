import React from "react";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import ResetIcon from "../icons/ResetIcon";
import { useSettings } from "../../hooks/useSettings";

// Simple refresh icon component
const RefreshIcon: React.FC<{ className?: string }> = ({ className = "" }) => (
  <svg
    className={`w-4 h-4 ${className}`}
    fill="none"
    stroke="currentColor"
    viewBox="0 0 24 24"
    xmlns="http://www.w3.org/2000/svg"
  >
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={2}
      d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
    />
  </svg>
);

interface MicrophoneSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const MicrophoneSelector: React.FC<MicrophoneSelectorProps> = ({
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
    console.log(
      `Microphone changed to: ${audioDevices.find((d) => d.name === deviceName)?.name}`,
    );
  };

  const handleReset = async () => {
    await resetSetting("selected_microphone");
    console.log("Microphone reset to default");
  };

  const microphoneOptions = audioDevices.map(device => ({
    value: device.name,
    label: device.name
  }));

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
          placeholder={isLoading ? "Loading..." : "Select microphone..."}
          disabled={isUpdating("selected_microphone") || isLoading}
          onRefresh={refreshAudioDevices}
        />
        <button
          className="px-2 py-1 hover:bg-logo-primary/30 active:bg-logo-primary/50 active:scale-95 rounded fill-text hover:cursor-pointer hover:border-logo-primary border border-transparent transition-all duration-150"
          onClick={handleReset}
          disabled={isUpdating("selected_microphone") || isLoading}
        >
          <ResetIcon className="" />
        </button>
      </div>
      {isUpdating("selected_microphone") && (
        <div className="absolute inset-0 bg-mid-gray/10 rounded flex items-center justify-center">
          <div className="w-4 h-4 border-2 border-logo-primary border-t-transparent rounded-full animate-spin"></div>
        </div>
      )}
    </SettingContainer>
  );
};
