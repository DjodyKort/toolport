import { describe, expect, it } from "vitest";
import { ctlShapes } from "../bridge/data";
import { check } from "../bridge/shape";
import { ctlTypeShapes } from "../types";
import { agentsCtlFixtures } from "./agents";
import { createAgentsWorld } from "./agentsWorld";

const shapes = { ...ctlShapes, ...ctlTypeShapes };
const WRITES = new Set(["add", "sync", "clean", "uninstall", "apply", "remove"]);

/** `agents sync --dry-run` is the golden `agents-sync.preview`; a read is its own golden. */
function stem(argv: string) {
  const words = argv.split(" ");
  const base = `${words[0]}-${words[1]}`;
  return WRITES.has(words[1])
    ? `${base}.${argv.endsWith("--dry-run") ? "preview" : "apply"}`
    : base;
}

const reads = [...agentsCtlFixtures.keys()].filter(
  (argv) => !WRITES.has(argv.split(" ")[1]),
);
const everyRow = () => [...createAgentsWorld().keys()];

describe("the stateful agents and styles world", () => {
  it("has every row of the static map and more, and its reads and creates start out the same", () => {
    const world = createAgentsWorld();
    expect(everyRow()).toEqual(expect.arrayContaining([...agentsCtlFixtures.keys()]));
    for (const [argv, data] of agentsCtlFixtures) {
      const verb = argv.split(" ")[1];
      if (verb === "add" || !WRITES.has(verb))
        expect(world.get(argv)!(), argv).toEqual(data);
    }
  });

  it("answers every row in the shape of the golden envelope, before and after a full walk", () => {
    const world = createAgentsWorld();
    const answer = (argv: string) => {
      const data = world.get(argv)!();
      const shape = shapes[stem(argv)];
      expect(shape, `no shape for ${stem(argv)}`).toBeDefined();
      expect(check(shape, data), argv).toEqual([]);
      return data;
    };
    const walk = [
      ...everyRow().filter((argv) => argv.endsWith("--dry-run")),
      "agents sync",
      ...reads,
      "agents clean",
      ...reads,
      "agents add reviewer",
      "agents sync --dry-run",
      "agents uninstall scout",
      ...reads,
      "styles add terse",
      "styles sync",
      "styles apply terse",
      ...reads,
      "styles remove",
      "styles clean",
      ...reads,
    ];
    for (const argv of walk) answer(argv);
  });

  it("changes what the next read answers only after the write is applied", () => {
    const world = createAgentsWorld();
    const read = (argv: string) => world.get(argv)!() as Record<string, unknown>;
    read("agents sync --dry-run");
    expect(read("agents status").lockfilePresent).toBe(false);
    read("agents sync");
    expect(read("agents status")).toMatchObject({ lockfilePresent: true, drift: false });
    read("agents clean --dry-run");
    expect(read("agents status").drift).toBe(false);
    read("agents clean");
    expect(read("agents status")).toMatchObject({ lockfilePresent: true, drift: true });
    expect(read("styles ls").styles).toEqual([]);
    read("styles add terse --dry-run");
    expect(read("styles ls").styles).toEqual([]);
    read("styles add terse");
    expect(read("styles ls").styles).toHaveLength(1);
    read("styles apply terse");
    expect(read("styles ls").active).toHaveLength(4);
    read("styles remove");
    expect(read("styles ls").active).toEqual([]);
    expect(createAgentsWorld().get("styles ls")!()).toMatchObject({ styles: [] });
  });
});
