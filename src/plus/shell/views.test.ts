import { describe, expect, it } from "vitest";
import {
  isPlusRoute,
  isPlusView,
  plusPageHeader,
  PLUS_SCREENS,
  PLUS_VIEWS,
} from "./views";

describe("plus views", () => {
  it("test01_the_group_adds_library_context_tasks_and_spend_after_upstreams_six", () => {
    expect([...PLUS_VIEWS]).toEqual(["library", "context", "tasks", "spend"]);
    for (const view of [
      "servers",
      "clients",
      "skills",
      "activity",
      "settings",
      "teams",
    ]) {
      expect(isPlusView(view as never)).toBe(false);
    }
  });

  it("test02_every_page_of_the_group_has_a_title_and_a_subtitle", () => {
    for (const view of PLUS_VIEWS) {
      expect(PLUS_SCREENS[view].title.length).toBeGreaterThan(0);
      expect(PLUS_SCREENS[view].subtitle.length).toBeGreaterThan(0);
    }
    expect(PLUS_SCREENS.tasks.subtitle).toBe("Named jobs Toolport can run for you");
  });

  it("test03_plus_routes_are_the_group_pages_and_the_toolport_settings_page", () => {
    expect(isPlusRoute("library", "general")).toBe(true);
    expect(isPlusRoute("settings", "toolport")).toBe(true);
    expect(isPlusRoute("settings", "general")).toBe(false);
    expect(isPlusRoute("servers", "toolport")).toBe(false);
  });

  it("test04_the_settings_page_keeps_upstreams_header", () => {
    expect(plusPageHeader("settings")).toBeNull();
    expect(plusPageHeader("tasks")?.title).toBe("Tasks");
  });
});
