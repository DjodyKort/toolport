export interface SlotTab {
  id: "logins" | "secrets" | "integrations";
  label: string;
  builtBy: string;
  /** The command group the All commands page offers until the tab exists. */
  group: string;
}

/** The tabs of the mockup's Servers screen that MIG-GUI-2 builds. They stay in the tab list
 * so the screen has its final shape; MIG-GUI-2 replaces `Slot` with its component. */
export const SLOT_TABS: SlotTab[] = [
  { id: "logins", label: "Logins", builtBy: "MIG-GUI-2", group: "auth" },
  { id: "secrets", label: "Secrets", builtBy: "MIG-GUI-2", group: "secret" },
  { id: "integrations", label: "Integrations", builtBy: "MIG-GUI-2", group: "auth" },
];
