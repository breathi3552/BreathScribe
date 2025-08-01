import React, { useEffect, useState, useRef } from "react";
import "./RecordingOverlay.css";
import { listen } from "@tauri-apps/api/event";

type OverlayState = "recording" | "transcribing";

const RecordingOverlay: React.FC = () => {
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [levels, setLevels] = useState<number[]>(Array(16).fill(0));
  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));

  useEffect(() => {
    const setupEventListeners = async () => {
      // Listen for show-overlay event from Rust
      const unlistenShow = await listen("show-overlay", (event) => {
        const overlayState = event.payload as OverlayState;
        setState(overlayState);
        setIsVisible(true);
      });

      // Listen for hide-overlay event from Rust
      const unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
      });

      // Listen for mic-level updates
      const unlistenLevel = await listen<number[]>("mic-level", (event) => {
        const newLevels = event.payload as number[];

        // Apply smoothing to reduce jitter
        const smoothed = smoothedLevelsRef.current.map((prev, i) => {
          const target = newLevels[i] || 0;
          return prev * 0.7 + target * 0.3; // Smooth transition
        });

        smoothedLevelsRef.current = smoothed;
        console.log(smoothed.length);
        setLevels(smoothed.slice(0, 10));
      });

      // Cleanup function
      return () => {
        unlistenShow();
        unlistenHide();
        unlistenLevel();
      };
    };

    setupEventListeners();
  }, []);

  const getIconPath = () => {
    return state === "recording"
      ? "/icon/recording.png"
      : "/icon/transcribing.png";
  };

  const getIconAlt = () => {
    return state === "recording" ? "Recording Icon" : "Transcribing Icon";
  };

  return (
    <div className={`recording-overlay ${isVisible ? "fade-in" : ""}`}>
      <img
        width="28"
        height="28"
        src={getIconPath()}
        alt={getIconAlt()}
        style={{}}
      />
      {state === "recording" && (
        <div className="bars-container">
          {levels.map((v, i) => (
            <div
              key={i}
              className="bar"
              style={{
                height: `${4 + Math.pow(v, 0.7) * 32}px`, // Slight curve for better visual
                transition: "height 60ms ease-out",
                opacity: Math.max(0.4, v * 1.7), // Minimum opacity for visibility
              }}
            />
          ))}
        </div>
      )}
      {state === "transcribing" && (
        <div className="transcribing-text">Transcribing...</div>
      )}
    </div>
  );
};

export default RecordingOverlay;
