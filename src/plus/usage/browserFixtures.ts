import { emptyUsage, otelBrowserFixtures, usageWorld } from "./world";

/** Replies of the dev browser fixture for what the Usage tab runs, keyed by argv joined with
 * spaces. `plusCtlFixtures` spreads this in. */
export const usageCtlFixtures = new Map<string, unknown>([
  ["usage", usageWorld()],
  ["usage --no-refresh", usageWorld()],
  ["usage --root /fixture/projects", usageWorld()],
  ["usage --no-refresh --root /fixture/projects", usageWorld()],
  ["usage --root /fixture/empty", emptyUsage()],
  ["usage --no-refresh --root /fixture/empty", emptyUsage()],
  ...otelBrowserFixtures,
]);
