import React from "react";
import { Slider } from "../../ui/Slider";
import { useSettings } from "../../../hooks/useSettings";

interface WordCorrectionThresholdProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const WordCorrectionThreshold: React.FC<
  WordCorrectionThresholdProps
> = ({ descriptionMode = "tooltip", grouped = false }) => {
  const { settings, updateSetting } = useSettings();

  const handleThresholdChange = (value: number) => {
    updateSetting("word_correction_threshold", value);
  };

  return (
    <Slider
      value={settings?.word_correction_threshold ?? 0.2}
      onChange={handleThresholdChange}
      min={0.0}
      max={1.0}
      label="Word Correction Threshold"
      description="Controls how aggressively custom words are applied. Lower values mean more corrections will be made, higher values mean fewer corrections. Range: 0 (most aggressive) to 1 (least aggressive)."
      descriptionMode={descriptionMode}
      grouped={grouped}
    />
  );
};
