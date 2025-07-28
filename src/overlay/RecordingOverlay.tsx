import React, { useEffect, useState } from "react";
import "./RecordingOverlay.css";
import { listen } from "@tauri-apps/api/event";

type OverlayState = "recording" | "transcribing";

const RecordingOverlay: React.FC = () => {
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");

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

      // Cleanup function
      return () => {
        unlistenShow();
        unlistenHide();
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
      <img width="32" height="32" src={getIconPath()} alt={getIconAlt()} />
      {state === "recording" && (
        <div className="bars-container">
          {Array.from({ length: 8 }, (_, i) => (
            <div
              key={i}
              className="bar"
              style={{
                animationDelay: `${i * 100}ms`,
                animationDuration: `${800 + Math.random() * 400}ms`,
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
