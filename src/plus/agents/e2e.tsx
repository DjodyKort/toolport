import { useState } from "react";
import { expect } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import type { View } from "@/lib/types";
import { PlusViews } from "../PlusViews";
import { PLUS_VIEWS } from "../nav";

/** Walks to a tab the way a person does: the Library screen, its Agents or Styles tab, then
 * the screen that holds it. Everything after that runs through the real panels. */
export async function openFromLibrary(tab: "Agents" | "Styles") {
  const user = userEvent.setup();
  function Host() {
    const [view, setView] = useState<View>("library");
    const plus = PLUS_VIEWS.find((candidate) => candidate === view);
    return plus ? <PlusViews view={plus} onSelectView={setView} /> : <p>upstream</p>;
  }
  render(<Host />);
  await user.click(await screen.findByRole("tab", { name: tab }));
  await user.click(await screen.findByRole("button", { name: "Open Agents & styles" }));
  if (tab === "Styles")
    await user.click(await screen.findByRole("tab", { name: "Styles" }));
  return user;
}

/** One confirmed write: press the button, wait for the plan, type the phrase when the tier
 * asks for one, confirm, wait for the result, close it. */
export async function write(
  user: UserEvent,
  button: string,
  confirm: string,
  options: { typed?: string; done: string | RegExp },
) {
  await user.click(screen.getByRole("button", { name: button }));
  const box = await screen.findByRole("dialog");
  await within(box).findByRole("button", { name: confirm });
  if (options.typed) {
    expect(within(box).getByRole("button", { name: confirm })).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), options.typed);
  }
  await user.click(within(box).getByRole("button", { name: confirm }));
  await screen.findByText(options.done);
  const done = screen.getByRole("dialog");
  await user.click(within(done).getAllByRole("button", { name: "Close" }).at(-1)!);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
}
