import { useEffect, useMemo, useState } from "react";
import {
  deleteAccount,
  getCatalog,
  getDashboard,
  getSettings,
  listenUpdated,
  saveAccount,
  saveSettings,
  setEnabled,
  testConnector,
} from "../api";
import type { AccountView, AppSettings, ConnectorMeta } from "../types";

interface Draft {
  id?: string;
  connectorId: string;
  label: string;
  username: string;
  leadHours: number;
  timezone: string;
  enabled: boolean;
  token: string;
  hasToken: boolean;
}

const EMPTY_SETTINGS: AppSettings = {
  quietStart: "",
  quietEnd: "",
  launchAtLogin: true,
};

function draftFrom(meta: ConnectorMeta, account?: AccountView): Draft {
  return {
    id: account?.id,
    connectorId: meta.id,
    label: account?.label ?? "",
    username: account?.username ?? "",
    leadHours: account?.leadHours ?? 3,
    timezone: account?.timezone || meta.defaultTimezone,
    enabled: account?.enabled ?? true,
    token: "",
    hasToken: account?.hasToken ?? false,
  };
}

export function Settings() {
  const [catalog, setCatalog] = useState<ConnectorMeta[]>([]);
  const [accounts, setAccounts] = useState<AccountView[]>([]);
  const [settings, setSettings] = useState<AppSettings>(EMPTY_SETTINGS);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [testNote, setTestNote] = useState("");
  const [testOk, setTestOk] = useState<boolean | null>(null);
  const [busy, setBusy] = useState<"" | "test" | "save">("");
  const [error, setError] = useState("");
  const [confirmRemove, setConfirmRemove] = useState(false);

  const meta = useMemo(
    () => catalog.find((item) => item.id === draft?.connectorId) ?? null,
    [catalog, draft],
  );

  async function load() {
    try {
      const [nextCatalog, dash, nextSettings] = await Promise.all([
        getCatalog(),
        getDashboard(),
        getSettings(),
      ]);
      setCatalog(nextCatalog);
      setAccounts(dash.accounts);
      setSettings({
        quietStart: nextSettings.quietStart ?? "",
        quietEnd: nextSettings.quietEnd ?? "",
        launchAtLogin: nextSettings.launchAtLogin,
      });
      setError("");
    } catch (err) {
      setError(typeof err === "string" ? err : "Couldn't load streaks.");
    }
  }

  useEffect(() => {
    void load();
    let unlisten = () => {};
    void listenUpdated(() => {
      void load();
    }).then((stop) => {
      unlisten = stop;
    });
    return () => unlisten();
  }, []);

  function startCreate(connector: ConnectorMeta) {
    setDraft(draftFrom(connector));
    setTestNote("");
    setTestOk(null);
    setConfirmRemove(false);
    setError("");
  }

  function startEdit(account: AccountView) {
    const connector = catalog.find((item) => item.id === account.connectorId);
    if (!connector) return;
    setDraft(draftFrom(connector, account));
    setTestNote("");
    setTestOk(null);
    setConfirmRemove(false);
    setError("");
  }

  async function runTest() {
    if (!draft) return;
    setBusy("test");
    setError("");
    try {
      const result = await testConnector({
        accountId: draft.id,
        connectorId: draft.connectorId,
        username: draft.username,
        timezone: draft.timezone,
        token: draft.token,
        tokenChanged: draft.token.length > 0,
      });
      setTestOk(result.ok);
      setTestNote(result.summary);
    } catch (err) {
      setTestOk(false);
      setTestNote(typeof err === "string" ? err : "Couldn't reach that site.");
    } finally {
      setBusy("");
    }
  }

  async function save() {
    if (!draft) return;
    setBusy("save");
    setError("");
    try {
      await saveAccount({
        id: draft.id,
        connectorId: draft.connectorId,
        label: draft.label,
        username: draft.username,
        leadHours: Number(draft.leadHours),
        timezone: draft.timezone,
        enabled: draft.enabled,
        token: draft.token,
        tokenChanged: draft.token.length > 0,
      });
      setDraft(null);
      await load();
    } catch (err) {
      setError(typeof err === "string" ? err : "Couldn't save that streak.");
    } finally {
      setBusy("");
    }
  }

  async function remove() {
    if (!draft?.id) return;
    if (!confirmRemove) {
      setConfirmRemove(true);
      return;
    }
    setBusy("save");
    try {
      await deleteAccount(draft.id);
      setDraft(null);
      await load();
    } catch (err) {
      setError(typeof err === "string" ? err : "Couldn't remove that streak.");
    } finally {
      setBusy("");
      setConfirmRemove(false);
    }
  }

  async function toggle(account: AccountView) {
    try {
      await setEnabled(account.id, !account.enabled);
      await load();
    } catch (err) {
      setError(typeof err === "string" ? err : "Couldn't update that streak.");
    }
  }

  function updateQuiet(partial: Partial<AppSettings>) {
    const next = { ...settings, ...partial };
    setSettings(next);
    const start = next.quietStart ?? "";
    const end = next.quietEnd ?? "";
    if ((start === "") !== (end === "")) return;
    void saveQuiet(next);
  }

  async function saveQuiet(next: AppSettings) {
    try {
      const saved = await saveSettings(next);
      setSettings({
        quietStart: saved.quietStart ?? "",
        quietEnd: saved.quietEnd ?? "",
        launchAtLogin: saved.launchAtLogin,
      });
      setError("");
    } catch (err) {
      setError(typeof err === "string" ? err : "Couldn't save those settings.");
    }
  }

  return (
    <div className="settings">
      <aside>
        <p className="kicker">Connectors</p>
        <h1>Streaks</h1>
        <div className="catalog">
          {catalog.map((connector) => (
            <button key={connector.id} type="button" className="catalog-card" onClick={() => startCreate(connector)}>
              <span className={`mark mark-${connector.id}`}>{connector.name.slice(0, 1)}</span>
              <span>
                <strong>{connector.name}</strong>
                <em>Add</em>
              </span>
            </button>
          ))}
        </div>
        <ul className="saved-list">
          {accounts.map((account) => (
            <li key={account.id}>
              <button type="button" className="saved" onClick={() => startEdit(account)}>
                <strong>{account.label}</strong>
                <em>{account.enabled ? account.connectorName : "Paused"}</em>
              </button>
              <button
                type="button"
                className="text-btn"
                onClick={() => void toggle(account)}
                aria-pressed={account.enabled}
              >
                {account.enabled ? "On" : "Off"}
              </button>
            </li>
          ))}
        </ul>
      </aside>
      <main>
        {!draft && (
          <div className="form-empty">
            <h2>Add a streak</h2>
            <p>Pick a connector. You'll only be asked for a username, plus a token when the site needs one.</p>
          </div>
        )}
        {draft && meta && (
          <form
            className="form"
            onSubmit={(event) => {
              event.preventDefault();
              void save();
            }}
          >
            <p className="kicker">{draft.id ? "Edit" : "New"}</p>
            <h2>{meta.name}</h2>
            <p className="blurb">{meta.blurb}</p>
            <label>
              Name
              <input
                value={draft.label}
                placeholder={`${meta.name} · ${draft.username || "username"}`}
                onChange={(event) => setDraft({ ...draft, label: event.target.value })}
              />
            </label>
            <label>
              Username
              <input
                required
                autoCapitalize="off"
                autoCorrect="off"
                spellCheck={false}
                value={draft.username}
                placeholder="public username"
                onChange={(event) => setDraft({ ...draft, username: event.target.value })}
              />
            </label>
            {meta.fields.map((field) => (
              <label key={field.key}>
                {field.label}
                <input
                  type={field.kind === "password" ? "password" : "text"}
                  autoComplete="off"
                  required={field.required}
                  placeholder={draft.hasToken ? "Saved in Keychain — leave blank to keep it" : field.placeholder}
                  value={draft.token}
                  onChange={(event) => setDraft({ ...draft, token: event.target.value })}
                />
                <span className="help">{field.help}</span>
                {draft.hasToken && (
                  <button
                    type="button"
                    className="text-btn danger"
                    onClick={() => {
                      setDraft({ ...draft, token: "", hasToken: false });
                      void saveAccount({
                        id: draft.id,
                        connectorId: draft.connectorId,
                        label: draft.label,
                        username: draft.username,
                        leadHours: draft.leadHours,
                        timezone: draft.timezone,
                        enabled: draft.enabled,
                        token: "",
                        tokenChanged: true,
                      }).then(() => load()).catch((err: unknown) => {
                        setError(typeof err === "string" ? err : "Couldn't remove the token.");
                      });
                    }}
                  >
                    Remove saved token
                  </button>
                )}
              </label>
            ))}
            <div className="split">
              <label>
                Remind me
                <span className="inline">
                  <input
                    type="number"
                    min={1}
                    max={24}
                    value={draft.leadHours}
                    onChange={(event) => setDraft({ ...draft, leadHours: Number(event.target.value) })}
                  />
                  <span>hours before midnight</span>
                </span>
              </label>
              <label>
                Time zone
                <input
                  value={draft.timezone}
                  placeholder={meta.defaultTimezone || "This Mac"}
                  onChange={(event) => setDraft({ ...draft, timezone: event.target.value })}
                />
                <span className="help">
                  Blank uses this Mac. LeetCode days reset at midnight UTC.
                </span>
              </label>
            </div>
            <label className="check">
              <input
                type="checkbox"
                checked={draft.enabled}
                onChange={(event) => setDraft({ ...draft, enabled: event.target.checked })}
              />
              Watch this streak
            </label>
            {testNote && <p className={testOk ? "note ok" : "note bad"}>{testNote}</p>}
            {error && <p className="note bad">{error}</p>}
            <div className="form-actions">
              <button type="button" className="ghost" disabled={busy !== ""} onClick={() => void runTest()}>
                {busy === "test" ? "Testing" : "Test"}
              </button>
              <button type="submit" className="primary" disabled={busy !== ""}>
                {busy === "save" ? "Saving" : "Save"}
              </button>
              {draft.id && (
                <button type="button" className="text-btn danger" disabled={busy !== ""} onClick={() => void remove()}>
                  {confirmRemove ? "Confirm remove" : "Remove"}
                </button>
              )}
              <button type="button" className="text-btn" onClick={() => setDraft(null)}>
                Cancel
              </button>
            </div>
          </form>
        )}
        <section className="quiet">
          <h2>Quiet hours</h2>
          <p>The orb stays hidden between these times. Checks still run.</p>
          <div className="split">
            <label>
              From
              <input
                type="time"
                value={settings.quietStart ?? ""}
                onChange={(event) => updateQuiet({ quietStart: event.target.value })}
              />
            </label>
            <label>
              Until
              <input
                type="time"
                value={settings.quietEnd ?? ""}
                onChange={(event) => updateQuiet({ quietEnd: event.target.value })}
              />
            </label>
          </div>
          <label className="check">
            <input
              type="checkbox"
              checked={settings.launchAtLogin}
              onChange={(event) => updateQuiet({ launchAtLogin: event.target.checked })}
            />
            Open Streak Orb when I log in
          </label>
          {error && !draft && <p className="note bad">{error}</p>}
        </section>
      </main>
    </div>
  );
}
