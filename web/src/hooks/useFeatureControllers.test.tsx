import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { overviewFixture } from "../test/fixtures";
import type { ProjectMemoriesResponse } from "../types";
import { useActivityController } from "../features/activity/useActivityController";
import { useAgentsController } from "../features/agents/useAgentsController";
import { useBundlesController } from "../features/bundles/useBundlesController";
import { useEmbeddingsController } from "../features/embeddings/useEmbeddingsController";
import { useErrorsController } from "../features/errors/useErrorsController";
import { useMemoriesController } from "../features/memories/useMemoriesController";
import { useQueryController } from "../features/query/useQueryController";
import { useResumeController } from "../features/resume/useResumeController";
import { useReviewController } from "../features/review/useReviewController";

vi.mock("../api", () => ({
  activateEmbeddingBackend: vi.fn(),
  approveProposal: vi.fn(),
  archiveProject: vi.fn(),
  curate: vi.fn(),
  deactivateEmbeddingBackend: vi.fn(),
  deleteMemory: vi.fn(),
  exportBundle: vi.fn(),
  getActivities: vi.fn(() => Promise.resolve({ project: "memory", total: 0, items: [] })),
  getAgentSnapshot: vi.fn(),
  getEmbeddingBackends: vi.fn(),
  getHealth: vi.fn(),
  getLlmAuditStatus: vi.fn(),
  getMemory: vi.fn(),
  getMemoryHistory: vi.fn(),
  getMemories: vi.fn(),
  getOverview: vi.fn(),
  getReplacementPolicy: vi.fn(),
  getReplacementProposals: vi.fn(),
  getRuntimeStatus: vi.fn(),
  getResume: vi.fn(),
  getUpToSpeed: vi.fn(),
  importBundle: vi.fn(),
  previewExportBundle: vi.fn(),
  previewImportBundle: vi.fn(),
  reembed: vi.fn(),
  reindex: vi.fn(),
  rejectProposal: vi.fn(),
  runQuery: vi.fn(),
  saveReplacementPolicy: vi.fn(),
  setEmbeddingCreationEnabled: vi.fn(),
  setLlmAuditEnabled: vi.fn(),
}));

const emptyMemories: ProjectMemoriesResponse = {
  project: "memory",
  total: 0,
  items: [],
};

const setStatusMessage = vi.fn();
const refreshProject = vi.fn(() => Promise.resolve());
const recordLocalDiagnostic = vi.fn();
const setTab = vi.fn();
const setRepoRootInput = vi.fn();
const sendStream = vi.fn();

function ControllerHarness() {
  const activity = useActivityController({
    project: "memory",
    activeTab: "memories",
    setStatusMessage,
    recordLocalDiagnostic,
  });
  const memories = useMemoriesController({
    memories: emptyMemories,
    setStatusMessage,
    sendStream,
  });
  const query = useQueryController({
    project: "memory",
    setTab,
    setStatusMessage,
    recordLocalDiagnostic,
    refreshProject,
  });
  const agents = useAgentsController({
    activeTab: "memories",
    project: "memory",
    effectiveRepoRoot: "",
  });
  const embeddings = useEmbeddingsController({
    activeTab: "memories",
    project: "memory",
    setStatusMessage,
    refreshProject,
  });
  const bundles = useBundlesController({
    project: "memory",
    setTab,
    setStatusMessage,
    refreshProject,
  });
  const resume = useResumeController({
    project: "memory",
    effectiveRepoRoot: "",
    setTab,
    setStatusMessage,
  });
  const review = useReviewController({
    activeTab: "memories",
    project: "memory",
    effectiveRepoRoot: "",
    repoRootInput: "",
    setRepoRootInput,
    setStatusMessage,
    refreshProject,
  });
  const errors = useErrorsController({
    activities: activity.activities,
    localDiagnostics: [],
    connectionState: "live",
  });

  return (
    <div>
      <span>activity:{activity.activities.length}</span>
      <span>memories:{memories.filteredMemories.length}</span>
      <span>query:{query.queryText}</span>
      <span>agents:{agents.sortedAgentSessions.length}</span>
      <span>embeddings:{embeddings.embeddingBackends?.backends.length ?? 0}</span>
      <span>bundles:{String(bundles.exportPreview)}</span>
      <span>resume:{String(resume.resumeData)}</span>
      <span>review:{review.proposals.length}</span>
      <span>errors:{errors.errorItems.length}</span>
      <span>overview:{overviewFixture.project}</span>
    </div>
  );
}

describe("feature controller hooks", () => {
  it("render with default state without requiring active feature fetches", async () => {
    render(<ControllerHarness />);

    await waitFor(() => expect(screen.getByText("activity:0")).toBeInTheDocument());
    expect(screen.getByText("memories:0")).toBeInTheDocument();
    expect(screen.getByText("agents:0")).toBeInTheDocument();
    expect(screen.getByText("embeddings:0")).toBeInTheDocument();
    expect(screen.getByText("errors:0")).toBeInTheDocument();
  });
});
