import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const assets = join(__dirname, "../../docs/assets");

function indexRows(): string[][] {
  return readFileSync(join(assets, "gui-index.md"), "utf8")
    .split("\n")
    .filter((line) => /^\| `[^`]+\.png`\s*\|/.test(line))
    .map((line) =>
      line
        .slice(1, line.lastIndexOf("|"))
        .split("|")
        .map((cell) => cell.trim()),
    );
}

describe("gui screenshot index", () => {
  it("lists every PNG of docs/assets and no file that is not there", () => {
    const listed = indexRows().map((row) => row[0].replace(/`/g, ""));
    const onDisk = readdirSync(assets).filter((name) => name.endsWith(".png"));
    expect(onDisk.length).toBeGreaterThan(100);
    expect(
      onDisk.filter((name) => !listed.includes(name)),
      "PNGs missing from docs/assets/gui-index.md",
    ).toEqual([]);
    expect(
      listed.filter((name) => !existsSync(join(assets, name))),
      "gui-index.md rows without a file",
    ).toEqual([]);
    expect(listed.filter((name, i) => listed.indexOf(name) !== i)).toEqual([]);
  });

  it("describes each shot with a screen, a state, a theme and the item that took it", () => {
    for (const [file, screen, tab, state, theme, item] of indexRows()) {
      const where = `${file}: `;
      expect(screen, where + "screen").not.toBe("");
      expect(tab, where + "tab").not.toBe("");
      expect(state, where + "state").not.toBe("");
      expect(["light", "dark"], where + "theme").toContain(theme);
      expect(item, where + "item").toMatch(/^MIG-[A-Z]+-\d+$/);
      expect(
        file.includes("-dark.") ? "dark" : "light",
        where + "theme matches the name",
      ).toBe(theme);
    }
  });
});
