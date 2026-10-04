import {
  createOtelWorld,
  emptyUsage,
  otelBrowserFixtures,
  shiftDays,
  usageWorld,
} from "./world";

const yesterday = new Date(Date.now() - 86_400_000).toISOString().slice(0, 10);
const populated = () => shiftDays(usageWorld(), yesterday);

/** Replies of the dev browser fixture for what the Usage tab runs, keyed by argv joined with
 * spaces. `plusCtlFixtures` spreads this in. The usage figures end yesterday so the chart is
 * never empty, and an applied Enable or Disable changes the next OTel status. */
export const usageCtlFixtures = new Map<string, unknown>([
  ["usage", populated],
  ["usage --no-refresh", populated],
  ["usage --root /fixture/projects", populated],
  ["usage --no-refresh --root /fixture/projects", populated],
  ["usage --root /fixture/empty", emptyUsage],
  ["usage --no-refresh --root /fixture/empty", emptyUsage],
  ...otelBrowserFixtures(createOtelWorld()),
]);
