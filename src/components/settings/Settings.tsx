import React from "react";
import { MicrophoneSelector } from "./MicrophoneSelector";
import { AlwaysOnMicrophone } from "./AlwaysOnMicrophone";
import { PushToTalk } from "./PushToTalk";
import { AudioFeedback } from "./AudioFeedback";
import { OutputDeviceSelector } from "./OutputDeviceSelector";
import { ShowOverlay } from "./ShowOverlay";
import { HandyShortcut } from "./HandyShortcut";
import { TranslateToEnglish } from "./TranslateToEnglish";
import { LanguageSelector } from "./LanguageSelector";
import { SettingsGroup } from "../ui/SettingsGroup";

export const Settings: React.FC = () => {
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup>
        <HandyShortcut descriptionMode="tooltip" grouped={true} />
        <MicrophoneSelector descriptionMode="tooltip" grouped={true} />
        <LanguageSelector descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>

      <SettingsGroup title="Advanced">
        <PushToTalk descriptionMode="tooltip" grouped={true} />
        <AudioFeedback descriptionMode="tooltip" grouped={true} />
        <OutputDeviceSelector descriptionMode="tooltip" grouped={true} />
        <ShowOverlay descriptionMode="tooltip" grouped={true} />
        <TranslateToEnglish descriptionMode="tooltip" grouped={true} />
        <AlwaysOnMicrophone descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>
    </div>
  );
};
