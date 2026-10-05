import { expect } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import type { PlusView } from "../nav";
import { AttentionScreen } from "./AttentionScreen";
import type { Bridge } from "./testkit";

export type Navigate = (view: PlusView, params: Record<string, string>) => void;

/** Walks the way a person does: the Attention screen, once the list and the registry (the
 * policy of every action) have been read. */
export async function openAttention(
  bridge: Bridge,
  onNavigate: Navigate = () => {},
  onOpenCommands?: (group?: string) => void,
) {
  const user = userEvent.setup();
  render(<AttentionScreen onNavigate={onNavigate} onOpenCommands={onOpenCommands} />);
  await screen.findByRole("button", { name: "Check again" });
  await waitFor(() => expect(bridge.count("commands")).toBeGreaterThan(0));
  await waitFor(() => expect(bridge.count("attention ls")).toBeGreaterThan(0));
  return user;
}

export const group = (name: string) => screen.findByRole("region", { name });

export const rowOf = async (title: string) =>
  within(await screen.findByRole("listitem", { name: title }));

export async function dismissRow(user: UserEvent, title: string, choice?: string) {
  const row = await rowOf(title);
  await user.click(row.getByRole("button", { name: `Dismiss ${title}` }));
  const box = within(await screen.findByRole("dialog", { name: "Hide this row?" }));
  if (choice) await user.click(box.getByRole("radio", { name: choice }));
  await user.click(box.getByRole("button", { name: "Show plan" }));
}
