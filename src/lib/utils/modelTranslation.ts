import type { TFunction } from "i18next";
import type { ModelInfo } from "@/bindings";

/**
 * Get the translated name for a model
 * @param model - The model info object
 * @param t - The translation function from useTranslation
 * @returns The translated model name, or the original name if no translation exists
 */
export function getTranslatedModelName(model: ModelInfo, t: TFunction): string {
  const translationKey = `onboarding.models.${model.id}.name`;
  const translated = t(translationKey, { defaultValue: "" });
  return translated !== "" ? translated : model.name;
}

/**
 * Get the translated description for a model
 * @param model - The model info object
 * @param t - The translation function from useTranslation
 * @returns The translated model description, or the original description if no translation exists
 */
export function getTranslatedModelDescription(
  model: ModelInfo,
  t: TFunction,
): string {
  // Custom models use a generic translation key
  if (model.is_custom) {
    return t("onboarding.customModelDescription");
  }
  const translationKey = `onboarding.models.${model.id}.description`;
  const translated = t(translationKey, { defaultValue: "" });
  return translated !== "" ? translated : model.description;
}

/**
 * Format a cloud model ID into a user-friendly display name
 * @param modelId - The cloud model ID (e.g. "gemini-2.5-flash")
 * @returns Human-readable cloud model name
 */
export function formatCloudModelName(modelId: string): string {
  switch (modelId) {
    case "gemini-3.5-transcribe-live":
      return "Gemini 3.5 Transcribe Live";
    case "gemini-3.5-transcribe":
      return "Gemini 3.5 Transcribe";
    case "gemini-3.6-flash":
      return "Gemini 3.6 Flash";
    case "gemini-3.5-flash":
      return "Gemini 3.5 Flash";
    case "gemini-2.5-flash":
      return "Gemini 2.5 Flash";
    case "gemini-2.5-pro":
      return "Gemini 2.5 Pro";
    default:
      return modelId;
  }
}
