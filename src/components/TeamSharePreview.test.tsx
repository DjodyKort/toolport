import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TeamSharePreview } from "./TeamSharePreview";
import { teamShareAction } from "@/lib/teamShare";
import type { ShareSelectionPreview, TeamPushPreview } from "@/lib/api";

const preview: TeamPushPreview = {
  baseVersion: 7,
  localFingerprint: "same-payload",
  added: ["Workspace"],
  changed: ["Remote"],
  removed: ["Old server"],
  definitions: [
    {
      id: "local",
      name: "Workspace",
      change: "Added",
      transport: "stdio",
      fields: [
        { label: "Command", value: "python3" },
        {
          label: "Arguments",
          value: '"/home/test/long project/server.py"\n"--flag"\n"two words"',
        },
        { label: "Working directory", value: "/home/test/long project" },
        { label: "Environment / credential keys", value: "GITHUB_TOKEN, WORKSPACE" },
      ],
    },
    {
      id: "remote",
      name: "Remote",
      change: "Changed",
      transport: "http",
      fields: [
        { label: "Endpoint", value: "https://example.internal/mcp" },
        { label: "Environment / credential keys", value: "API_TOKEN" },
      ],
    },
  ],
  selections: [],
};

function selection(
  name: string,
  teamChange: ShareSelectionPreview["teamChange"],
  outcome: ShareSelectionPreview["local"]["outcome"],
  message: string,
): ShareSelectionPreview {
  const id = name.toLowerCase();
  return {
    id,
    name,
    teamChange,
    teamDetail: `${teamChange} detail.`,
    notes: [],
    local: { id, name, outcome, message },
  };
}

const alreadyShared: TeamPushPreview = {
  ...preview,
  added: [],
  changed: [],
  removed: [],
  definitions: [],
  selections: [
    {
      ...selection(
        "Linear",
        "Already shared",
        "switched",
        "This profile switches to the Team copy. Your personal server stays saved and turns off here.",
      ),
      notes: [
        "The team also has a separate definition named Linear (ID linear-2). It stays separate because sharing matches server IDs, not names.",
      ],
    },
    selection(
      "Vercel",
      "Already shared",
      "attention",
      "This team copy already has its own local credentials. Keep its existing setup and enable it separately. Your personal server stays on in this profile.",
    ),
  ],
};

describe("Team share definition preview", () => {
  it("shows resulting definitions and removal separately with sharing boundaries", () => {
    render(<TeamSharePreview preview={preview} />);
    for (const definition of preview.definitions) {
      for (const field of definition.fields)
        expect(
          screen.getByText(field.value, { exact: true, normalizer: (s) => s }),
        ).toBeInTheDocument();
    }
    for (const text of ["Added (1)", "Changed (1)", "Removed (1)", "Old server"])
      expect(screen.getByText(text)).toBeInTheDocument();
    expect(screen.getByText(/Your personal servers remain saved/)).toBeInTheDocument();
    expect(
      screen.getByText(/Other team servers, instructions and policies stay unchanged/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Each member uses their own credentials locally/),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Old server").closest("section")?.querySelector("details"),
    ).toBeNull();
  });

  it("does not render extraneous credential values even if present beside the display allowlist", () => {
    const contaminated = {
      ...preview,
      env: [{ key: "API_TOKEN", value: "SYNTHETIC_ENV_SECRET" }],
      oauthToken: "SYNTHETIC_OAUTH_SECRET",
      definitions: preview.definitions.map((d) => ({
        ...d,
        clientSecret: "SYNTHETIC_CLIENT_SECRET",
        resolvedArgs: ["SYNTHETIC_LAUNCH_SECRET"],
      })),
    };
    const { container } = render(<TeamSharePreview preview={contaminated} />);
    expect(container.innerHTML).not.toMatch(/SYNTHETIC_.*SECRET/);
    expect(screen.getByText("API_TOKEN")).toBeInTheDocument();
  });

  it("explains each selection's relationship to the team and its local route", () => {
    render(<TeamSharePreview preview={alreadyShared} />);
    expect(screen.getByText("Linear · Already shared")).toBeInTheDocument();
    expect(screen.getByText(/\(ID linear-2\)/)).toBeInTheDocument();
    expect(screen.getByText(/Your personal server stays on in this profile/)).toHaveClass(
      "text-destructive",
    );
    expect(
      screen.getByText("Nothing new is uploaded to the team. Only this profile changes."),
    ).toBeInTheDocument();
    expect(screen.queryByText("Added (0)")).toBeNull();
  });

  it("labels the confirm action by what the share will do", () => {
    expect(teamShareAction(preview)).toBe("Share selected");
    expect(teamShareAction(alreadyShared)).toBe("Use Team copies");
    const nothing = {
      ...alreadyShared,
      selections: [selection("Linear", "Already shared", "kept", "keeps")],
    };
    expect(teamShareAction(nothing)).toBeNull();
    render(<TeamSharePreview preview={nothing} />);
    expect(
      screen.getByText("Nothing to upload or switch for this selection."),
    ).toBeInTheDocument();
  });

  it("opens a single definition and keeps multiple definitions compact", () => {
    const { container, rerender } = render(
      <TeamSharePreview
        preview={{ ...preview, definitions: preview.definitions.slice(0, 1) }}
      />,
    );
    expect(container.querySelector("details")).toHaveAttribute("open");
    rerender(<TeamSharePreview preview={preview} />);
    expect(container.querySelector("details")).not.toHaveAttribute("open");
  });
});
