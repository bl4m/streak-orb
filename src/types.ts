export interface FieldSpec {
  key: string;
  label: string;
  kind: "text" | "password" | string;
  required: boolean;
  placeholder: string;
  help: string;
}

export interface ConnectorMeta {
  id: string;
  name: string;
  blurb: string;
  defaultTimezone: string;
  fields: FieldSpec[];
}

export interface AccountView {
  id: string;
  connectorId: string;
  connectorName: string;
  label: string;
  username: string;
  leadHours: number;
  enabled: boolean;
  timezone: string;
  hasToken: boolean;
  streak: number;
  doneToday: boolean;
  status: string;
  detail: string;
  error: string | null;
  checkedAt: string | null;
  serviceUrl: string;
  timeLeft: string | null;
  freshBreak: boolean;
}

export interface Dashboard {
  accounts: AccountView[];
  nextCheckAt: string | null;
  checking: boolean;
}

export interface Reminder {
  accountId: string;
  connectorName: string;
  label: string;
  kind: "at_risk" | "ended" | string;
  message: string;
  serviceUrl: string;
  streak: number;
}

export interface AppSettings {
  quietStart: string | null;
  quietEnd: string | null;
  launchAtLogin: boolean;
}

export interface TestResult {
  ok: boolean;
  summary: string;
}

export interface SaveAccount {
  id?: string;
  connectorId: string;
  label: string;
  username: string;
  leadHours: number;
  timezone: string;
  enabled: boolean;
  token?: string;
  tokenChanged: boolean;
}

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
