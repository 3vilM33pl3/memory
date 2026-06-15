import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LoopGlobalStateResponse, LoopRunDetail } from "../../types";
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

const runDetail: LoopRunDetail = {
  summary: automation.lastRun!,
  run_reason: "Manual browser UI run now request.",
  trigger_event: {
    id: "33333333-3333-3333-3333-333333333333",
    source: "manual",
    event_type: "manual_run",
    project: "memory",
    repo_root: "/home/olivier/Projects/memory",
    payload_hash: "abc123",
    trust_level: "high",
    payload: { source: "web" },
    received_at: "2026-06-15T10:00:00Z",
  },
  effective_settings: { mode: "suggest_only", enabled: true },
  policy_decisions: [
    { action: "read_memory", allowed: true, requires_approval: false, reason: "read allowed" },
    { action: "write_repo", allowed: false, requires_approval: true, reason: "approval required" },
  ],
  cost: { total_tokens: 42 },
  output: { summary: "Control-plane loop run recorded." },
  traces: [
    {
      id: "44444444-4444-4444-4444-444444444444",
      run_id: automation.lastRun!.id,
      sequence: 1,
      trace_type: "policy",
      title: "Policy evaluation",
      payload: { decisions: 2 },
      redacted: false,
      created_at: "2026-06-15T10:00:00Z",
    },
    {
      id: "55555555-5555-5555-5555-555555555555",
      run_id: automation.lastRun!.id,
      sequence: 2,
      trace_type: "command",
      title: "Sensitive command output",
      payload: { secret: "hidden" },
      redacted: true,
      created_at: "2026-06-15T10:00:01Z",
    },
  ],
  memory_proposals: [
    {
      id: "66666666-6666-6666-6666-666666666666",
      run_id: automation.lastRun!.id,
      project: "memory",
      loop_id: "context_pack_refresh",
      proposal_type: "add",
      candidate: { summary: "New context fact" },
      evidence: [],
      confidence: 0.9,
      status: "pending",
      created_at: "2026-06-15T10:00:02Z",
    },
  ],
  context_pack: {
    id: "88888888-8888-8888-8888-888888888888",
    loop_id: "context_pack_refresh",
    project: "memory",
    repo_root: "/home/olivier/Projects/memory",
    run_id: automation.lastRun!.id,
    generated_at: "2026-06-15T10:00:02Z",
    token_budget: 4000,
    estimated_tokens: 128,
    instructions: [
      { path: "AGENTS.md", reason: "repo instructions", estimated_tokens: 20 },
    ],
    memories: [
      {
        memory_id: "99999999-9999-9999-9999-999999999999",
        canonical_id: "99999999-9999-9999-9999-999999999999",
        summary: "Architecture context",
        preview: "Use the loop control plane.",
        memory_type: "architecture",
        confidence: 0.92,
        importance: 4,
        freshness: "fresh",
        updated_at: "2026-06-15T09:00:00Z",
        tags: ["loop"],
        source_refs: [{ source_kind: "file", file_path: "AGENTS.md" }],
        estimated_tokens: 42,
        stale: false,
        contradictory: false,
        inclusion_reason: "ranked by importance",
      },
    ],
    exclusions: [],
    warnings: [],
    metadata: { builder: "deterministic" },
  },
  context_diff: {
    previous_run_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
    previous_pack_id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
    added_memory_ids: ["99999999-9999-9999-9999-999999999999"],
    removed_memory_ids: [],
    changed_memory_ids: [],
    token_delta: 128,
    warning_delta: [],
  },
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
    selectedLoopRun: null,
    selectedLoopRunApprovals: [],
    selectedLoopRunLoading: false,
    approvalQueue: [],
    approvalEdits: {},
    proposalEdits: {},
    onRefresh: vi.fn(),
    onSelectAutomation: vi.fn(),
    onSetLoopMode: vi.fn(),
    onDisableLoop: vi.fn(),
    onPauseLoop: vi.fn(),
    onSnoozeLoop: vi.fn(),
    onRunLoop: vi.fn(),
    onLoadLoopRun: vi.fn(),
    onApprovalEditChange: vi.fn(),
    onApprovalDecision: vi.fn(),
    onProposalEditChange: vi.fn(),
    onProposalDecision: vi.fn(),
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

    fireEvent.click(screen.getByRole("button", { name: "Load run" }));
    expect(props.onLoadLoopRun).toHaveBeenCalledWith(automation.lastRun!.id);
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

  it("renders the selected run detail ledger", () => {
    renderTab({
      selectedLoopRun: runDetail,
      selectedLoopRunApprovals: [
        {
          id: "77777777-7777-7777-7777-777777777777",
          run_id: automation.lastRun!.id,
          project: "memory",
          loop_id: "context_pack_refresh",
          action_type: "write_memory_proposal",
          proposed_action: { proposal_id: "66666666-6666-6666-6666-666666666666" },
          risk_reason: "Durable memory write requires review.",
          status: "pending",
          created_at: "2026-06-15T10:00:03Z",
        },
      ],
    });

    expect(screen.getByText("manual / manual_run")).toBeInTheDocument();
    expect(screen.getByText("Policy gates")).toBeInTheDocument();
    expect(screen.getByText("Context pack")).toBeInTheDocument();
    expect(screen.getByText("Architecture context")).toBeInTheDocument();
    expect(screen.getByText("128/4000 tokens")).toBeInTheDocument();
    expect(screen.getByText("write repo")).toBeInTheDocument();
    expect(screen.getByText("add · pending")).toBeInTheDocument();
    expect(screen.getByText("write memory proposal · pending")).toBeInTheDocument();
    expect(screen.getByText("Payload redacted.")).toBeInTheDocument();
  });

  it("renders pending approvals with edit and decision actions", () => {
    const approval = {
      id: "77777777-7777-7777-7777-777777777777",
      run_id: automation.lastRun!.id,
      project: "memory",
      loop_id: "context_pack_refresh",
      action_type: "write_memory_proposal",
      proposed_action: { proposal_id: "66666666-6666-6666-6666-666666666666" },
      risk_reason: "Durable memory write requires review.",
      requester: "loop-agent",
      status: "pending" as const,
      created_at: "2026-06-15T10:00:03Z",
    };
    const props = renderTab({
      selectedLoopRun: runDetail,
      selectedLoopRunApprovals: [approval],
      approvalQueue: [approval],
      approvalEdits: {
        [approval.id]: '{\n  "proposal_id": "66666666-6666-6666-6666-666666666666"\n}',
      },
    });

    expect(screen.getByText("Approval queue")).toBeInTheDocument();
    expect(screen.getByText("1 pending")).toBeInTheDocument();
    expect(screen.getAllByText("Memory proposal add")[0]).toBeInTheDocument();

    fireEvent.change(screen.getAllByLabelText("Proposed action")[0], {
      target: { value: '{"proposal_id":"updated"}' },
    });
    expect(props.onApprovalEditChange).toHaveBeenCalledWith(approval.id, '{"proposal_id":"updated"}');

    fireEvent.click(screen.getAllByRole("button", { name: "Approve" })[0]);
    expect(props.onApprovalDecision).toHaveBeenCalledWith(approval, "approve");

    fireEvent.click(screen.getAllByRole("button", { name: "Save edit" })[0]);
    expect(props.onApprovalDecision).toHaveBeenCalledWith(approval, "edit");

    fireEvent.click(screen.getAllByRole("button", { name: "Reject" })[0]);
    expect(props.onApprovalDecision).toHaveBeenCalledWith(approval, "reject");
  });
});
