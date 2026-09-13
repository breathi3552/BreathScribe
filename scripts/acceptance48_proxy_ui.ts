import { chromium, type Browser, type Page } from "@playwright/test";
import {
  createConnection,
  createServer,
  type AddressInfo,
  type Server,
} from "node:net";
import { createServer as createHttpServer, request } from "node:http";
import type { Duplex } from "node:stream";
import { createHash, randomUUID } from "node:crypto";
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import {
  existsSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { mkdir, open, readFile, rm, writeFile } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { dirname, join, resolve, win32 } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const feature = "acceptance48_test";
const appIdentifier = `io.github.breathi3552.breathscribe.acceptance48.${randomUUID()}`;
const probeMessage = "acceptance48-ws";
let activeInterrupt: Promise<never> | undefined;

type ProxySettings = {
  mode: "manual";
  protocol: "http";
  host: string;
  port: number;
  auth_enabled: false;
  username: null;
  password: null;
};

type TauriResult =
  | { status: "ok"; data: unknown }
  | { status: "error"; error: string };

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function resultError(result: TauriResult): string {
  return result.status === "error" ? result.error : "no error";
}

async function waitFor(
  condition: () => boolean | Promise<boolean>,
  description: string,
  timeoutMs = 10_000,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!(await condition())) {
    if (Date.now() >= deadline)
      throw new Error(`Timed out waiting for ${description}`);
    const delay = new Promise<void>((resolve) => setTimeout(resolve, 50));
    if (activeInterrupt) await Promise.race([delay, activeInterrupt]);
    else await delay;
  }
}

async function listen(server: Server): Promise<number> {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolve());
  });
  return (server.address() as AddressInfo).port;
}

async function closeServer(server: Server | undefined): Promise<void> {
  if (!server?.listening) return;
  await new Promise<void>((resolve) => server.close(() => resolve()));
}

function wsTextFrame(text: string): Buffer {
  const payload = Buffer.from(text);
  assert(payload.length < 126, "acceptance response is unexpectedly large");
  return Buffer.concat([Buffer.from([0x81, payload.length]), payload]);
}

function consumeWebSocketFrames(socket: Duplex, input: Buffer): Buffer {
  let buffer = input;
  while (buffer.length >= 2) {
    const first = buffer[0];
    const second = buffer[1];
    const opcode = first & 0x0f;
    const masked = (second & 0x80) !== 0;
    let length = second & 0x7f;
    let offset = 2;

    if (length === 126) {
      if (buffer.length < 4) return buffer;
      length = buffer.readUInt16BE(2);
      offset = 4;
    } else if (length === 127) {
      if (buffer.length < 10) return buffer;
      const length64 = buffer.readBigUInt64BE(2);
      if (length64 > BigInt(Number.MAX_SAFE_INTEGER)) {
        socket.destroy(new Error("acceptance WebSocket frame is too large"));
        return Buffer.alloc(0);
      }
      length = Number(length64);
      offset = 10;
    }

    const maskLength = masked ? 4 : 0;
    if (buffer.length < offset + maskLength + length) return buffer;

    const mask = masked ? buffer.subarray(offset, offset + 4) : undefined;
    offset += maskLength;
    const payload = Buffer.from(buffer.subarray(offset, offset + length));
    if (mask) {
      for (let index = 0; index < payload.length; index += 1) {
        payload[index] ^= mask[index % 4];
      }
    }
    buffer = buffer.subarray(offset + length);

    if (opcode === 0x8) {
      socket.end();
      return buffer;
    }
    if (opcode === 0x1) socket.write(wsTextFrame(probeMessage));
  }
  return buffer;
}

class ProbeTarget {
  readonly server = createHttpServer((request, response) => {
    this.requests.push(request.url ?? "");
    response.writeHead(204, { Connection: "close" });
    response.end();
  }).on("upgrade", (request, socket, head) => {
    this.requests.push(request.url ?? "");
    socket.on("error", () => undefined);
    // HTTP upgrade sockets are half-open; preserve the old net.Server cleanup.
    socket.on("end", () => socket.end());
    if (request.headers.upgrade?.toLowerCase() !== "websocket") {
      socket.end(
        "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
      );
      return;
    }
    const key = request.headers["sec-websocket-key"];
    if (!key) {
      socket.destroy(new Error("WebSocket handshake has no key"));
      return;
    }
    const accept = createHash("sha1")
      .update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
      .digest("base64");
    socket.write(
      `HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`,
    );
    let buffer = consumeWebSocketFrames(socket, head);
    socket.on("data", (chunk) => {
      buffer = consumeWebSocketFrames(socket, Buffer.concat([buffer, chunk]));
    });
  });
  readonly requests: string[] = [];
  port = 0;

  async start(): Promise<void> {
    this.port = await listen(this.server);
  }

  async close(): Promise<void> {
    await closeServer(this.server);
  }
}

class HttpProxy {
  readonly server = createHttpServer((incoming, response) => {
    this.httpHits += 1;
    let url: URL;
    try {
      url = new URL(incoming.url ?? "");
    } catch {
      incoming.socket.destroy();
      return;
    }
    const target = request(
      url,
      { method: incoming.method, headers: incoming.headers, agent: false },
      (upstream) => {
        response.writeHead(upstream.statusCode ?? 502, upstream.headers);
        upstream.pipe(response);
      },
    );
    target.once("error", () => response.destroy());
    incoming.once("error", () => target.destroy());
    response.once("close", () => target.destroy());
    incoming.pipe(target);
  }).on("connect", (request, socket, head) => {
    this.connectHits += 1;
    const [host, rawPort] = (request.url ?? "").split(":");
    this.tunnel(socket, host, Number(rawPort), head);
  });
  httpHits = 0;
  connectHits = 0;
  port = 0;

  async start(): Promise<void> {
    this.port = await listen(this.server);
  }

  async close(): Promise<void> {
    await closeServer(this.server);
  }

  private tunnel(
    socket: Duplex,
    host: string,
    port: number,
    remainder: Buffer,
  ): void {
    const target = createConnection(port, host);
    target.once("connect", () => {
      socket.write("HTTP/1.1 200 Connection Established\r\n\r\n");
      if (remainder.length) target.write(remainder);
      socket.pipe(target).pipe(socket);
    });
    target.once("error", () => socket.destroy());
    socket.once("error", () => target.destroy());
  }
}

async function reservePort(): Promise<number> {
  const server = createServer();
  const port = await listen(server);
  await closeServer(server);
  return port;
}

async function startRejectingProxy(): Promise<{
  server: Server;
  port: number;
  connections: number[];
}> {
  const connections: number[] = [];
  const server = createServer((socket) => {
    connections.push(Date.now());
    socket.destroy();
  });
  const port = await listen(server);
  return { server, port, connections };
}

function windowsPath(path: string): string {
  return win32.normalize(path).replace(/\\/g, "\\");
}

function findCmakeDir(): string {
  try {
    const cmake = execFileSync("where.exe", ["cmake"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    })
      .split(/\r?\n/)
      .find(Boolean);
    if (cmake) return dirname(cmake.trim());
  } catch {
    // The normal Windows CMake installation is checked below.
  }
  for (const candidate of [
    "C:\\Program Files\\CMake\\bin",
    "C:\\Program Files (x86)\\CMake\\bin",
  ]) {
    if (existsSync(join(candidate, "cmake.exe"))) return candidate;
  }

  const walk = (directory: string): string | undefined => {
    let entries;
    try {
      entries = readdirSync(directory, { withFileTypes: true });
    } catch {
      return undefined;
    }
    for (const entry of entries) {
      const entryPath = join(directory, entry.name);
      if (entry.isFile() && entry.name.toLowerCase() === "cmake.exe") {
        return dirname(entryPath);
      }
      if (entry.isDirectory()) {
        const found = walk(entryPath);
        if (found) return found;
      }
    }
    return undefined;
  };
  const found = walk(
    join(
      process.env.USERPROFILE ?? homedir(),
      "AppData",
      "Local",
      "Microsoft",
      "WinGet",
      "Packages",
    ),
  );
  if (found) return found;
  throw new Error("cmake.exe was not found; no Tauri build can be started");
}

function findBunPath(): string {
  const userProfile = process.env.USERPROFILE ?? homedir();
  const candidates = [
    join(
      userProfile,
      "AppData",
      "Roaming",
      "npm",
      "node_modules",
      "bun",
      "bin",
      "bun.exe",
    ),
    join(userProfile, ".bun", "bin", "bun.exe"),
    process.execPath,
  ];
  const bunPath = candidates.find((candidate) => existsSync(candidate));
  if (!bunPath)
    throw new Error("bun.exe was not found; no Tauri dev app can be started");
  return bunPath;
}

function writeTauriLauncher(
  path: string,
  configPath: string,
  targetDir: string,
  remotePort: number,
  connectivityUrl: string,
): void {
  const userProfile = process.env.USERPROFILE ?? homedir();
  const bunPath = findBunPath();
  const cmakeDir = findCmakeDir();
  const escaped = (value: string) => windowsPath(value).replace(/%/g, "%%");
  const lines = [
    "@echo off",
    `set "PATH=${escaped(join(userProfile, ".cargo", "bin"))};${escaped(dirname(bunPath))};${escaped(cmakeDir)};C:\\Windows\\System32;C:\\Windows"`,
    `set "CARGO_TARGET_DIR=${escaped(targetDir)}"`,
    `set "WEBVIEW2_USER_DATA_FOLDER=${escaped(join(targetDir, "debug", "Data", "webview"))}"`,
    `set "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=${remotePort}"`,
    `set "BREATHSCRIBE_ACCEPTANCE48_CONNECTIVITY_URL=${connectivityUrl}"`,
    'set "HANDY_DISABLE_UPDATER=1"',
    `cd /d "${escaped(repoRoot)}"`,
    `"${escaped(bunPath)}" node_modules\\@tauri-apps\\cli\\tauri.js dev --no-watch --features ${feature} --config "${escaped(configPath)}"`,
  ];
  writeFileSync(path, `${lines.join("\r\n")}\r\n`);
}

async function fetchJson(url: string): Promise<unknown> {
  const response = await fetch(url, { signal: AbortSignal.timeout(2_000) });
  if (!response.ok)
    throw new Error(`HTTP ${response.status} from local CDP endpoint`);
  return response.json();
}

async function connectMainPage(
  remotePort: number,
): Promise<{ browser: Browser; page: Page }> {
  const browser = await chromium.connectOverCDP(
    `http://127.0.0.1:${remotePort}`,
  );
  await waitFor(
    () =>
      browser
        .contexts()
        .flatMap((context) => context.pages())
        .some((page) => page.url() === "http://tauri.localhost/"),
    "the Tauri main WebView",
    30_000,
  );
  const page = browser
    .contexts()
    .flatMap((context) => context.pages())
    .find((candidate) => candidate.url() === "http://tauri.localhost/");
  assert(page, "Tauri main page was not found");
  await page.waitForLoadState("domcontentloaded");
  return { browser, page };
}

async function tauriInvoke(
  page: Page,
  command: string,
  args: Record<string, unknown> = {},
): Promise<TauriResult> {
  return page.evaluate(
    async ({ command: invokeCommand, args: invokeArgs }) => {
      const internals = (
        window as Window & {
          __TAURI_INTERNALS__?: {
            invoke: (
              name: string,
              payload: Record<string, unknown>,
            ) => Promise<unknown>;
          };
        }
      ).__TAURI_INTERNALS__;
      if (!internals)
        throw new Error("Tauri internals are unavailable in the WebView");
      try {
        return {
          status: "ok",
          data: await internals.invoke(invokeCommand, invokeArgs),
        };
      } catch (error) {
        return { status: "error", error: String(error) };
      }
    },
    { command, args },
  );
}

async function toastCount(
  page: Page,
  type: "success" | "error",
): Promise<number> {
  return page.locator(`[data-sonner-toast][data-type="${type}"]`).count();
}

function proxyFromValue(value: unknown): ProxySettings {
  assert(
    value && typeof value === "object",
    "stored settings are not an object",
  );
  const proxy = (value as { proxy?: unknown }).proxy;
  assert(proxy && typeof proxy === "object", "stored proxy is missing");
  const candidate = proxy as Partial<ProxySettings>;
  return {
    mode: "manual",
    protocol: "http",
    host: String(candidate.host),
    port: Number(candidate.port),
    auth_enabled: false,
    username: null,
    password: null,
  };
}

async function readStoredProxy(storePath: string): Promise<ProxySettings> {
  const stored = JSON.parse(await readFile(storePath, "utf8")) as {
    settings?: unknown;
  };
  return proxyFromValue(stored.settings ?? stored);
}

async function terminateProcessTree(
  child: ChildProcess | undefined,
): Promise<void> {
  if (!child?.pid) return;
  if (process.platform === "win32") {
    try {
      execFileSync(
        "C:\\Windows\\System32\\taskkill.exe",
        ["/PID", String(child.pid), "/T", "/F"],
        {
          stdio: "ignore",
        },
      );
    } catch {
      // The launcher may already have exited after a compile failure.
    }
  } else {
    child.kill("SIGTERM");
  }
  await new Promise((resolve) => setTimeout(resolve, 500));
}

async function main(): Promise<void> {
  if (process.platform !== "win32") {
    throw new Error(
      "This acceptance runner currently requires Windows WebView2",
    );
  }
  process.env.NO_PROXY = "127.0.0.1,localhost";
  process.env.no_proxy = process.env.NO_PROXY;

  const tempDir = mkdtempSync(join(tmpdir(), "breathscribe-acceptance48-"));
  const targetDir = join(tempDir, "cargo-target");
  const dataDir = join(targetDir, "debug", "Data");
  const portableMarkerPath = join(targetDir, "debug", "portable");
  const storePath = join(dataDir, "settings_store.json");
  const configPath = join(tempDir, "tauri.conf.json");
  const launcherPath = join(tempDir, "run-tauri.cmd");
  const logPath = join(tempDir, "tauri.log");
  const bindingsPath = join(repoRoot, "src", "bindings.ts");
  const target = new ProbeTarget();
  const proxyA = new HttpProxy();
  const proxyB = new HttpProxy();
  let bindingsBefore: Buffer | undefined;
  let rejecting: Awaited<ReturnType<typeof startRejectingProxy>> | undefined;
  let launcher: ChildProcess | undefined;
  let browser: Browser | undefined;
  let logHandle: Awaited<ReturnType<typeof open>> | undefined;
  let runFailed = false;
  const cleanupErrors: unknown[] = [];
  const cleanupStep = async (
    operation: () => Promise<unknown>,
  ): Promise<void> => {
    try {
      await operation();
    } catch (error) {
      cleanupErrors.push(error);
    }
  };
  let rejectInterrupt: (reason: Error) => void = () => undefined;
  const interruptPromise = new Promise<never>((_, reject) => {
    rejectInterrupt = reject;
  });
  void interruptPromise.catch(() => undefined);
  let interrupted = false;
  const onInterrupt = (signal?: NodeJS.Signals): void => {
    if (interrupted) return;
    interrupted = true;
    rejectInterrupt(
      new Error(`acceptance48 runner interrupted by ${signal ?? "SIGINT"}`),
    );
    if (browser) void browser.close().catch(() => undefined);
  };
  activeInterrupt = interruptPromise;
  process.once("SIGINT", onInterrupt);
  process.once("SIGTERM", onInterrupt);

  try {
    bindingsBefore = readFileSync(bindingsPath);
    rejecting = await startRejectingProxy();
    assert(rejecting, "rejecting proxy failed to start");
    await mkdir(dataDir, { recursive: true });
    await target.start();
    await proxyA.start();
    await proxyB.start();

    const proxyASettings: ProxySettings = {
      mode: "manual",
      protocol: "http",
      host: "127.0.0.1",
      port: proxyA.port,
      auth_enabled: false,
      username: null,
      password: null,
    };
    const proxyBSettings: ProxySettings = {
      ...proxyASettings,
      port: proxyB.port,
    };
    const connectivityUrl = `http://127.0.0.1:${target.port}/probe`;
    const websocketUrl = `ws://127.0.0.1:${target.port}/ws`;
    const frontendPort = await reservePort();
    const remotePort = await reservePort();
    // These reservations are intentionally short-lived; Vite and the Tauri
    // WebView2 process own the ports once they start.

    await writeFile(
      configPath,
      JSON.stringify({
        identifier: appIdentifier,
        productName: "BreathScribe Acceptance 48",
        build: {
          beforeDevCommand: `bun run dev -- --host 127.0.0.1 --port ${frontendPort}`,
          devUrl: `http://127.0.0.1:${frontendPort}`,
        },
      }),
    );
    await writeFile(
      storePath,
      JSON.stringify(
        {
          settings: {
            settings_schema_version: 2,
            onboarding_completed: true,
            selected_model: "small",
            show_tray_icon: false,
            start_hidden: false,
            debug_mode: false,
            whats_new_last_seen_version: "0.1.1",
            proxy: proxyASettings,
          },
        },
        null,
        2,
      ),
    );
    await writeFile(portableMarkerPath, "BreathScribe Portable Mode\n");
    writeTauriLauncher(
      launcherPath,
      configPath,
      targetDir,
      remotePort,
      connectivityUrl,
    );

    logHandle = await open(logPath, "w");
    launcher = spawn(
      "C:\\Windows\\System32\\cmd.exe",
      ["/d", "/c", launcherPath],
      {
        cwd: repoRoot,
        stdio: ["ignore", logHandle.fd, logHandle.fd],
        windowsHide: false,
      },
    );
    await waitFor(
      () => existsSync(join(targetDir, "debug", "breath-scribe.exe")),
      "the isolated Tauri binary",
      900_000,
    );
    await waitFor(
      async () => {
        try {
          await fetchJson(`http://127.0.0.1:${remotePort}/json/version`);
          return true;
        } catch {
          return false;
        }
      },
      "the isolated WebView2 CDP endpoint",
      60_000,
    );

    const connected = await connectMainPage(remotePort);
    browser = connected.browser;
    const page = connected.page;

    await page.waitForTimeout(1_000);
    const initialBody = await page.locator("body").innerText();
    if (!/通用|General|高级|Advanced/.test(initialBody)) {
      throw new Error(
        `isolated main UI did not reach settings: ${initialBody.slice(0, 600)}`,
      );
    }

    const dialog = page.locator('[role="dialog"]');
    if (await dialog.count()) {
      const close = dialog.getByRole("button", { name: /关闭|Close/i });
      if (await close.count()) await close.first().click();
    }
    await page
      .getByText(/^(高级|Advanced)$/)
      .first()
      .click();
    const hostInput = page.locator('input[placeholder="127.0.0.1"]');
    const portInput = page.locator('input[placeholder="7890"]');
    const testButton = page.getByRole("button", {
      name: /测试连通性|Test Connectivity/i,
    });
    const saveButton = page.getByRole("button", {
      name: /保存代理设置|Save Proxy Settings/i,
    });
    await hostInput.waitFor();
    assert((await testButton.count()) === 1, "proxy test button is missing");
    assert((await saveButton.count()) === 1, "proxy save button is missing");

    // UI Test with candidate B: it must use B without changing saved A.
    await hostInput.fill(proxyBSettings.host);
    await portInput.fill(String(proxyBSettings.port));
    const bBeforeCandidateTest = proxyB.httpHits;
    const successToastsBeforeCandidateTest = await toastCount(page, "success");
    await testButton.click();
    await waitFor(
      () => proxyB.httpHits > bBeforeCandidateTest,
      "candidate B connectivity request through proxy B",
    );
    await waitFor(
      async () => !(await testButton.isDisabled()),
      "candidate B test button to recover",
    );
    await waitFor(
      () =>
        toastCount(page, "success").then(
          (count) => count > successToastsBeforeCandidateTest,
        ),
      "candidate B success toast",
    );
    assert(
      (await page.locator(".text-emerald-500").count()) > 0,
      "successful Test feedback is missing",
    );
    assert(
      (await readStoredProxy(storePath)).port === proxyASettings.port,
      "candidate Test changed the persisted A proxy",
    );

    // The same production command, through real Tauri IPC, must still use A.
    const beforeSavedProbeA = proxyA.httpHits;
    const beforeSavedProbeB = proxyB.httpHits;
    const currentProbe = await tauriInvoke(page, "test_proxy_connectivity", {
      settings: null,
    });
    assert(
      currentProbe.status === "ok",
      `current A probe failed: ${resultError(currentProbe)}`,
    );
    await waitFor(
      () => proxyA.httpHits > beforeSavedProbeA,
      "current shared HTTP client to remain on A before Save",
    );
    assert(
      proxyB.httpHits >= beforeSavedProbeB,
      "proxy counters moved backwards",
    );

    // UI Test with a reachable-but-rejecting candidate, then UI validation of
    // illegal values. Neither path may alter A.
    await hostInput.fill("127.0.0.1");
    await portInput.fill(String(rejecting.port));
    const rejectingConnections = rejecting.connections;
    const rejectingBefore = rejectingConnections.length;
    const errorToastsBeforeCandidateFailure = await toastCount(page, "error");
    await testButton.click();
    await waitFor(
      () => rejectingConnections.length > rejectingBefore,
      "failed candidate connection attempt",
    );
    await waitFor(
      async () => !(await testButton.isDisabled()),
      "failed Test button to recover",
    );
    await waitFor(
      () =>
        toastCount(page, "error").then(
          (count) => count > errorToastsBeforeCandidateFailure,
        ),
      "failed candidate error toast",
    );
    assert(
      (await page.locator(".text-rose-500").count()) > 0,
      "failed Test feedback is missing",
    );
    assert(
      (await readStoredProxy(storePath)).port === proxyASettings.port,
      "failed candidate Test changed the persisted A proxy",
    );

    await hostInput.fill("");
    await portInput.fill(String(proxyBSettings.port));
    const errorToastsBeforeEmptyHost = await toastCount(page, "error");
    await saveButton.click();
    await waitFor(
      () =>
        toastCount(page, "error").then(
          (count) => count > errorToastsBeforeEmptyHost,
        ),
      "empty-host validation feedback",
    );
    assert(
      (await readStoredProxy(storePath)).port === proxyASettings.port,
      "empty-host Save changed the persisted A proxy",
    );

    await hostInput.fill("127.0.0.1");
    await portInput.fill("0");
    const errorToastsBeforeInvalidPort = await toastCount(page, "error");
    await saveButton.click();
    await waitFor(
      () =>
        toastCount(page, "error").then(
          (count) => count > errorToastsBeforeInvalidPort,
        ),
      "invalid-port validation feedback",
    );
    assert(
      (await readStoredProxy(storePath)).port === proxyASettings.port,
      "invalid-port Save changed the persisted A proxy",
    );

    const invalidIpc = await tauriInvoke(page, "update_proxy_settings", {
      settings: { ...proxyBSettings, port: 0 },
    });
    assert(
      invalidIpc.status === "error",
      "invalid update_proxy_settings IPC unexpectedly succeeded",
    );
    assert(
      /port|65535|between/i.test(resultError(invalidIpc)),
      "invalid IPC error did not identify the port",
    );
    assert(
      (await readStoredProxy(storePath)).port === proxyASettings.port,
      "invalid update_proxy_settings IPC changed the persisted A proxy",
    );

    // Recover the visible error state with a successful candidate test.
    await hostInput.fill(proxyBSettings.host);
    await portInput.fill(String(proxyBSettings.port));
    const bAfterFailure = proxyB.httpHits;
    const successToastsBeforeRecovery = await toastCount(page, "success");
    await testButton.click();
    await waitFor(
      () => proxyB.httpHits > bAfterFailure,
      "candidate Test feedback recovery through B",
    );
    await waitFor(
      async () => !(await testButton.isDisabled()),
      "recovered Test button",
    );
    await waitFor(
      () =>
        toastCount(page, "success").then(
          (count) => count > successToastsBeforeRecovery,
        ),
      "recovered candidate success toast",
    );
    assert(
      (await page.locator(".text-emerald-500").count()) > 0,
      "recovered success feedback is missing",
    );

    // UI Save: production IPC must persist B before the manager is published.
    const successToastsBeforeSave = await toastCount(page, "success");
    await saveButton.click();
    await waitFor(
      async () =>
        (await readStoredProxy(storePath)).port === proxyBSettings.port,
      "proxy B to be persisted by the UI Save action",
    );
    const savedSettings = await tauriInvoke(page, "get_app_settings");
    assert(
      savedSettings.status === "ok",
      `get_app_settings failed: ${resultError(savedSettings)}`,
    );
    assert(
      proxyFromValue(savedSettings.data).port === proxyBSettings.port,
      "AppHandle memory did not contain B",
    );
    await waitFor(
      () =>
        toastCount(page, "success").then(
          (count) => count > successToastsBeforeSave,
        ),
      "successful Save toast",
    );

    // Real production IPC observes the newly published shared HTTP client on B.
    const savedHttpBefore = proxyB.httpHits;
    const savedCurrentProbe = await tauriInvoke(
      page,
      "test_proxy_connectivity",
      { settings: null },
    );
    assert(
      savedCurrentProbe.status === "ok",
      `saved B probe failed: ${resultError(savedCurrentProbe)}`,
    );
    await waitFor(
      () => proxyB.httpHits > savedHttpBefore,
      "new shared HTTP client to use B",
    );

    // This is the smallest private acceptance-only observer for the same
    // NetworkManager. The UI Save above and production commands remain real.
    const wsBefore = proxyB.connectHits;
    const networkProbe = await tauriInvoke(page, "acceptance48_probe_network", {
      websocketUrl,
    });
    assert(
      networkProbe.status === "ok",
      `new transport observer failed: ${resultError(networkProbe)}`,
    );
    const probeData = networkProbe.data as {
      http_rtt_ms: number;
      websocket_response: string;
    };
    assert(
      probeData.websocket_response === probeMessage,
      "new WebSocket did not reach the local target through B",
    );
    await waitFor(
      () => proxyB.connectHits > wsBefore,
      "new WebSocket to use B",
    );

    console.log(
      JSON.stringify({
        status: "passed",
        isolation: {
          appIdentifier,
          configPath,
          targetDir,
          frontendPort,
          remotePort,
          launcherPid: launcher?.pid ?? null,
        },
        ui: {
          candidateTestSuccess: true,
          candidateFailureFeedback: true,
          illegalValueRejected: true,
          errorStateRecovery: true,
          saveSuccessFeedback: true,
        },
        ipc: {
          productionCommands: [
            "test_proxy_connectivity(candidate)",
            "test_proxy_connectivity(null)",
            "update_proxy_settings",
            "get_app_settings",
          ],
          privateObserver: "acceptance48_probe_network",
          mocks: false,
        },
        store: {
          file: storePath,
          beforeSavePort: proxyASettings.port,
          afterSavePort: (await readStoredProxy(storePath)).port,
          memoryPort: proxyFromValue(savedSettings.data).port,
        },
        localEgress: {
          candidateBHttpHits: proxyB.httpHits,
          preSaveAHttpHits: proxyA.httpHits,
          savedBHttpHits: proxyB.httpHits,
          savedBWebSocketConnects: proxyB.connectHits,
          targetRequests: target.requests.length,
        },
      }),
    );
  } catch (error) {
    runFailed = true;
    throw error;
  } finally {
    await cleanupStep(async () => {
      if (browser) await browser.close();
    });
    await cleanupStep(() => terminateProcessTree(launcher));
    await cleanupStep(async () => {
      await logHandle?.close();
    });
    await cleanupStep(() => target.close());
    await cleanupStep(() => proxyA.close());
    await cleanupStep(() => proxyB.close());
    await cleanupStep(() => closeServer(rejecting?.server));
    await cleanupStep(() =>
      rm(tempDir, {
        recursive: true,
        force: true,
        maxRetries: 20,
        retryDelay: 250,
      }),
    );
    await cleanupStep(async () => {
      if (!bindingsBefore) return;
      const bindingsAfter = readFileSync(bindingsPath);
      if (!bindingsAfter.equals(bindingsBefore))
        writeFileSync(bindingsPath, bindingsBefore);
    });
    process.removeListener("SIGINT", onInterrupt);
    process.removeListener("SIGTERM", onInterrupt);
    activeInterrupt = undefined;

    if (cleanupErrors.length > 0) {
      const details = cleanupErrors
        .map((error) =>
          error instanceof Error ? error.message : String(error),
        )
        .join("; ");
      if (runFailed) console.error(`acceptance48 cleanup failed: ${details}`);
      else throw new Error(`acceptance48 cleanup failed: ${details}`);
    }
  }
}

try {
  await main();
} catch (error) {
  console.error(
    `acceptance48 proxy UI runner failed: ${error instanceof Error ? error.message : String(error)}`,
  );
  process.exitCode = 1;
}
