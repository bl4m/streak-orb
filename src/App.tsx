import { useEffect, useState } from "react";
import { inTauri } from "./types";
import { Orb } from "./windows/Orb";
import { Popover } from "./windows/Popover";
import { Settings } from "./windows/Settings";

function hashLabel(): string {
  const hash = location.hash.replace("#", "");
  if (hash === "settings" || hash === "orb" || hash === "popover") return hash;
  return "popover";
}

export function App() {
  const [label, setLabel] = useState(inTauri() ? "popover" : hashLabel());

  useEffect(() => {
    if (!inTauri()) {
      const apply = () => setLabel(hashLabel());
      window.addEventListener("hashchange", apply);
      return () => window.removeEventListener("hashchange", apply);
    }
    let cancelled = false;
    void import("@tauri-apps/api/window").then(({ getCurrentWindow }) => {
      if (!cancelled) setLabel(getCurrentWindow().label);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className={`window window-${label}`}>
      {label === "settings" && <Settings />}
      {label === "orb" && <Orb />}
      {label !== "settings" && label !== "orb" && <Popover />}
      {!inTauri() && (
        <nav className="preview-nav">
          <a href="#popover">Popover</a>
          <a href="#settings">Settings</a>
          <a href="#orb">Orb</a>
        </nav>
      )}
    </div>
  );
}
