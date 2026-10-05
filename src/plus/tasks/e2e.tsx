import { expect } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { TasksScreen } from "./TasksScreen";
import type { Bridge } from "./testkit";

/** Walks the way a person does: the Tasks screen, then a task, then its button. The polling
 * is fast so a run that takes a few reads finishes within a test. */
export async function openTasks(bridge: Bridge, tab?: string) {
  const user = userEvent.setup();
  render(<TasksScreen pollMs={15} />);
  const tabs = await screen.findByRole("tablist", { name: "Tasks sections" });
  if (tab) await user.click(within(tabs).getByRole("tab", { name: tab }));
  await waitFor(() => expect(bridge.count("commands")).toBeGreaterThan(0));
  return user;
}

export async function pick(user: UserEvent, name: RegExp) {
  const list = await screen.findByRole("list", { name: "Tasks" });
  await user.click(within(list).getByRole("button", { name }));
}

export async function startRun(user: UserEvent) {
  await user.click(await screen.findByRole("button", { name: "Run now…" }));
  const box = await screen.findByRole("dialog", { name: /^Run .*\?$/ });
  await user.click(await within(box).findByRole("button", { name: "Run now" }));
  return within(await screen.findByRole("dialog", { name: /^Run [^?]*$/ }));
}

export const calls = (bridge: Bridge, verb: string) =>
  bridge.ran().filter((argv) => argv.startsWith(verb));
