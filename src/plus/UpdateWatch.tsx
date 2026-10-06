import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { plusUpdateWatchSettings, plusUpdateWatchTick } from "./api";
import type { UpdateWatchSettings } from "./api";

const POLL_MS = 30 * 60_000;
const TOAST_MS = 30_000;

/** Headless: asks the backend for a scheduled server update check. The backend keeps the
 * interval, the off switch and what it already told you, so a poll that is not due, or a
 * state that did not change, raises nothing. A hidden window is skipped. */
export function UpdateWatcher({ onReview }: { onReview?: () => void }) {
  const review = useRef(onReview);
  useEffect(() => {
    review.current = onReview;
  });

  useEffect(() => {
    let alive = true;
    const poll = () => {
      if (document.visibilityState === "hidden") return;
      plusUpdateWatchTick()
        .then((result) => {
          if (!alive) return;
          for (const found of result.raised) {
            toast.info(found.title, {
              id: `updates:${found.server}:${found.kind}:${found.signature}`,
              description: found.detail,
              duration: TOAST_MS,
              action: review.current
                ? { label: "Review", onClick: () => review.current?.() }
                : undefined,
            });
          }
        })
        .catch(() => {});
    };
    poll();
    const timer = setInterval(poll, POLL_MS);
    document.addEventListener("visibilitychange", poll);
    return () => {
      alive = false;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", poll);
    };
  }, []);

  return null;
}

const INTERVALS: { hours: number; label: string }[] = [
  { hours: 6, label: "Every 6 hours" },
  { hours: 24, label: "Daily" },
  { hours: 24 * 7, label: "Weekly" },
];

export function UpdateWatchSettingsView({
  settings,
  onChange,
}: {
  settings: UpdateWatchSettings;
  onChange: (patch: Partial<UpdateWatchSettings>) => void;
}) {
  const known = INTERVALS.some((i) => i.hours === settings.intervalHours);
  return (
    <section aria-label="Update watch" className="flex flex-col gap-2">
      <header className="flex items-center gap-2">
        <h3 className="text-sm font-medium">Server update check</h3>
        <label className="ml-auto flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={settings.enabled}
            onChange={(e) => onChange({ enabled: e.target.checked })}
          />
          Enabled
        </label>
      </header>
      <p className="text-xs text-muted-foreground">
        Checks every server for new fork or upstream commits, newer package versions and
        sync conflicts, and tells you only when something changed.
      </p>
      <label className="flex items-center gap-2 text-xs">
        Check
        <select
          aria-label="Check interval"
          disabled={!settings.enabled}
          value={settings.intervalHours}
          onChange={(e) => onChange({ intervalHours: Number(e.target.value) })}
          className="rounded border bg-background px-1.5 py-1"
        >
          {!known && (
            <option value={settings.intervalHours}>
              Every {settings.intervalHours} hours
            </option>
          )}
          {INTERVALS.map((i) => (
            <option key={i.hours} value={i.hours}>
              {i.label}
            </option>
          ))}
        </select>
      </label>
    </section>
  );
}

export function UpdateWatchSection() {
  const [settings, setSettings] = useState<UpdateWatchSettings | null>(null);
  useEffect(() => {
    let alive = true;
    plusUpdateWatchSettings()
      .then((s) => alive && setSettings(s))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);
  if (!settings) return null;
  const change = (patch: Partial<UpdateWatchSettings>) => {
    plusUpdateWatchSettings(patch)
      .then(setSettings)
      .catch(() => {});
  };
  return <UpdateWatchSettingsView settings={settings} onChange={change} />;
}
