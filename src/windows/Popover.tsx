import { useEffect, useState } from "react";
import { getDashboard, listenUpdated, openService, quitApp, refreshNow, showSettings } from "../api";
import { Character } from "../character";
import type { AccountView, Dashboard } from "../types";

function formatChecked(iso: string | null): string {
  if (!iso) return "Not checked yet";
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "Not checked yet";
  const mins = Math.max(0, Math.round((Date.now() - then) / 60000));
  if (mins < 1) return "Checked just now";
  if (mins < 60) return `Checked ${mins}m ago`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `Checked ${hours}h ago`;
  return `Checked ${new Date(iso).toLocaleDateString()}`;
}

function formatNext(iso: string | null, checking: boolean): string {
  if (checking) return "Checking now";
  if (!iso) return "Waiting for the first check";
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "Next check soon";
  const mins = Math.round((then - Date.now()) / 60000);
  if (mins <= 1) return "Next check in a minute";
  if (mins < 60) return `Next check in ${mins}m`;
  return `Next check ${new Date(iso).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}`;
}

function statusLine(account: AccountView): string {
  if (account.status === "error") return account.detail;
  if (account.status === "unchecked") return "Not checked yet";
  if (account.status === "ok") return `Done today · ${account.streak}-day streak`;
  if (account.status === "at_risk" || account.status === "waiting") {
    const left = account.timeLeft ? ` · ${account.timeLeft}` : "";
    const length = account.streak > 0 ? `${account.streak}-day streak` : "No days yet";
    return `${length}${left}`;
  }
  if (account.status === "ended" && account.freshBreak) return "Streak ended";
  if (account.status === "ended") return "No current streak";
  return account.detail;
}

export function Popover() {
  const [dash, setDash] = useState<Dashboard | null>(null);
  const [error, setError] = useState("");

  async function load() {
    try {
      setDash(await getDashboard());
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

  async function refresh() {
    setDash((current) => (current ? { ...current, checking: true } : current));
    try {
      setDash(await refreshNow());
    } catch (err) {
      setError(typeof err === "string" ? err : "Couldn't check right now.");
    }
  }

  return (
    <div className="popover-shell">
      <section className="panel">
        <header className="panel-head">
          <div>
            <p className="kicker">Today</p>
            <h1>Streaks</h1>
          </div>
          <button type="button" className="text-btn" onClick={() => void refresh()}>
            {dash?.checking ? "Checking" : "Check"}
          </button>
        </header>
        <p className="subline">{formatNext(dash?.nextCheckAt ?? null, Boolean(dash?.checking))}</p>
        {error && <p className="banner">{error}</p>}
        <div className="card-list">
          {dash && dash.accounts.length === 0 && (
            <div className="empty">
              <Character mood="calm" />
              <p>No streaks yet. Add Duolingo, GitHub, LeetCode, or Codeforces.</p>
            </div>
          )}
          {dash?.accounts.map((account) => (
            <button
              key={account.id}
              type="button"
              className={`card status-${account.status}${account.enabled ? "" : " is-off"}`}
              onClick={() => void openService(account.serviceUrl)}
            >
              <span className={`dot status-${account.status}`} />
              <span>
                <strong>{account.label}</strong>
                <em>{statusLine(account)}</em>
                <em className="muted">{formatChecked(account.checkedAt)}</em>
              </span>
            </button>
          ))}
        </div>
        <footer className="panel-foot">
          <button type="button" className="primary" onClick={() => void showSettings()}>
            Edit streaks
          </button>
          <button type="button" className="text-btn" onClick={() => void quitApp()}>
            Quit
          </button>
        </footer>
      </section>
    </div>
  );
}
