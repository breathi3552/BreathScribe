import React, { useState } from "react";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";

interface CorrectWordsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const CorrectWords: React.FC<CorrectWordsProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const [newWord, setNewWord] = useState("");

  const correctWords = getSetting("correct_words") || [];

  const handleAddWord = () => {
    const trimmedWord = newWord.trim();
    // Don't allow spaces in words
    if (trimmedWord && !trimmedWord.includes(' ') && !correctWords.includes(trimmedWord)) {
      updateSetting("correct_words", [...correctWords, trimmedWord]);
      setNewWord("");
    }
  };

  const handleRemoveWord = (wordToRemove: string) => {
    updateSetting(
      "correct_words", 
      correctWords.filter((word) => word !== wordToRemove)
    );
  };

  const handleKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      handleAddWord();
    }
  };

  return (
    <div className={`px-4 p-2 ${grouped ? "" : "rounded-lg border border-mid-gray/20"}`}>
      {/* Title, info button, input and add button all in one line */}
      <div className="flex items-center gap-2">
        <h3 className="text-sm font-medium">Custom Words</h3>
        {descriptionMode === "tooltip" && (
          <div title="Add words that are often misheard or misspelled during transcription. The system will automatically correct similar-sounding words to match your list.">
            <svg
              className="w-4 h-4 text-mid-gray cursor-help hover:text-logo-primary transition-colors duration-200 select-none"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
              />
            </svg>
          </div>
        )}
        <input
          type="text"
          value={newWord}
          onChange={(e) => setNewWord(e.target.value)}
          onKeyPress={handleKeyPress}
          placeholder="Add a word"
          className="flex-1 px-2 py-1 text-sm border border-mid-gray/30 rounded focus:outline-none focus:ring-1 focus:ring-logo-primary focus:border-transparent bg-background"
          disabled={isUpdating("correct_words")}
        />
        <button
          onClick={handleAddWord}
          disabled={!newWord.trim() || newWord.includes(' ') || isUpdating("correct_words")}
          className="px-3 py-1 text-xs font-medium text-white bg-logo-primary rounded hover:bg-logo-primary/90 focus:outline-none focus:ring-1 focus:ring-logo-primary disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          Add
        </button>
      </div>

      {/* Words list - flex wrap layout */}
      {correctWords.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1">
          {correctWords.map((word) => (
            <button
              key={word}
              onClick={() => handleRemoveWord(word)}
              disabled={isUpdating("correct_words")}
              className="inline-flex items-center gap-1 px-2 py-1 bg-mid-gray/10 rounded text-xs hover:bg-red-100 hover:text-red-700 focus:outline-none disabled:opacity-50 cursor-pointer transition-colors"
              aria-label={`Remove ${word}`}
            >
              <span>{word}</span>
              <svg
                className="w-3 h-3"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={3}
                  d="M6 18L18 6M6 6l12 12"
                />
              </svg>
            </button>
          ))}
        </div>
      )}
    </div>
  );
};