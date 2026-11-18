import { ctlData } from "../bridge/ctl";
import type {
  AttentionCheckName,
  AttentionCheckReport,
  AttentionData,
  AttentionHideResult,
  AttentionSettings,
} from "../bridge/data";

export const ATTENTION_REFRESH_EVENT = "plus-attention-refresh";
const WEEK = "7d";

/** Tells the bell's feed to read the list again now, after a change or a check. */
export function refreshAttention() {
  window.dispatchEvent(new Event(ATTENTION_REFRESH_EVENT));
}

export type HideChoice =
  | { kind: "snooze"; label: string; args: string[] }
  | { kind: "skip"; label: string; args: string[] }
  | { kind: "dismiss"; label: string; args: string[] };

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

/** An RFC 3339 time with the local offset, so `--until` ends at 08:00 where the person is and not
 * at 08:00 UTC. */
export function localTimestamp(at: Date): string {
  const offset = -at.getTimezoneOffset();
  const sign = offset < 0 ? "-" : "+";
  const hours = pad(Math.trunc(Math.abs(offset) / 60));
  const minutes = pad(Math.abs(offset) % 60);
  return (
    `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}` +
    `T${pad(at.getHours())}:${pad(at.getMinutes())}:00${sign}${hours}:${minutes}`
  );
}

/** The next 08:00 on a day `days` ahead, or on the next Monday when `monday` is set. */
export function morning(now: Date, days: number, monday = false): Date {
  const at = new Date(now);
  at.setHours(8, 0, 0, 0);
  if (monday) {
    const ahead = (8 - at.getDay()) % 7 || 7;
    at.setDate(at.getDate() + ahead);
  } else {
    at.setDate(at.getDate() + days);
  }
  return at;
}

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

export function snoozeChoices(now: Date, monday: boolean): HideChoice[] {
  const tomorrow = morning(now, 1);
  const choices: HideChoice[] = [];
  if (!monday) {
    choices.push({ kind: "snooze", label: "Snooze 1 hour", args: ["--for", "1h"] });
    choices.push({
      kind: "snooze",
      label: "Snooze until tomorrow 08:00",
      args: ["--until", localTimestamp(tomorrow)],
    });
  } else {
    const next = morning(now, 0, true);
    choices.push({
      kind: "snooze",
      label: `Snooze until ${WEEKDAYS[next.getDay()]} 08:00`,
      args: ["--until", localTimestamp(next)],
    });
  }
  choices.push({ kind: "snooze", label: "Snooze for a week", args: ["--for", WEEK] });
  return choices;
}

export function hideRow(
  kind: HideChoice["kind"],
  id: string,
  args: string[] = [],
): Promise<AttentionHideResult> {
  return ctlData<AttentionHideResult>(["attention", kind, id, ...args]);
}

export function restoreRow(id: string) {
  return ctlData<{ id: string }>(["attention", "restore", id]);
}

export function undoLast() {
  return ctlData<{ id: string }>(["attention", "undo"]);
}

export function listWithHidden() {
  return ctlData<AttentionData>(["attention", "ls", "--hidden"]);
}

export function readSettings() {
  return ctlData<AttentionSettings>(["attention", "settings"]);
}

export function setSwitch(
  key: "notify" | `${"check" | "notify"}.${AttentionCheckName}`,
  on: boolean,
) {
  return ctlData<{ settings: AttentionSettings }>([
    "attention",
    "set",
    key,
    on ? "on" : "off",
  ]);
}

export function checkNow() {
  return ctlData<AttentionCheckReport>(["attention", "check"]);
}

export function sendTestNotification() {
  return ctlData<{ sent: boolean }>(["attention", "notify", "--test"]);
}
