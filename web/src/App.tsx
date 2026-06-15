import { HelpPanel } from "./components/HelpPanel";
import { RuntimeSkillsStatus } from "./components/RuntimeSkillsStatus";
import { ActivityTab } from "./features/activity/ActivityTab";
import { AgentsTab } from "./features/agents/AgentsTab";
import { AutomationsTab } from "./features/automations/AutomationsTab";
import { BundlesTab } from "./features/bundles/BundlesTab";
import { EmbeddingsTab } from "./features/embeddings/EmbeddingsTab";
import { ErrorsTab } from "./features/errors/ErrorsTab";
import { MemoriesTab } from "./features/memories/MemoriesTab";
import { ProjectTab } from "./features/project/ProjectTab";
import { QueryTab } from "./features/query/QueryTab";
import { ReviewTab } from "./features/review/ReviewTab";
import { ResumeTab } from "./features/resume/ResumeTab";
import { WatchersTab } from "./features/watchers/WatchersTab";
import { SkillsTab } from "./features/skills/SkillsTab";
import { useAppShell } from "./hooks/useAppShell";
import { MORE_TABS, PRIMARY_TABS, type Tab } from "./tabs";

export default function App() {
  const {
    tab,
    setTab,
    project,
    projectInput,
    setProjectInput,
    repoRootInput,
    setRepoRootInput,
    connectionState,
    overview,
    runtimeStatus,
    skillFilter,
    serviceVersion,
    helpOpen,
    setHelpOpen,
    applyProjectInput,
    searchRef,
    filteredMemories,
    selectedMemoryId,
    selectedMemory,
    selectedHistory,
    textFilter,
    setTextFilter,
    tagFilter,
    setTagFilter,
    statusFilter,
    setStatusFilter,
    typeFilter,
    setTypeFilter,
    setSelectedMemoryId,
    setSelectedHistory,
    handleLoadHistory,
    handleDelete,
    agentSnapshot,
    sortedAgentSessions,
    selectedAgent,
    selectedAgentIndex,
    setSelectedAgentIndex,
    queryRef,
    queryText,
    setQueryText,
    queryResponse,
    activeQueryResult,
    selectedQueryMemory,
    selectedQueryIndex,
    selectedQueryMemoryLoading,
    selectedQueryMemoryError,
    queryLoading,
    queryError,
    queryRoundtripMs,
    includeStale,
    setIncludeStale,
    handleQuerySubmit,
    applyQueryHistory,
    setQueryHistoryCursor,
    setSelectedQueryIndex,
    activities,
    activeActivity,
    selectedActivityIndex,
    setSelectedActivityIndex,
    upToSpeed,
    upToSpeedLoading,
    upToSpeedError,
    llmAudit,
    llmAuditLoading,
    llmAuditError,
    llmAuditToggling,
    handleUpToSpeed,
    handleToggleLlmAudit,
    errorItems,
    activeError,
    selectedErrorIndex,
    setSelectedErrorIndex,
    proposals,
    replacementPolicy,
    refreshProject,
    runProjectAction,
    handleApproveProposal,
    handleRejectProposal,
    effectiveRepoRoot,
    activeProposal,
    selectedProposalIndex,
    setSelectedProposalIndex,
    refreshReview,
    handleCyclePolicy,
    embeddingBackends,
    selectedEmbeddingBackend,
    selectedEmbeddingIndex,
    setSelectedEmbeddingIndex,
    embeddingBusy,
    embeddingLoading,
    embeddingOperation,
    refreshEmbeddings,
    handleToggleEmbeddingSearch,
    handleToggleEmbeddingCreation,
    handleReembedEmbeddingBackend,
    handleReindexEmbeddingBackend,
    skillInventory,
    skillDetail,
    selectedSkill,
    selectedSkillIndex,
    setSelectedSkillIndex,
    setSkillFilter,
    skillsLoading,
    skillsOperation,
    skillsBusy,
    skillsError,
    refreshSkills,
    handleRepairSkills,
    automations,
    activeAutomation,
    selectedAutomationIndex,
    setSelectedAutomationIndex,
    automationsLoading,
    automationBusy,
    automationOperation,
    loopGlobalState,
    selectedLoopRun,
    selectedLoopRunApprovals,
    selectedLoopRunLoading,
    approvalQueue,
    approvalEdits,
    proposalEdits,
    refreshAutomations,
    handleSetLoopMode,
    handleDisableLoop,
    handlePauseLoop,
    handleSnoozeLoop,
    handleRunLoop,
    handleLoadLoopRun,
    setApprovalEdit,
    handleApprovalDecision,
    setProposalEdit,
    handleMemoryProposalDecision,
    handleToggleGlobalKillSwitch,
    resumeData,
    resumeLoading,
    handleLoadResume,
    bundleOptions,
    setBundleOptions,
    exportPreview,
    importPreview,
    setImportFile,
    handlePreviewExport,
    handleDownloadExport,
    handlePreviewImport,
    handleApplyImport,
    statusMessage,
  } = useAppShell();

  return (
    <div className="app-shell">
      <header className="topbar">
        <div>
          <h1>Memory Layer Web</h1>
        </div>
        <form
          className="project-form"
          onSubmit={(event) => {
            event.preventDefault();
            applyProjectInput();
          }}
        >
          <label>
            Project
            <input value={projectInput} onChange={(event) => setProjectInput(event.target.value)} />
          </label>
          <label>
            Repo root
            <input
              placeholder="Auto"
              value={repoRootInput}
              onChange={(event) => setRepoRootInput(event.target.value)}
            />
          </label>
          <button type="submit">Load</button>
        </form>
      </header>

      <section className="status-strip">
        <span className={`status-pill status-${connectionState}`}>{connectionState}</span>
        <span><strong>{overview.project}</strong></span>
        <span>Web v{runtimeStatus?.web.version ?? serviceVersion} {runtimeStatus?.web.status ?? "ok"}</span>
        <span>Service v{runtimeStatus?.service.version ?? serviceVersion} {runtimeStatus?.service.status ?? overview.service_status}</span>
        <span>Manager v{runtimeStatus?.manager.version ?? serviceVersion} {runtimeStatus?.manager.state ?? "unknown"}{runtimeStatus?.manager.detail ? ` ${runtimeStatus.manager.detail}` : ""}</span>
        <span>Watchers v{runtimeStatus?.watchers.version ?? serviceVersion} {runtimeStatus?.watchers.status ?? "unknown"} {runtimeStatus?.watchers.detail ?? `${overview.watchers?.active_count ?? 0} active`}</span>
        <span>Provenance {runtimeStatus?.provenance.status ?? "unknown"} {runtimeStatus?.provenance.last_finished_at ? `last ${new Date(runtimeStatus.provenance.last_finished_at).toLocaleString()}` : "not run"}</span>
        <RuntimeSkillsStatus
          serviceVersion={serviceVersion}
          skills={runtimeStatus?.skills ?? null}
          onOpenSkills={() => setTab("skills")}
        />
        <span>db {overview.database_status}</span>
        <span>{overview.memory_entries_total} memories</span>
        <span>{overview.raw_captures_total} captures</span>
        {runtimeStatus?.restart_notice ? <span className="restart-text">restart {runtimeStatus.restart_notice.version}</span> : null}
      </section>

      <nav className="tabs">
        {PRIMARY_TABS.map((name, i) => (
          <button
            key={name}
            className={tab === name ? "tab-active" : ""}
            onClick={() => setTab(name)}
            type="button"
            title={`${i + 1}`}
          >
            {name}
          </button>
        ))}
        <select className="more-select" value={MORE_TABS.includes(tab as (typeof MORE_TABS)[number]) ? tab : ""} onChange={(event) => event.target.value && setTab(event.target.value as Tab)}>
          <option value="">More</option>
          {MORE_TABS.map((name) => (
            <option key={name} value={name}>{name}</option>
          ))}
        </select>
        <button className={helpOpen ? "tab-active" : ""} onClick={() => setHelpOpen((current) => !current)} type="button">
          Help
        </button>
      </nav>

      {helpOpen ? <HelpPanel tab={tab} /> : null}

      {tab === "memories" ? (
        <MemoriesTab
          searchRef={searchRef}
          filteredMemories={filteredMemories}
          selectedMemoryId={selectedMemoryId}
          selectedMemory={selectedMemory}
          selectedHistory={selectedHistory}
          textFilter={textFilter}
          tagFilter={tagFilter}
          statusFilter={statusFilter}
          typeFilter={typeFilter}
          onTextFilterChange={setTextFilter}
          onTagFilterChange={setTagFilter}
          onStatusFilterChange={setStatusFilter}
          onTypeFilterChange={setTypeFilter}
          onSelectMemory={setSelectedMemoryId}
          onClearHistory={() => setSelectedHistory(null)}
          onLoadHistory={(memoryId) => void handleLoadHistory(memoryId)}
          onDelete={(memoryId) => void handleDelete(memoryId)}
        />
      ) : null}
      {tab === "agents" ? (
        <AgentsTab
          agentSnapshot={agentSnapshot}
          sessions={sortedAgentSessions}
          selectedAgent={selectedAgent}
          selectedAgentIndex={selectedAgentIndex}
          onSelectAgent={setSelectedAgentIndex}
        />
      ) : null}
      {tab === "query" ? (
        <QueryTab
          queryRef={queryRef}
          queryText={queryText}
          queryResponse={queryResponse}
          activeQueryResult={activeQueryResult}
          selectedQueryMemory={selectedQueryMemory}
          selectedQueryIndex={selectedQueryIndex}
          selectedQueryMemoryLoading={selectedQueryMemoryLoading}
          selectedQueryMemoryError={selectedQueryMemoryError}
          queryLoading={queryLoading}
          queryError={queryError}
          queryRoundtripMs={queryRoundtripMs}
          includeStale={includeStale}
          onQueryTextChange={setQueryText}
          onIncludeStaleChange={setIncludeStale}
          onSubmit={handleQuerySubmit}
          onApplyHistory={applyQueryHistory}
          onResetHistoryCursor={() => setQueryHistoryCursor(null)}
          onSelectResult={setSelectedQueryIndex}
          onDelete={(memoryId) => void handleDelete(memoryId)}
        />
      ) : null}
      {tab === "activity" ? (
        <ActivityTab
          activities={activities}
          activeActivity={activeActivity}
          selectedActivityIndex={selectedActivityIndex}
          upToSpeed={upToSpeed}
          upToSpeedLoading={upToSpeedLoading}
          upToSpeedError={upToSpeedError}
          llmAudit={llmAudit}
          llmAuditLoading={llmAuditLoading}
          llmAuditError={llmAuditError}
          llmAuditToggling={llmAuditToggling}
          onLoadUpToSpeed={(includeLlmSummary) => void handleUpToSpeed(includeLlmSummary)}
          onToggleLlmAudit={() => void handleToggleLlmAudit()}
          onSelectActivity={setSelectedActivityIndex}
        />
      ) : null}
      {tab === "errors" ? (
        <ErrorsTab
          errorItems={errorItems}
          activeError={activeError}
          selectedErrorIndex={selectedErrorIndex}
          onSelectError={setSelectedErrorIndex}
        />
      ) : null}
      {tab === "project" ? (
        <ProjectTab
          project={project}
          overview={overview}
          activities={activities}
          proposals={proposals}
          replacementPolicy={replacementPolicy}
          onRefresh={() => void refreshProject(project)}
          onProjectAction={(action) => void runProjectAction(action)}
          onOpenActivity={(index) => {
            setSelectedActivityIndex(index);
            setTab("activity");
          }}
          onApproveProposal={(proposalId) => void handleApproveProposal(proposalId)}
          onRejectProposal={(proposalId) => void handleRejectProposal(proposalId)}
        />
      ) : null}

      {tab === "review" ? (
        <ReviewTab
          effectiveRepoRoot={effectiveRepoRoot}
          proposals={proposals}
          activeProposal={activeProposal}
          selectedProposalIndex={selectedProposalIndex}
          replacementPolicy={replacementPolicy}
          onRefresh={() => void refreshReview()}
          onCyclePolicy={() => void handleCyclePolicy()}
          onSelectProposal={setSelectedProposalIndex}
          onApproveProposal={(proposalId) => void handleApproveProposal(proposalId)}
          onRejectProposal={(proposalId) => void handleRejectProposal(proposalId)}
        />
      ) : null}

      {tab === "watchers" ? <WatchersTab overview={overview} project={project} /> : null}
      {tab === "skills" ? (
        <SkillsTab
          inventory={skillInventory}
          detail={skillDetail}
          selectedSkill={selectedSkill}
          selectedSkillIndex={selectedSkillIndex}
          filter={skillFilter}
          loading={skillsLoading}
          busy={skillsBusy}
          operation={skillsOperation}
          error={skillsError}
          onFilterChange={setSkillFilter}
          onRefresh={() => void refreshSkills()}
          onRepair={() => void handleRepairSkills()}
          onSelectSkill={setSelectedSkillIndex}
        />
      ) : null}
      {tab === "embeddings" ? (
        <EmbeddingsTab
          embeddingBackends={embeddingBackends}
          selectedEmbeddingBackend={selectedEmbeddingBackend}
          selectedEmbeddingIndex={selectedEmbeddingIndex}
          embeddingBusy={embeddingBusy}
          embeddingLoading={embeddingLoading}
          embeddingOperation={embeddingOperation}
          onRefresh={() => void refreshEmbeddings()}
          onReindexAll={() => void runProjectAction("reindex")}
          onReembedAll={() => void runProjectAction("reembed")}
          onSelectBackend={setSelectedEmbeddingIndex}
          onToggleSearch={(backend) => void handleToggleEmbeddingSearch(backend)}
          onToggleCreation={(backend) => void handleToggleEmbeddingCreation(backend)}
          onReembedBackend={(backend) => void handleReembedEmbeddingBackend(backend)}
          onReindexBackend={(backend) => void handleReindexEmbeddingBackend(backend)}
        />
      ) : null}
      {tab === "resume" ? (
        <ResumeTab
          resumeData={resumeData}
          resumeLoading={resumeLoading}
          onLoadResume={() => void handleLoadResume()}
        />
      ) : null}
      {tab === "automations" ? (
        <AutomationsTab
          automations={automations}
          activeAutomation={activeAutomation}
          selectedAutomationIndex={selectedAutomationIndex}
          automationsLoading={automationsLoading}
          automationBusy={automationBusy}
          automationOperation={automationOperation}
          loopGlobalState={loopGlobalState}
          selectedLoopRun={selectedLoopRun}
          selectedLoopRunApprovals={selectedLoopRunApprovals}
          selectedLoopRunLoading={selectedLoopRunLoading}
          approvalQueue={approvalQueue}
          approvalEdits={approvalEdits}
          proposalEdits={proposalEdits}
          onRefresh={() => void refreshAutomations()}
          onSelectAutomation={setSelectedAutomationIndex}
          onSetLoopMode={(loopId, mode) => void handleSetLoopMode(loopId, mode)}
          onDisableLoop={(loopId) => void handleDisableLoop(loopId)}
          onPauseLoop={(loopId) => void handlePauseLoop(loopId)}
          onSnoozeLoop={(loopId) => void handleSnoozeLoop(loopId)}
          onRunLoop={(loopId) => void handleRunLoop(loopId)}
          onLoadLoopRun={(runId) => void handleLoadLoopRun(runId)}
          onApprovalEditChange={setApprovalEdit}
          onApprovalDecision={(approval, action) => void handleApprovalDecision(approval, action)}
          onProposalEditChange={setProposalEdit}
          onProposalDecision={(proposal, action) => void handleMemoryProposalDecision(proposal, action)}
          onToggleGlobalKillSwitch={() => void handleToggleGlobalKillSwitch()}
        />
      ) : null}
      {tab === "bundles" ? (
        <BundlesTab
          bundleOptions={bundleOptions}
          exportPreview={exportPreview}
          importPreview={importPreview}
          onBundleOptionsChange={setBundleOptions}
          onImportFileChange={setImportFile}
          onPreviewExport={() => void handlePreviewExport()}
          onDownloadExport={() => void handleDownloadExport()}
          onPreviewImport={() => void handlePreviewImport()}
          onApplyImport={() => void handleApplyImport()}
        />
      ) : null}
      <footer className="statusbar">{statusMessage}</footer>
    </div>
  );
}
