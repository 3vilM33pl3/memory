import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LoopGlobalStateResponse } from "../../types";
import { AutomationsTab } from "./AutomationsTab";
import type { AutomationCardState } from "./useAutomationsController";

const automation: AutomationCardState = {
  definition: {
    id: "11111111-1111-1111-1111-111111111111",
    loop_id: "context_pack_refresh",
    version: 1,
    name: "Context Pack Refresh",
    description: "Refreshes project/repo context packs when docs change.",
    risk_level: "low",
    default_mode: "suggest_only",
    trigger_spec: { supported: ["manual", "repo_docs_changed"] },
    context_spec: { capabilities: ["read_memory", "read_repo"] },
    policy_spec: {},
    output_spec: { produces: ["context_pack_diff", "memory_proposals"] },
    created_at: "2026-06-15T09:00:00Z",
  },
  effectiveSettings: {
    loop_id: "context_pack_refresh",
    enabled: true,
    mode: "suggest_only",
    scope_type: "repo",
    scope_id: "/home/olivier/Projects/memory",
    global_kill_switch: false,
    blocked_reasons: [],
    budgets: { daily_runs: 3 },
  },
  lastRun: {
    id: "22222222-2222-2222-2222-222222222222",
    loop_id: "context_pack_refresh",
    definition_version: 1,
    project: "memory",
    repo_root: "/home/olivier/Projects/memory",
    mode: "suggest_only",
    status: "succeeded",
    started_at: "2026-06-15T10:00:00Z",
    finished_at: "2026-06-15T10:00:01Z",
    output_summary: "Control-plane loop run recorded.",
    trace_count: 2,
    blocked_reasons: [],
  },
};

const globalState: LoopGlobalStateResponse = {
  kill_switch_enabled: false,
  updated_at: "2026-06-15T10:00:00Z",
};

function renderTab(overrides: Partial<ComponentProps<typeof AutomationsTab>> = {}) {
  const props: ComponentProps<typeof AutomationsTab> = {
    automations: [automation],
    activeAutomation: automation,
    selectedAutomationIndex: 0,
    automationsLoading: false,
    automationBusy: false,
    automationOperation: null,
    loopGlobalState: globalState,
    onRefresh: vi.fn(),
    onSelectAutomation: vi.fn(),
    onSetLoopMode: vi.fn(),
    onDisableLoop: vi.fn(),
    onPauseLoop: vi.fn(),
    onSnoozeLoop: vi.fn(),
    onRunLoop: vi.fn(),
    onToggleGlobalKillSwitch: vi.fn(),
    ...overrides,
  };
  render(<AutomationsTab {...props} />);
  return props;
}

describe("AutomationsTab", () => {
  afterEach(() => {
    cleanup();
  });

  it("renders automation cards with scope, budget, outputs, and last run", () => {
    renderTab();

    expect(screen.getAllByText("Context Pack Refresh")[0]).toBeInTheDocument();
    expect(screen.getAllByText("repo override")[0]).toBeInTheDocument();
    expect(screen.getByText("budget 3 runs/day")).toBeInTheDocument();
    expect(screen.getByText("context pack diff, memory proposals")).toBeInTheDocument();
    expect(screen.getAllByText("succeeded", { exact: false })[0]).toBeInTheDocument();
  });

  it("calls loop control actions", () => {
    const props = renderTab();

    fireEvent.click(screen.getByRole("button", { name: "Run now" }));
    expect(props.onRunLoop).toHaveBeenCalledWith("context_pack_refresh");

    fireEvent.change(screen.getByLabelText("Mode for Context Pack Refresh"), {
      target: { value: "observe" },
    });
    expect(props.onSetLoopMode).toHaveBeenCalledWith("context_pack_refresh", "observe");
  });

  it("shows the global kill switch state", () => {
    const props = renderTab({
      loopGlobalState: { ...globalState, kill_switch_enabled: true },
    });

    expect(screen.getByText("global stopped")).toBeInTheDocument();
    expect(screen.getAllByText("globally stopped")[0]).toBeInTheDocument();

    fireEvent.click(screen.getByText("Disable global stop"));
    expect(props.onToggleGlobalKillSwitch).toHaveBeenCalled();
  });
});
