import React from "react";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import ResetIcon from "../icons/ResetIcon";
import { useSettings } from "../../hooks/useSettings";

interface OutputDeviceSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const OutputDeviceSelector: React.FC<OutputDeviceSelectorProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const {
    getSetting,
    updateSetting,
    resetSetting,
    isUpdating,
    isLoading,
    outputDevices,
    refreshOutputDevices,
  } = useSettings();

  const selectedOutputDevice =
    getSetting("selected_output_device") || "Default";

  const handleOutputDeviceSelect = async (deviceName: string) => {
    await updateSetting("selected_output_device", deviceName);
    console.log(
      `Output device changed to: ${outputDevices.find((d) => d.name === deviceName)?.name}`,
    );
  };

  const handleReset = async () => {
    await resetSetting("selected_output_device");
    console.log("Output device reset to default");
  };

  const outputDeviceOptions = outputDevices.map(device => ({
    value: device.name,
    label: device.name
  }));

  return (
    <SettingContainer
      title="Output Device"
      description="Select your preferred audio output device for feedback sounds"
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div className="flex items-center space-x-1">
        <Dropdown
          options={outputDeviceOptions}
          selectedValue={selectedOutputDevice}
          onSelect={handleOutputDeviceSelect}
          placeholder={isLoading ? "Loading..." : "Select output device..."}
          disabled={isUpdating("selected_output_device") || isLoading}
          onRefresh={refreshOutputDevices}
        />
        <button
          className="px-2 py-1 hover:bg-logo-primary/30 active:bg-logo-primary/50 active:scale-95 rounded fill-text hover:cursor-pointer hover:border-logo-primary border border-transparent transition-all duration-150"
          onClick={handleReset}
          disabled={isUpdating("selected_output_device") || isLoading}
        >
          <ResetIcon className="" />
        </button>
      </div>
      {isUpdating("selected_output_device") && (
        <div className="absolute inset-0 bg-mid-gray/10 rounded flex items-center justify-center">
          <div className="w-4 h-4 border-2 border-logo-primary border-t-transparent rounded-full animate-spin"></div>
        </div>
      )}
    </SettingContainer>
  );
};
