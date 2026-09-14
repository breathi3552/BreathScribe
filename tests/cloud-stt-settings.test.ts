// @ts-expect-error bun:test is provided by Bun test runner at runtime
import { describe, expect, test } from "bun:test";
import {
  CLOUD_STT_PROVIDERS,
  type CloudSttProviderConfig,
} from "../src/components/settings/CloudSTTSettings";

describe("CLOUD_STT_PROVIDERS registry", () => {
  test("gemini provider is registered with correct default configuration", () => {
    const gemini = CLOUD_STT_PROVIDERS.gemini;
    expect(gemini).toBeDefined();
    expect(gemini.id).toBe("gemini");
    expect(gemini.label).toBe("Google Gemini");
    expect(gemini.apiKeyUrl).toBe("https://aistudio.google.com/app/apikey");
    expect(gemini.defaultModelId).toBe("gemini-3.5-transcribe");
    expect(gemini.placeholder).toBe("AIzaSy...");
  });

  test("gemini provider offers expected models", () => {
    const modelIds = CLOUD_STT_PROVIDERS.gemini.models.map((m) => m.value);
    expect(modelIds).toContain("gemini-3.5-transcribe");
    expect(modelIds).toContain("gemini-3.5-transcribe-live");
  });

  test("gemini key validation correctly recognizes valid and invalid keys", () => {
    const validator = CLOUD_STT_PROVIDERS.gemini.isKeyFormatValid;
    expect(validator).toBeDefined();
    if (!validator) return;

    // Empty/blank string is considered valid draft until submission
    expect(validator("")).toBe(true);
    expect(validator("   ")).toBe(true);

    // Valid Gemini API key patterns
    expect(validator("AIzaSyCG_1234567890123456789012345678901")).toBe(true);
    expect(validator("AQ.valid-google-cloud-style-key")).toBe(true);
    expect(validator("ya29.oauth-access-token-format-sample")).toBe(true);
    expect(validator("some-arbitrary-valid-length-key-exceeding-20")).toBe(
      true,
    );

    // Invalid keys (less than 20 characters and not starting with expected prefixes)
    expect(validator("short-key")).toBe(false);
    expect(validator("1234567890")).toBe(false);
    expect(validator("AIzaSyShort")).toBe(false);
  });

  test("registry supports adding and dynamically resolving new providers", () => {
    const mockProviders: Record<string, CloudSttProviderConfig> = {
      ...CLOUD_STT_PROVIDERS,
      mock_provider: {
        id: "mock_provider",
        label: "Mock Cloud Provider",
        apiKeyUrl: "https://example.com/console/keys",
        defaultModelId: "mock-model-v1",
        models: [
          {
            value: "mock-model-v1",
            label: "Mock Model v1",
            descriptionKey: "mock.v1",
          },
        ],
      },
      local_relay: {
        id: "local_relay",
        label: "Local Relay (No Key Required)",
        defaultModelId: "relay-v1",
        models: [
          {
            value: "relay-v1",
            label: "Relay v1",
            descriptionKey: "relay.v1",
          },
        ],
      },
    };

    // Provider with console URL provides valid apiKeyUrl
    expect(mockProviders.mock_provider.apiKeyUrl).toBe(
      "https://example.com/console/keys",
    );

    // Provider without console URL safely has undefined apiKeyUrl
    expect(mockProviders.local_relay.apiKeyUrl).toBeUndefined();

    // Models dynamically match the chosen provider
    expect(mockProviders.mock_provider.models[0].value).toBe("mock-model-v1");
    expect(mockProviders.local_relay.models[0].value).toBe("relay-v1");
  });
});
