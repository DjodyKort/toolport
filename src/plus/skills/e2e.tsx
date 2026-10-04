import { expect } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { PlusViews } from "../PlusViews";

/** Walks to the Skills tab the way a person does: the Library screen, then the section of
 * the tab. Everything after that runs through the real panels. */
export async function openLibrary(
  section?: "Taps" | "Find and install" | "Bundles",
  wait = true,
) {
  const user = userEvent.setup();
  render(<PlusViews view="library" onSelectView={() => {}} />);
  await screen.findByRole("tablist", { name: "Library sections" });
  if (wait) await screen.findByRole("list", { name: "Skills" });
  if (section) await user.click(await screen.findByRole("tab", { name: section }));
  return user;
}

/** One confirmed write: press the button, wait for the plan, type the phrase when the tier
 * asks for one, confirm, wait for the result, close it. `first` is the button of a form that
 * comes before the plan (the client picker of a sync). */
export async function write(
  user: UserEvent,
  button: string | RegExp,
  confirm: string,
  options: { typed?: string; done: string | RegExp; first?: string },
) {
  const opener = screen.getByRole("button", { name: button });
  await waitFor(() => expect(opener).toBeEnabled());
  await user.click(opener);
  if (options.first) {
    const form = await screen.findByRole("dialog");
    await user.click(within(form).getByRole("button", { name: options.first }));
  }
  const box = await screen.findByRole("dialog", { name: /\?$/ });
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
