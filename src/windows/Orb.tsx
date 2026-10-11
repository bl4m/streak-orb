import { useEffect, useState } from "react";
import { dismissReminder, getReminder, listenReminder, openService, snooze } from "../api";
import { Character } from "../character";
import type { Reminder } from "../types";

export function Orb() {
  const [reminder, setReminder] = useState<Reminder | null>(null);
  const [leaving, setLeaving] = useState(false);

  useEffect(() => {
    void getReminder().then((current) => setReminder(current));
    let unlisten = () => {};
    void listenReminder(
      (next) => {
        setLeaving(false);
        setReminder(next);
      },
      () => setReminder(null),
    ).then((stop) => {
      unlisten = stop;
    });
    return () => unlisten();
  }, []);

  async function act(action: "snooze30" | "snooze60" | "open" | "dismiss") {
    if (!reminder) return;
    if (action === "open") {
      await openService(reminder.serviceUrl);
      return;
    }
    setLeaving(true);
    if (action === "snooze30") await snooze(reminder.accountId, 30);
    if (action === "snooze60") await snooze(reminder.accountId, 60);
    if (action === "dismiss") await dismissReminder(reminder.accountId);
    setReminder(null);
  }

  if (!reminder) {
    return <div className="orb-empty" />;
  }

  const mood = reminder.kind === "ended" ? "ended" : "risk";
  return (
    <div className={`orb-shell${leaving ? " leaving" : ""}`}>
      <div className="bubble" data-tauri-drag-region>
        <p className="kicker">{reminder.kind === "ended" ? "Streak ended" : "Still time today"}</p>
        <h1>{reminder.label}</h1>
        <p>{reminder.message}</p>
        <div className="orb-actions">
          {reminder.kind !== "ended" && (
            <>
              <button type="button" onClick={() => void act("snooze30")}>
                Snooze 30m
              </button>
              <button type="button" onClick={() => void act("snooze60")}>
                Snooze 1h
              </button>
            </>
          )}
          <button type="button" className="primary" onClick={() => void act("open")}>
            Open
          </button>
          <button type="button" onClick={() => void act("dismiss")}>
            Not now
          </button>
        </div>
      </div>
      <div data-tauri-drag-region>
        <Character mood={mood} />
      </div>
    </div>
  );
}
