import React, { useEffect, useState, useRef } from "react";
import { BindingResponseSchema, ShortcutBindingsMap } from "../../lib/types";
import { type } from "@tauri-apps/plugin-os";
import { getKeyName } from "../../lib/utils/keyboard";
import ResetIcon from "../icons/ResetIcon";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { invoke } from "@tauri-apps/api/core";

interface HandyShortcutProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const HandyShortcut: React.FC<HandyShortcutProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateBinding, resetBinding, isUpdating, isLoading } =
    useSettings();
  const [keyPressed, setKeyPressed] = useState<string[]>([]);
  const [recordedKeys, setRecordedKeys] = useState<string[]>([]);
  const [editingShortcutId, setEditingShortcutId] = useState<string | null>(
    null,
  );
  const [originalBinding, setOriginalBinding] = useState<string>("");
  const [isMacOS, setIsMacOS] = useState<boolean>(false);
  const shortcutRefs = useRef<Map<string, HTMLDivElement | null>>(new Map());

  const bindings = getSetting("bindings") || {};

  // Check if running on macOS
  useEffect(() => {
    const checkOsType = async () => {
      try {
        const osType = await type();
        setIsMacOS(osType === "macos");
      } catch (error) {
        console.error("Error detecting OS type:", error);
        setIsMacOS(false);
      }
    };

    checkOsType();
  }, []);

  // Normalize modifier keys (unify left/right variants)
  const normalizeKey = (key: string): string => {
    // Handle left/right variants of modifier keys
    if (key.startsWith("left ") || key.startsWith("right ")) {
      const parts = key.split(" ");
      if (parts.length === 2) {
        // Return just the modifier name without left/right prefix
        return parts[1];
      }
    }
    return key;
  };

  // Format keys for macOS display
  const formatMacOSKeys = (key: string): string => {
    if (!isMacOS) return key; // Only format for macOS

    const keyMap: Record<string, string> = {
      alt: "option",
    };

    return keyMap[key.toLowerCase()] || key;
  };

  // Format a key combination for display
  const formatKeyCombination = (combination: string): string => {
    if (!isMacOS) return combination; // Only format for macOS

    return combination.split("+").map(formatMacOSKeys).join(" + ");
  };

  useEffect(() => {
    // Only add event listeners when we're in editing mode
    if (editingShortcutId === null) return;

    console.log("keyPressed", keyPressed);

    // Keyboard event listeners
    const handleKeyDown = async (e: KeyboardEvent) => {
      if (e.repeat) return; // ignore auto-repeat
      if (e.key === "Escape") {
        // Cancel recording
        if (editingShortcutId) {
          await invoke("resume_binding", { id: editingShortcutId }).catch(
            console.error,
          );
        }
        setEditingShortcutId(null);
        setKeyPressed([]);
        setRecordedKeys([]);
        setOriginalBinding("");
        return;
      }
      e.preventDefault();

      // Get the key and normalize it (unify left/right modifiers)
      const rawKey = getKeyName(e);
      const key = normalizeKey(rawKey);

      console.log("You pressed", rawKey, "normalized to", key);

      if (!keyPressed.includes(key)) {
        setKeyPressed((prev) => [...prev, key]);
        // Also add to recorded keys if not already there
        if (!recordedKeys.includes(key)) {
          setRecordedKeys((prev) => [...prev, key]);
        }
      }
    };

    const handleKeyUp = async (e: KeyboardEvent) => {
      e.preventDefault();

      // Get the key and normalize it
      const rawKey = getKeyName(e);
      const key = normalizeKey(rawKey);

      // Remove from currently pressed keys
      setKeyPressed((prev) => prev.filter((k) => k !== key));

      // If no keys are pressed anymore, commit the shortcut
      const updatedKeyPressed = keyPressed.filter((k) => k !== key);
      if (updatedKeyPressed.length === 0 && recordedKeys.length > 0) {
        // Create the shortcut string from all recorded keys
        const newShortcut = recordedKeys.join("+");

        if (editingShortcutId && bindings[editingShortcutId]) {
          try {
            await updateBinding(editingShortcutId, newShortcut);
            // Re-register the shortcut now that recording is finished
            await invoke("resume_binding", { id: editingShortcutId }).catch(
              console.error,
            );
          } catch (error) {
            console.error("Failed to change binding:", error);
          }

          // Exit editing mode and reset states
          setEditingShortcutId(null);
          setKeyPressed([]);
          setRecordedKeys([]);
          setOriginalBinding("");
        }
      }
    };

    // Add click outside handler
    const handleClickOutside = (e: MouseEvent) => {
      const activeElement = shortcutRefs.current.get(editingShortcutId);
      if (activeElement && !activeElement.contains(e.target as Node)) {
        // Cancel shortcut recording - the hook will handle rollback
        if (editingShortcutId) {
          invoke("resume_binding", { id: editingShortcutId }).catch(
            console.error,
          );
        }
        setEditingShortcutId(null);
        setKeyPressed([]);
        setRecordedKeys([]);
        setOriginalBinding("");
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    window.addEventListener("click", handleClickOutside);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
      window.removeEventListener("click", handleClickOutside);
    };
  }, [
    keyPressed,
    recordedKeys,
    editingShortcutId,
    bindings,
    originalBinding,
    updateBinding,
  ]);

  // Start recording a new shortcut
  const startRecording = async (id: string) => {
    if (editingShortcutId === id) return; // Already editing this shortcut

    // Suspend current binding to avoid firing while recording
    await invoke("suspend_binding", { id }).catch(console.error);

    // Store the original binding to restore if canceled
    setOriginalBinding(bindings[id]?.current_binding || "");
    setEditingShortcutId(id);
    setKeyPressed([]);
    setRecordedKeys([]);
  };

  // Format the current shortcut keys being recorded
  const formatCurrentKeys = () => {
    if (recordedKeys.length === 0) return "Press keys...";

    if (!isMacOS) {
      return recordedKeys.join("+");
    }

    // Map each key to its macOS-friendly name for display
    return recordedKeys.map(formatMacOSKeys).join(" + ");
  };

  // Store references to shortcut elements
  const setShortcutRef = (id: string, ref: HTMLDivElement | null) => {
    shortcutRefs.current.set(id, ref);
  };

  // If still loading, show loading state
  if (isLoading) {
    return (
      <SettingContainer
        title="Handy Shortcuts"
        description="Configure keyboard shortcuts to trigger speech-to-text recording"
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-mid-gray">Loading shortcuts...</div>
      </SettingContainer>
    );
  }

  // If no bindings are loaded, show empty state
  if (Object.keys(bindings).length === 0) {
    return (
      <SettingContainer
        title="Handy Shortcuts"
        description="Configure keyboard shortcuts to trigger speech-to-text recording"
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-mid-gray">No shortcuts configured</div>
      </SettingContainer>
    );
  }

  return (
    <SettingContainer
      title="Handy Shortcut"
      description="Set the keyboard shortcut to start and stop speech-to-text recording"
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      {(() => {
        const primaryBinding = Object.values(bindings)[0];
        const primaryId = Object.keys(bindings)[0];

        if (!primaryBinding) {
          return (
            <div className="text-sm text-mid-gray">No shortcuts configured</div>
          );
        }

        return (
          <div className="flex items-center space-x-1">
            {editingShortcutId === primaryId ? (
              <div
                ref={(ref) => setShortcutRef(primaryId, ref)}
                className="px-2 py-1 text-sm font-semibold border border-logo-primary bg-logo-primary/30 rounded min-w-[120px] text-center"
              >
                {formatCurrentKeys()}
              </div>
            ) : (
              <div
                className="px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 hover:bg-logo-primary/10 rounded cursor-pointer hover:border-logo-primary"
                onClick={() => startRecording(primaryId)}
              >
                {formatKeyCombination(primaryBinding.current_binding)}
              </div>
            )}
            <button
              className="px-2 py-1 hover:bg-logo-primary/30 active:bg-logo-primary/50 active:scale-95 rounded fill-text hover:cursor-pointer hover:border-logo-primary border border-transparent transition-all duration-150"
              onClick={() => resetBinding(primaryId)}
              disabled={isUpdating(`binding_${primaryId}`)}
            >
              <ResetIcon className="" />
            </button>
          </div>
        );
      })()}
    </SettingContainer>
  );
};
