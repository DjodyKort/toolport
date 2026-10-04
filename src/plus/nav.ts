import {
  Activity,
  AlignLeft,
  Bell,
  BookOpen,
  ChartColumn,
  FileText,
  FlaskConical,
  Layers,
  MonitorCog,
  Play,
  ScrollText,
  Server,
  Settings,
  ShieldCheck,
  Store,
  Users,
  type LucideIcon,
} from "lucide-react";
import type { View } from "@/lib/types";

/** Views that `PlusViews` renders. The other entries of the sidebar are the upstream views. */
export const PLUS_VIEWS = [
  "control",
  "logins",
  "agents",
  "attention",
  "library",
  "context",
  "tokens",
  "tasks",
  "system",
  "commands",
] as const;

export type PlusView = (typeof PLUS_VIEWS)[number];

export function isPlusView(view: string): view is PlusView {
  return (PLUS_VIEWS as readonly string[]).includes(view);
}

export interface NavItem {
  label: string;
  view: View;
  icon: LucideIcon;
  /** `attention` shows the Attention counter, `quarantine` the blocked-tools count. */
  badge?: "attention" | "quarantine";
}

export interface NavGroup {
  label: string | null;
  items: NavItem[];
}

/** The grouped sidebar B of the approved mockup (D-069): Attention on top, then the groups
 * Servers, Claude, Agents and More, 16 items. Entries that name an upstream view keep it
 * until a Toolport+ screen replaces it. */
export const NAV_GROUPS: NavGroup[] = [
  {
    label: null,
    items: [{ label: "Attention", view: "attention", icon: Bell, badge: "attention" }],
  },
  {
    label: "Servers",
    items: [
      { label: "Servers", view: "control", icon: Layers },
      { label: "Clients", view: "clients", icon: MonitorCog },
      { label: "Browse catalog", view: "catalog", icon: Store },
      { label: "Playground", view: "playground", icon: FlaskConical },
    ],
  },
  {
    label: "Claude",
    items: [
      { label: "Library", view: "library", icon: BookOpen },
      { label: "Context", view: "context", icon: AlignLeft },
      { label: "Tokens", view: "tokens", icon: ChartColumn },
      { label: "Tasks", view: "tasks", icon: Play },
    ],
  },
  {
    label: "Agents",
    items: [
      { label: "Agent rules", view: "rules", icon: FileText },
      { label: "Agent activity", view: "hooks", icon: Activity },
      { label: "Agent permissions", view: "permissions", icon: ShieldCheck },
    ],
  },
  {
    label: "More",
    items: [
      { label: "Activity", view: "activity", icon: ScrollText },
      { label: "Teams", view: "teams", icon: Users },
      { label: "System", view: "system", icon: Server },
      {
        label: "Settings",
        view: "settings",
        icon: Settings,
        badge: "quarantine",
      },
    ],
  },
];

/** A view that is reached from another entry and keeps that entry highlighted. The upstream
 * `servers` page stays reachable as the Classic view of the Servers screen. */
const LIVES_UNDER: Partial<Record<View, View>> = {
  commands: "settings",
  servers: "control",
  logins: "control",
  agents: "library",
};

export function navItemActive(item: NavItem, view: View): boolean {
  return view === item.view || LIVES_UNDER[view] === item.view;
}

export interface PlusScreen {
  title: string;
  subtitle: string;
  /** The item that builds the screen. */
  builtBy: string;
}

/** Titles and subtitles as drawn in the mockup, for the header of the app and for the
 * "not built yet" screens. */
export const PLUS_SCREENS: Record<PlusView, PlusScreen> = {
  control: {
    title: "Servers",
    subtitle: "Every server, its login and the gateway",
    builtBy: "MIG-GUI-1",
  },
  logins: {
    title: "Logins & secrets",
    subtitle: "Which servers are signed in, and the secrets behind them",
    builtBy: "MIG-GUI-2",
  },
  agents: {
    title: "Agents & styles",
    subtitle: "Sub-agents and output styles, written once and synced to every client",
    builtBy: "MIG-GUI-4",
  },
  attention: {
    title: "Needs attention",
    subtitle: "Everything across Toolport that wants a decision, in one list",
    builtBy: "MIG-GUI-11",
  },
  library: {
    title: "Library",
    subtitle: "Skills, agents and styles, and where each comes from",
    builtBy: "MIG-GUI-3",
  },
  context: {
    title: "Context",
    subtitle: "What Claude loads in a folder, and how to shape it",
    builtBy: "MIG-GUI-6",
  },
  tokens: {
    title: "Tokens",
    subtitle: "What Claude costs, and how tool output is shortened",
    builtBy: "MIG-GUI-7",
  },
  tasks: {
    title: "Tasks",
    subtitle:
      "Named jobs Toolport can run: for you, on a schedule, when a login fails, or when Claude asks",
    builtBy: "MIG-AUTO-2",
  },
  system: {
    title: "System",
    subtitle: "Sync, updates, plugins, council, import and the self-management MCP",
    builtBy: "MIG-GUI-8",
  },
  commands: {
    title: "All commands",
    subtitle: "Every toolportctl command, generated from the registry",
    builtBy: "MIG-GUI-0",
  },
};
