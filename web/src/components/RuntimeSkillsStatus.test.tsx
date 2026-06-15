import { fireEvent, render } from "@testing-library/react";
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
  it("opens skill detail rows from the status summary", () => {
    const view = render(
      <RuntimeSkillsStatus
        serviceVersion="0.9.4-dev"
        skillFilter="all"
        skills={skills}
        onSkillFilterChange={vi.fn()}
      />,
    );

    fireEvent.click(view.getByRole("button", { name: /0 missing, 1 outdated/ }));

    expect(view.container.querySelector('[aria-label="Skill details"]')).toBeInTheDocument();
    expect(view.getByText("Umbrella entrypoint for Memory Layer workflows.")).toBeInTheDocument();
    expect(view.getByText("/repo/.agents/skills/memory-remember/SKILL.md")).toBeInTheDocument();
  });

  it("opens details when the filter changes", () => {
    const onSkillFilterChange = vi.fn();
    const view = render(
      <RuntimeSkillsStatus
        serviceVersion="0.9.4-dev"
        skillFilter="memory-layer"
        skills={skills}
        onSkillFilterChange={onSkillFilterChange}
      />,
    );

    const select = view.container.querySelector("select");
    expect(select).not.toBeNull();
    fireEvent.change(select as HTMLSelectElement, { target: { value: "all" } });

    expect(onSkillFilterChange).toHaveBeenCalledWith("all");
    expect(view.container.querySelector('[aria-label="Skill details"]')).toBeInTheDocument();
  });
});
