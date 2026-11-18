import type { CtlReply } from "./ctlWorld";

const PARTS = [
  {
    id: "repos",
    label: "Repositories",
    count: 3,
    notes: ["1 without a remote cannot be cloned again"],
  },
  { id: "servers", label: "Servers", count: 2, notes: [] },
  { id: "settings", label: "Settings", count: 1, notes: [] },
];

/** The setup commands against a fixed file and one reset checklist. */
export function createSetupWorld() {
  const carry = ["browser", "keys", "licenses", "outside"].map((id) => ({
    id,
    label: id,
    ticked: false,
  }));
  const acked = new Set<string>();
  const finding = (id: string, kind: string, note: string) => ({
    id,
    kind,
    repo: id.split(":")[1],
    note,
    acked: acked.has(id),
  });
  let restored = false;
  const rows = () => [
    {
      id: "repo:~/code/web",
      label: "web",
      part: "repos",
      state: restored ? "here" : "add",
      note: "",
    },
    {
      id: "server:github",
      label: "github",
      part: "servers",
      state: "needs",
      note: "needs a secret",
    },
  ];

  return {
    run(key: string[], stdin?: string): CtlReply {
      const [, verb] = key;
      const flag = (name: string) => key[key.indexOf(name) + 1];
      if (verb === "parts") {
        return {
          ok: true,
          data: {
            encryption: true,
            minPassphrase: 12,
            lastExport: null,
            parts: PARTS.map((p) => ({
              ...p,
              default: true,
              sealed: false,
              error: null,
            })),
          },
        };
      }
      if (verb === "export") {
        if (key.includes("--secrets") && !stdin) {
          return {
            ok: false,
            error: { code: "usage", message: "the passphrase comes on stdin" },
          };
        }
        const dryRun = key.includes("--dry-run");
        return {
          ok: true,
          data: {
            dryRun,
            file: `${flag("--out")}/toolport-setup.zip`,
            name: "toolport-setup.zip",
            fileSize: dryRun ? 0 : 3674,
            parts: (flag("--parts") ?? "").split(",").map((id) => ({ id })),
          },
        };
      }
      if (verb === "restore") {
        const apply = key.includes("--apply");
        if (apply && flag("--plan") !== "fixture-plan") {
          return {
            ok: false,
            error: { code: "stale_plan", message: "the file changed since the plan" },
          };
        }
        if (apply) restored = true;
        return {
          ok: true,
          data: {
            applied: apply,
            dryRun: !apply,
            plan: "fixture-plan",
            rows: rows(),
            summary: {
              add: restored ? 0 : 1,
              here: restored ? 1 : 0,
              conflict: 0,
              needs: 1,
              skip: 0,
            },
          },
        };
      }
      if (verb === "undo") {
        if (!restored)
          return { ok: false, error: { code: "nothing", message: "nothing to undo" } };
        restored = false;
        return { ok: true, data: { undone: true } };
      }
      if (verb === "check") {
        for (const [flagName, on] of [
          ["--tick", true],
          ["--untick", false],
        ] as const) {
          const id = flag(flagName);
          const item = carry.find((c) => c.id === id);
          if (key.includes(flagName) && item) item.ticked = on;
        }
        if (key.includes("--ack")) acked.add(flag("--ack"));
        const blockers = [
          finding("unpushed:~/code/web", "unpushed", "1 commits are on no remote"),
        ];
        const open = blockers.filter((b) => !b.acked).length;
        return {
          ok: true,
          data: {
            blockers,
            amber: [],
            carry,
            lastExport: null,
            open: { blockers: open, carry: carry.filter((c) => !c.ticked).length },
            ready: open === 0 && carry.every((c) => c.ticked),
          },
        };
      }
      return {
        ok: false,
        error: { code: "usage", message: `unknown setup verb ${verb}` },
      };
    },
  };
}
