import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { RuntimeSkillsStatus } from "./RuntimeSkillsStatus";
import type { RuntimeSkillStatus } from "../types";

const skills: RuntimeSkillStatus = {
  bundle_version: "0.9.4-dev",
  status: "warn",
  summary: "0 missing, 1 outdated",
  filter: "all",
  details: [
    {
      id: "memory-layer",
      name: "memory-layer",
      description: "Umbrella entrypoint for Memory Layer workflows.",
      version: "0.9.4-dev",
      status: "ok",
      path: "/repo/.agents/skills/memory-layer/SKILL.md",
    },
    {
      id: "memory-remember",
      name: "memory-remember",
      description: "Remember meaningful completed work.",
      version: "0.9.3",
      status: "outdated",
      path: "/repo/.agents/skills/memory-remember/SKILL.md",
    },
  ],
};

describe("RuntimeSkillsStatus", () => {
  it("renders a compact summary and opens the skills tab", () => {
    const onOpenSkills = vi.fn();
    const view = render(
      <RuntimeSkillsStatus
        serviceVersion="0.9.4-dev"
        skills={skills}
        onOpenSkills={onOpenSkills}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /Skills/ }));

    expect(onOpenSkills).toHaveBeenCalled();
    expect(view.getByText(/0 missing, 1 outdated/)).toBeInTheDocument();
  });

  it("falls back to the service version", () => {
    render(
      <RuntimeSkillsStatus
        serviceVersion="0.9.4-dev"
        skills={null}
        onOpenSkills={vi.fn()}
      />,
    );

    expect(screen.getByText(/v0.9.4-dev unknown/)).toBeInTheDocument();
  });
});
