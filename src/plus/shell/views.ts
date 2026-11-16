import { BookOpen, ListTodo, Layers2, type LucideIcon } from "lucide-react";
import { msg, type SourceMessage } from "@/lib/i18n";
import type { View } from "@/lib/types";

/** Views the "Claude" group adds after upstream's six sidebar entries. */
export const PLUS_VIEWS = ["library", "context", "tasks"] as const;
export type PlusView = (typeof PLUS_VIEWS)[number];

/** Upstream pages the fork adds rows to; the parity manifest names them as `view:<id>` too. */
export const PLUS_EXTENDED_VIEWS = ["servers"] as const;

export const CLAUDE_GROUP_LABEL = msg("Claude");

/** The Settings page that holds the fork's own controls. */
export const TOOLPORT_PAGE = "toolport";

export interface PlusScreen {
  view: PlusView;
  /** Sidebar label; goes through upstream's i18n so the sidebar translates. */
  label: SourceMessage;
  icon: LucideIcon;
  title: string;
  subtitle: string;
  /** The slice that delivers the real screen, named in the empty state. */
  slice: string;
  what: string;
}

export const PLUS_SCREENS: Record<PlusView, PlusScreen> = {
  library: {
    view: "library",
    label: msg("Library"),
    icon: BookOpen,
    title: "Library",
    subtitle: "What Claude Code can load, and where it comes from",
    slice: "S4",
    what: "Sync your skills, agents and styles to Claude Code, with one plan to review before anything is written.",
  },
  context: {
    view: "context",
    label: msg("Context"),
    icon: Layers2,
    title: "Context",
    subtitle: "What Claude loads in a folder, and what it costs",
    slice: "S5",
    what: "See what loads in this folder, how many tokens it costs, and switch the biggest items off.",
  },
  tasks: {
    view: "tasks",
    label: msg("Tasks"),
    icon: ListTodo,
    title: "Tasks",
    subtitle: "Scheduled jobs",
    slice: "S6",
    what: "Run named jobs on a schedule or on demand, and read their history.",
  },
};

export function isPlusView(view: View): view is PlusView {
  return (PLUS_VIEWS as readonly string[]).includes(view);
}

/** True when `PlusViews` renders the content: one of the group's pages, or the Toolport+ page of
 * Settings. */
export function isPlusRoute(view: View, settingsPage: string): boolean {
  return isPlusView(view) || (view === "settings" && settingsPage === TOOLPORT_PAGE);
}

/** Header text for the group's own pages; `null` leaves the header to the app. */
export function plusPageHeader(view: View): { title: string; subtitle: string } | null {
  return isPlusView(view) ? PLUS_SCREENS[view] : null;
}
