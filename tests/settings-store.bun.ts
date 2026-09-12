import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import type { AppSettings } from "../src/bindings";

type Unlisten = () => void;
type EventCallback<T> = (event: { payload: T }) => void;

type PendingRegistration = {
  eventName: string;
  callback: (event: { payload: unknown }) => void;
  resolve: (unlisten: Unlisten) => void;
  reject: (error: Error) => void;
  unlisten: Unlisten;
  settled: boolean;
  removed: boolean;
};

const registrations: PendingRegistration[] = [];
const registrationFailures = new Set<string>();
let unlistenCalls = 0;

const listenMock = <T>(
  eventName: string,
  callback: EventCallback<T>,
): Promise<Unlisten> => {
  let resolveRegistration!: (unlisten: Unlisten) => void;
  let rejectRegistration!: (error: Error) => void;
  const registration: PendingRegistration = {
    eventName,
    callback: callback as EventCallback<unknown>,
    resolve: (unlisten) => resolveRegistration(unlisten),
    reject: (error) => rejectRegistration(error),
    unlisten: () => {
      if (!registration.removed) {
        registration.removed = true;
        unlistenCalls += 1;
      }
    },
    settled: false,
    removed: false,
  };

  const promise = new Promise<Unlisten>((resolve, reject) => {
    resolveRegistration = resolve;
    rejectRegistration = reject;
  });

  registrations.push(registration);
  if (registrationFailures.has(eventName)) {
    registration.settled = true;
    registration.reject(new Error(`failed to listen: ${eventName}`));
  }

  return promise;
};

mock.module("@tauri-apps/api/event", () => ({ listen: listenMock }));

const { commands } = await import("../src/bindings");
const { useSettingsStore } = await import("../src/stores/settingsStore");
type SettingsResult = Awaited<ReturnType<typeof commands.getAppSettings>>;

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve };
}

function settings(
  selectedModel: string,
  overrides: Partial<AppSettings> = {},
): AppSettings {
  return { selected_model: selectedModel, ...overrides };
}

function ok(data: AppSettings): SettingsResult {
  return { status: "ok", data };
}

function emit(eventName: string, payload: unknown) {
  for (const registration of registrations) {
    if (
      registration.eventName === eventName &&
      registration.settled &&
      !registration.removed
    ) {
      registration.callback({ payload });
    }
  }
}

function resolvePendingListeners() {
  for (const registration of registrations) {
    if (
      !registration.settled &&
      !registrationFailures.has(registration.eventName)
    ) {
      registration.settled = true;
      registration.resolve(registration.unlisten);
    }
  }
}

async function waitFor(condition: () => boolean) {
  const deadline = Date.now() + 1000;
  while (!condition()) {
    if (Date.now() >= deadline) {
      throw new Error("Timed out waiting for condition");
    }
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

let appSettingsResponder: () => Promise<SettingsResult>;
let appSettingsCalls = 0;
let defaultSettingsCalls = 0;
let customSoundsCalls = 0;
let updateLockCalls = 0;
let microphoneCalls = 0;

beforeEach(async () => {
  resolvePendingListeners();
  await useSettingsStore.getState().dispose();
  registrations.length = 0;
  registrationFailures.clear();
  unlistenCalls = 0;
  appSettingsCalls = 0;
  defaultSettingsCalls = 0;
  customSoundsCalls = 0;
  updateLockCalls = 0;
  microphoneCalls = 0;
  appSettingsResponder = async () => ok(settings("initial"));

  commands.getAppSettings = () => {
    appSettingsCalls += 1;
    return appSettingsResponder();
  };
  commands.getDefaultSettings = async () => {
    defaultSettingsCalls += 1;
    return ok(settings("default"));
  };
  commands.checkCustomSounds = async () => {
    customSoundsCalls += 1;
    return { start: false, stop: false };
  };
  commands.isUpdateChecksLocked = async () => {
    updateLockCalls += 1;
    return false;
  };
  commands.getAvailableMicrophones = async () => {
    microphoneCalls += 1;
    return { status: "ok", data: [] };
  };
});

afterEach(async () => {
  resolvePendingListeners();
  await useSettingsStore.getState().dispose();
});

describe("settings store synchronization", () => {
  test("shares initialization and registers listeners before the first read", async () => {
    const initialization = useSettingsStore.getState().initialize();
    const duplicateInitialization = useSettingsStore.getState().initialize();

    expect(initialization).toBe(duplicateInitialization);
    expect(registrations.map(({ eventName }) => eventName)).toEqual([
      "model-state-changed",
      "settings-changed",
    ]);
    expect(appSettingsCalls).toBe(0);

    resolvePendingListeners();
    await initialization;

    expect(appSettingsCalls).toBe(1);
    expect(defaultSettingsCalls).toBe(1);
    expect(customSoundsCalls).toBe(1);
    expect(updateLockCalls).toBe(1);
    expect(useSettingsStore.getState().settings?.selected_model).toBe(
      "initial",
    );
    expect(useSettingsStore.getState().initialize()).toBe(initialization);
  });

  test("refreshes again when a setting changes during the initial read", async () => {
    const firstRead = deferred<SettingsResult>();
    const secondRead = deferred<SettingsResult>();
    appSettingsResponder = () =>
      appSettingsCalls === 1 ? firstRead.promise : secondRead.promise;

    const initialization = useSettingsStore.getState().initialize();
    resolvePendingListeners();
    await waitFor(() => appSettingsCalls === 1);

    emit("settings-changed", { setting: "selected_model" });
    firstRead.resolve(
      ok(
        settings("old", {
          selected_language: "en",
          transcription_mode: { type: "local" },
        }),
      ),
    );
    await waitFor(() => appSettingsCalls === 2);
    secondRead.resolve(
      ok(
        settings("latest", {
          selected_language: "zh",
          transcription_mode: {
            type: "cloud",
            config: { provider_id: "gemini", model_id: "latest" },
          },
        }),
      ),
    );

    await initialization;
    expect(useSettingsStore.getState().settings).toMatchObject({
      selected_model: "latest",
      selected_language: "zh",
      transcription_mode: {
        type: "cloud",
        config: { provider_id: "gemini", model_id: "latest" },
      },
    });
  });

  test("waits for the latest explicit and event-triggered refresh", async () => {
    const initialization = useSettingsStore.getState().initialize();
    resolvePendingListeners();
    await initialization;

    appSettingsCalls = 0;
    const firstRead = deferred<SettingsResult>();
    const secondRead = deferred<SettingsResult>();
    appSettingsResponder = () =>
      appSettingsCalls === 1 ? firstRead.promise : secondRead.promise;

    const refresh = useSettingsStore.getState().refreshSettings();
    await waitFor(() => appSettingsCalls === 1);
    const waitingRefresh = useSettingsStore.getState().refreshSettings();
    emit("model-state-changed", undefined);
    emit("settings-changed", { setting: "selected_language" });

    firstRead.resolve(ok(settings("old")));
    await waitFor(() => appSettingsCalls === 2);
    secondRead.resolve(
      ok(
        settings("latest", {
          selected_language: "ja",
          selected_microphone: "Desk Mic",
          selected_output_device: "Headphones",
        }),
      ),
    );

    await Promise.all([refresh, waitingRefresh]);
    expect(useSettingsStore.getState().settings).toMatchObject({
      selected_model: "latest",
      selected_language: "ja",
      selected_microphone: "Desk Mic",
      selected_output_device: "Headphones",
    });
    expect(appSettingsCalls).toBe(2);
  });

  test("does not enumerate microphones until an explicit device refresh", async () => {
    const initialization = useSettingsStore.getState().initialize();
    resolvePendingListeners();
    await initialization;

    emit("settings-changed", { setting: "selected_microphone" });
    await waitFor(() => appSettingsCalls === 2);
    expect(microphoneCalls).toBe(0);

    await useSettingsStore.getState().refreshAudioDevices();
    expect(microphoneCalls).toBe(1);

    emit("settings-changed", { setting: "selected_microphone" });
    await waitFor(() => microphoneCalls === 2);
    expect(microphoneCalls).toBe(2);
  });

  test("releases shared listeners and ignores callbacks after disposal", async () => {
    const initialization = useSettingsStore.getState().initialize();
    resolvePendingListeners();
    await initialization;

    await useSettingsStore.getState().dispose();
    expect(unlistenCalls).toBe(2);

    const callsAfterDispose = appSettingsCalls;
    emit("model-state-changed", undefined);
    emit("settings-changed", { setting: "selected_model" });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(appSettingsCalls).toBe(callsAfterDispose);
  });

  test("cleans up registrations that finish after disposal", async () => {
    const initialization = useSettingsStore.getState().initialize();
    const disposing = useSettingsStore.getState().dispose();

    expect(unlistenCalls).toBe(0);
    resolvePendingListeners();
    await Promise.all([initialization, disposing]);

    expect(unlistenCalls).toBe(2);
    expect(appSettingsCalls).toBe(0);
  });

  test("ignores an in-flight read from a disposed lifecycle", async () => {
    useSettingsStore.setState({ settings: null });
    const staleRead = deferred<SettingsResult>();
    appSettingsResponder = () => staleRead.promise;

    const initialization = useSettingsStore.getState().initialize();
    resolvePendingListeners();
    await waitFor(() => appSettingsCalls === 1);

    const disposing = useSettingsStore.getState().dispose();
    staleRead.resolve(ok(settings("stale")));
    await Promise.all([initialization, disposing]);

    expect(useSettingsStore.getState().settings).toBeNull();
  });

  test("releases successful listeners when another listener fails", async () => {
    registrationFailures.add("settings-changed");

    const initialization = useSettingsStore.getState().initialize();
    resolvePendingListeners();
    await initialization;

    expect(appSettingsCalls).toBe(1);
    expect(unlistenCalls).toBe(1);
  });

  test("ends loading and keeps existing settings when a read fails", async () => {
    const existingSettings = settings("existing", {
      selected_language: "en",
    });
    useSettingsStore.setState({ settings: existingSettings, isLoading: true });
    appSettingsResponder = async () => {
      throw new Error("read failed");
    };

    const initialization = useSettingsStore.getState().initialize();
    resolvePendingListeners();
    await initialization;

    expect(useSettingsStore.getState().settings).toBe(existingSettings);
    expect(useSettingsStore.getState().isLoading).toBe(false);
  });
});
