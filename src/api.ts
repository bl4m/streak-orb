import type {
  AccountView,
  AppSettings,
  ConnectorMeta,
  Dashboard,
  Reminder,
  SaveAccount,
  TestResult,
} from "./types";
import { checkPreview } from "./previewCheck";
import { inTauri } from "./types";

const CATALOG: ConnectorMeta[] = [
  {
    id: "duolingo",
    name: "Duolingo",
    blurb: "Public profile streak. Duolingo has no official streak API, so this can break if their page changes.",
    defaultTimezone: "",
    fields: [],
  },
  {
    id: "github",
    name: "GitHub",
    blurb: "Consecutive days with at least one contribution. A token is only needed for private contributions.",
    defaultTimezone: "",
    fields: [
      {
        key: "token",
        label: "Personal access token",
        kind: "password",
        required: false,
        placeholder: "Leave blank for public contributions",
        help: "Optional. Stored in the macOS Keychain, never in the app database.",
      },
    ],
  },
  {
    id: "leetcode",
    name: "LeetCode",
    blurb: "Public solve calendar. LeetCode days roll over at midnight UTC, and the API is unofficial.",
    defaultTimezone: "UTC",
    fields: [],
  },
  {
    id: "codeforces",
    name: "Codeforces",
    blurb: "Consecutive days with an accepted submission. Codeforces has no built-in streak, so this one is counted here.",
    defaultTimezone: "",
    fields: [],
  },
];

const PREVIEW_KEY = "streak-orb-preview";

const sampleAccounts: AccountView[] = [
  {
    id: "duo",
    connectorId: "duolingo",
    connectorName: "Duolingo",
    label: "Spanish",
    username: "ada",
    leadHours: 3,
    enabled: true,
    timezone: "",
    hasToken: false,
    streak: 42,
    doneToday: false,
    status: "at_risk",
    detail: "Still open today",
    error: null,
    checkedAt: new Date().toISOString(),
    serviceUrl: "https://www.duolingo.com/profile/ada",
    timeLeft: "2 hours left",
    freshBreak: false,
  },
  {
    id: "gh",
    connectorId: "github",
    connectorName: "GitHub",
    label: "GitHub · ada",
    username: "ada",
    leadHours: 3,
    enabled: true,
    timezone: "",
    hasToken: false,
    streak: 12,
    doneToday: true,
    status: "ok",
    detail: "Done today",
    error: null,
    checkedAt: new Date().toISOString(),
    serviceUrl: "https://github.com/ada",
    timeLeft: null,
    freshBreak: false,
  },
];

const sampleSettings: AppSettings = {
  quietStart: "22:00",
  quietEnd: "08:00",
  launchAtLogin: true,
};

function readPreview(): { accounts: AccountView[]; settings: AppSettings; live: boolean } | null {
  try {
    const raw = localStorage.getItem(PREVIEW_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as { accounts?: AccountView[]; settings?: AppSettings; liveVersion?: number };
    if (!Array.isArray(parsed.accounts)) return null;
    return {
      accounts: parsed.accounts,
      settings: { ...sampleSettings, ...parsed.settings },
      live: parsed.liveVersion === 2,
    };
  } catch {
    return null;
  }
}

const storedPreview = readPreview();
let previewAccounts: AccountView[] = storedPreview?.accounts ?? sampleAccounts;
let previewSettings: AppSettings = storedPreview?.settings ?? { ...sampleSettings };
let previewLive = storedPreview?.live ?? false;

const TOKEN_KEY = "streak-orb-preview-tokens";

function readTokens(): Record<string, string> {
  try {
    return JSON.parse(sessionStorage.getItem(TOKEN_KEY) ?? "{}") as Record<string, string>;
  } catch {
    return {};
  }
}

function rememberToken(id: string, token: string) {
  const tokens = readTokens();
  if (token.trim()) tokens[id] = token.trim();
  else delete tokens[id];
  sessionStorage.setItem(TOKEN_KEY, JSON.stringify(tokens));
}

function tokenFor(id: string | undefined): string | undefined {
  if (!id) return undefined;
  return readTokens()[id];
}

let rechecking: Promise<void> | null = null;

async function recheckPreview(force: boolean) {
  if (rechecking) return rechecking;
  const due = previewAccounts.filter((account) => {
    if (!account.enabled) return false;
    if (force || !previewLive || account.serviceUrl === "https://example.com") return true;
    const then = account.checkedAt ? Date.parse(account.checkedAt) : 0;
    return !then || Date.now() - then > 45 * 60 * 1000;
  });
  if (due.length === 0) return;
  rechecking = (async () => {
    await Promise.all(
      due.map(async (account) => {
        const checked = await checkPreview({
          connectorId: account.connectorId,
          username: account.username,
          timezone: account.timezone,
          leadHours: account.leadHours,
          token: tokenFor(account.id),
        });
        const index = previewAccounts.findIndex((item) => item.id === account.id);
        if (index < 0) return;
        previewAccounts[index] = { ...previewAccounts[index], ...checked.view };
      }),
    );
    previewLive = true;
    rememberPreview();
  })().finally(() => {
    rechecking = null;
  });
  return rechecking;
}

function rememberPreview() {
  try {
    localStorage.setItem(
      PREVIEW_KEY,
      JSON.stringify({ accounts: previewAccounts, settings: previewSettings, liveVersion: previewLive ? 2 : 1 }),
    );
  } catch {
    // A private window can refuse storage. The list still updates for this tab.
  }
}

let previewReminder: Reminder | null = {
  accountId: "duo",
  connectorName: "Duolingo",
  label: "Spanish",
  kind: "at_risk",
  message: "42-day streak · 2 hours left",
  serviceUrl: "https://www.duolingo.com/profile/ada",
  streak: 42,
};

function previewDashboard(): Dashboard {
  return {
    accounts: previewAccounts.map((account) => ({ ...account })),
    nextCheckAt: new Date(Date.now() + 45 * 60 * 1000).toISOString(),
    checking: false,
  };
}

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

export async function getDashboard(): Promise<Dashboard> {
  if (!inTauri()) {
    await recheckPreview(false);
    return previewDashboard();
  }
  return invoke<Dashboard>("dashboard");
}

export async function getCatalog(): Promise<ConnectorMeta[]> {
  if (!inTauri()) return CATALOG;
  return invoke<ConnectorMeta[]>("catalog");
}

export async function getSettings(): Promise<AppSettings> {
  if (!inTauri()) return { ...previewSettings };
  return invoke<AppSettings>("get_settings");
}

export async function saveSettings(settings: AppSettings): Promise<AppSettings> {
  if (!inTauri()) {
    previewSettings = { ...settings };
    rememberPreview();
    return { ...previewSettings };
  }
  return invoke<AppSettings>("save_settings", { input: settings });
}

export async function refreshNow(): Promise<Dashboard> {
  if (!inTauri()) {
    await recheckPreview(true);
    return previewDashboard();
  }
  return invoke<Dashboard>("refresh_now");
}

export async function testConnector(input: {
  accountId?: string;
  connectorId: string;
  username: string;
  timezone: string;
  token?: string;
  tokenChanged: boolean;
}): Promise<TestResult> {
  if (!inTauri()) {
    const result = await checkPreview({
      connectorId: input.connectorId,
      username: input.username,
      timezone: input.timezone,
      leadHours: 3,
      token: input.token,
    });
    return { ok: result.ok, summary: result.summary };
  }
  return invoke<TestResult>("test_connector", { input });
}

export async function saveAccount(input: SaveAccount): Promise<AccountView> {
  if (!inTauri()) {
    const meta = CATALOG.find((item) => item.id === input.connectorId);
    const previous = previewAccounts.find((account) => account.id === input.id);
    const hasToken = input.tokenChanged ? Boolean(input.token) : previous?.hasToken ?? false;
    const id = input.id ?? crypto.randomUUID();
    if (input.tokenChanged) rememberToken(id, input.token ?? "");
    const checked = await checkPreview({
      connectorId: input.connectorId,
      username: input.username,
      timezone: input.timezone,
      leadHours: input.leadHours,
      token: tokenFor(id),
    });
    const view: AccountView = {
      id,
      connectorId: input.connectorId,
      connectorName: meta?.name ?? "Streak",
      label: input.label.trim() || `${meta?.name ?? "Streak"} · ${input.username}`,
      username: input.username,
      leadHours: input.leadHours,
      enabled: input.enabled,
      timezone: input.timezone,
      hasToken,
      streak: 0,
      doneToday: false,
      status: "idle",
      detail: "No streak yet",
      error: null,
      checkedAt: new Date().toISOString(),
      serviceUrl: "https://example.com",
      timeLeft: null,
      freshBreak: false,
      ...checked.view,
    };
    const index = previewAccounts.findIndex((account) => account.id === view.id);
    if (index >= 0) previewAccounts[index] = view;
    else previewAccounts = [...previewAccounts, view];
    rememberPreview();
    return view;
  }
  return invoke<AccountView>("save_account", { input });
}

export async function deleteAccount(id: string): Promise<void> {
  if (!inTauri()) {
    previewAccounts = previewAccounts.filter((account) => account.id !== id);
    rememberPreview();
    return;
  }
  return invoke("delete_account", { id });
}

export async function setEnabled(id: string, enabled: boolean): Promise<void> {
  if (!inTauri()) {
    previewAccounts = previewAccounts.map((account) =>
      account.id === id ? { ...account, enabled } : account,
    );
    rememberPreview();
    return;
  }
  return invoke("set_enabled", { id, enabled });
}

export async function getReminder(): Promise<Reminder | null> {
  if (!inTauri()) return previewReminder;
  return invoke<Reminder | null>("active_reminder");
}

export async function snooze(accountId: string, minutes: number): Promise<void> {
  if (!inTauri()) {
    previewReminder = null;
    return;
  }
  return invoke("snooze", { accountId, minutes });
}

export async function dismissReminder(accountId: string): Promise<void> {
  if (!inTauri()) {
    previewReminder = null;
    return;
  }
  return invoke("dismiss_reminder", { accountId });
}

export async function openService(url: string): Promise<void> {
  if (!inTauri()) {
    window.open(url, "_blank");
    return;
  }
  return invoke("open_service", { url });
}

export async function showSettings(): Promise<void> {
  if (!inTauri()) {
    location.hash = "settings";
    return;
  }
  return invoke("show_settings");
}

export async function quitApp(): Promise<void> {
  if (!inTauri()) return;
  return invoke("quit");
}

export async function listenUpdated(handler: () => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen("streaks-updated", () => handler());
  return unlisten;
}

export async function listenReminder(
  onShow: (reminder: Reminder) => void,
  onClear: () => void,
): Promise<() => void> {
  if (!inTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const shown = await listen<Reminder>("reminder", (event) => onShow(event.payload));
  const cleared = await listen("reminder-clear", () => onClear());
  return () => {
    shown();
    cleared();
  };
}
