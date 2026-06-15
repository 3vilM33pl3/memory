import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { SkillsTab } from "./SkillsTab";
import type { SkillContentResponse, SkillInventoryReport } from "../../types";

const inventory: SkillInventoryReport = {
  project_root: "/repo",
  project_skill_root: "/repo/.agents/skills",
  template_root: "/template",
  bundle_version: "0.9.4",
  status: "warn",
  summary: "1 project skill(s) need upgrade",
  filter: "all",
  skills: [
    {
      name: "memory-layer",
      project_path: "/repo/.agents/skills/memory-layer",
      template_path: "/template/memory-layer",
      project_version: "0.9.4",
      template_version: "0.9.4",
      status: "up_to_date",
      action: "skip",
    },
    {
      name: "memory-remember",
      project_path: "/repo/.agents/skills/memory-remember",
      template_path: "/template/memory-remember",
      project_version: "0.9.3",
      template_version: "0.9.4",
      status: "outdated",
      action: "replace",
    },
  ],
};

const detail: SkillContentResponse = {
  skill: inventory.skills[1],
  content: "---\nname: memory-remember\n---\n\nRemember completed work.",
  content_truncated: false,
};

describe("SkillsTab", () => {
  it("renders skill inventory, paths, content, and repair action", () => {
    const onRepair = vi.fn();
    render(
      <SkillsTab
        inventory={inventory}
        detail={detail}
        selectedSkill={inventory.skills[1]}
        selectedSkillIndex={1}
        filter="all"
        loading={false}
        busy={false}
        operation={null}
        error={null}
        onFilterChange={vi.fn()}
        onRefresh={vi.fn()}
        onRepair={onRepair}
        onSelectSkill={vi.fn()}
      />,
    );

    expect(screen.getByRole("heading", { name: "Skills" })).toBeInTheDocument();
    expect(screen.getAllByText("/repo/.agents/skills/memory-remember").length).toBeGreaterThan(0);
    expect(screen.getByText(/Remember completed work/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Repair skills" }));
    expect(onRepair).toHaveBeenCalled();
  });

  it("changes the skill filter", () => {
    const onFilterChange = vi.fn();
    const view = render(
      <SkillsTab
        inventory={inventory}
        detail={detail}
        selectedSkill={inventory.skills[1]}
        selectedSkillIndex={1}
        filter="all"
        loading={false}
        busy={false}
        operation={null}
        error={null}
        onFilterChange={onFilterChange}
        onRefresh={vi.fn()}
        onRepair={vi.fn()}
        onSelectSkill={vi.fn()}
      />,
    );

    const select = view.container.querySelector("select");
    expect(select).not.toBeNull();
    fireEvent.change(select as HTMLSelectElement, { target: { value: "memory-layer" } });

    expect(onFilterChange).toHaveBeenCalledWith("memory-layer");
  });
});
