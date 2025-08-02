import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { TextDisplay, SettingsGroup } from "../../ui";

interface DebugPathsProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const DebugPaths: React.FC<DebugPathsProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  const [appDirPath, setAppDirPath] = useState<string>("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const loadPaths = async () => {
      try {
        const result = await invoke<string>("get_app_dir_path");
        setAppDirPath(result);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to load paths");
      } finally {
        setLoading(false);
      }
    };

    loadPaths();
  }, []);

  const handleCopy = (value: string) => {
    // Could add a toast notification here if desired
    console.log("Copied to clipboard:", value);
  };

  if (loading) {
    return (
      <div className="space-y-4">
        <div className="animate-pulse">
          <div className="h-4 bg-gray-200 rounded w-1/3 mb-2"></div>
          <div className="h-8 bg-gray-100 rounded"></div>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 bg-red-50 border border-red-200 rounded-lg">
        <p className="text-red-600 text-sm">Error loading paths: {error}</p>
      </div>
    );
  }

  return (
    <TextDisplay
      label="App Data Directory"
      description="Main directory where application data, settings, and models are stored"
      value={appDirPath}
      descriptionMode={descriptionMode}
      grouped={true}
      copyable={true}
      monospace={true}
      onCopy={handleCopy}
    />
  );
};
