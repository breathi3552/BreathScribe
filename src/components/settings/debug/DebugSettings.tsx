import React from "react";
import { DebugPaths } from "./DebugPaths";

interface DebugSettingsProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const DebugSettings: React.FC<DebugSettingsProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  return (
    <div className="space-y-6">
      {descriptionMode === "inline" && !grouped && (
        <div className="mb-6">
          <div className="flex items-center space-x-2 mb-2">
            <h2 className="text-xl font-semibold text-gray-900">Debug</h2>
          </div>
          <p className="text-sm text-gray-600">
            Debug information and developer tools. These settings are only
            visible when debug mode is enabled.
          </p>
        </div>
      )}

      <DebugPaths descriptionMode={descriptionMode} grouped={true} />
    </div>
  );
};
