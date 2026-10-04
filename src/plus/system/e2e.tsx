import { expect, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { PlusViews } from "../PlusViews";
import type { Bridge } from "./testkit";

export const TAB_NAMES = ["Sync", "Updates", "Council", "Import", "Self-management"];

/** Walks to a tab the way a person does: the System screen, then its tab. Everything after
 * that runs through the real panels. Waits until the command list is read, so a write button
 * already has its policy. */
export async function openSystem(bridge: Bridge, tab = "Sync") {
  const user = userEvent.setup();
  render(<PlusViews view="system" onSelectView={() => {}} />);
  const tabs = await screen.findByRole("tablist", { name: "System sections" });
  if (tab !== "Sync") await user.click(within(tabs).getByRole("tab", { name: tab }));
  await screen.findByRole("tab", { name: tab, selected: true });
  await waitFor(() => expect(bridge.count("commands")).toBeGreaterThan(0));
  return user;
}

export const group = (name: string) => within(screen.getByRole("group", { name }));
export const button = (name: string | RegExp) => screen.getByRole("button", { name });

/** One confirmed write: wait for the plan, type the phrase when the tier asks for one,
 * confirm, wait for the result, close it. */
export async function confirm(
  user: UserEvent,
  label: string,
  options: { typed?: string; done?: string | RegExp } = {},
) {
  const box = await screen.findByRole("dialog", { name: /\?$/ });
  await within(box).findByRole("button", { name: label });
  if (options.typed) {
    expect(within(box).getByRole("button", { name: label })).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), options.typed);
  }
  await user.click(within(box).getByRole("button", { name: label }));
  if (options.done) await screen.findByText(options.done);
  const done = await screen.findByRole("dialog", { name: (name) => !name.endsWith("?") });
  await waitFor(() =>
    expect(within(done).getAllByRole("button", { name: "Close" }).length).toBeGreaterThan(
      1,
    ),
  );
  await user.click(within(done).getAllByRole("button", { name: "Close" }).at(-1)!);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
}

/** The typed value of every field and every attribute of the page; a secret must not be in it. */
export const pageText = () =>
  document.body.innerHTML +
  document.body.textContent +
  [...document.querySelectorAll("input,textarea")]
    .map((el) => (el as HTMLInputElement).value)
    .join("|");

export const offline = (value: boolean) =>
  vi.spyOn(navigator, "onLine", "get").mockReturnValue(!value);
