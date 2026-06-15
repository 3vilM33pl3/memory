import { Metric } from "../../components/Details";
import type {
  SkillContentResponse,
  SkillInventoryFilter,
  SkillInventoryReport,
  SkillVersionInfo,
} from "../../types";

interface SkillsTabProps {
  inventory: SkillInventoryReport | null;
  detail: SkillContentResponse | null;
  selectedSkill: SkillVersionInfo | null;
  selectedSkillIndex: number;
  filter: SkillInventoryFilter;
  loading: boolean;
  busy: boolean;
  operation: string | null;
  error: string | null;
  onFilterChange: (filter: SkillInventoryFilter) => void;
  onRefresh: () => void;
  onRepair: () => void;
  onSelectSkill: (index: number) => void;
}

export function SkillsTab({
  inventory,
  detail,
  selectedSkill,
  selectedSkillIndex,
  filter,
  loading,
  busy,
  operation,
  error,
  onFilterChange,
  onRefresh,
  onRepair,
  onSelectSkill,
}: SkillsTabProps) {
  const skills = inventory?.skills ?? [];
  const visibleDetail = detail?.skill.name === selectedSkill?.name ? detail : null;

  return (
    <section className="panel-stack">
      <div className="panel actions-row">
        <label className="status-filter">
          Filter
          <select
            value={filter}
            onChange={(event) => onFilterChange(event.target.value as SkillInventoryFilter)}
            disabled={busy}
          >
            <option value="memory-layer">Memory Layer</option>
            <option value="all">All skills</option>
          </select>
        </label>
        <button onClick={onRefresh} type="button" disabled={busy}>
          {loading ? "Refreshing..." : "Refresh"}
        </button>
        <button onClick={onRepair} type="button" disabled={busy}>
          Repair skills
        </button>
        <span className="muted">
          {operation ? `${operation}...` : inventory ? inventory.summary : "idle"}
        </span>
      </div>

      {error ? <div className="panel error-banner">Skills error: {error}</div> : null}

      <section className="panel-grid">
        <div className="panel">
          <h2>Skills</h2>
          {inventory ? (
            <div className="stats-row">
              <span>bundle v{inventory.bundle_version}</span>
              <span className={`badge ${inventory.status === "ok" ? "badge-active" : "badge-archived"}`}>
                {inventory.status}
              </span>
              <span>{skills.length} shown</span>
            </div>
          ) : (
            <p className="muted">No skill inventory loaded.</p>
          )}
          <div className="list-view">
            {skills.map((skill, index) => (
              <button
                key={skill.name}
                type="button"
                className={`list-item ${selectedSkillIndex === index ? "selected" : ""}`}
                onClick={() => onSelectSkill(index)}
              >
                <div>
                  <strong>{skill.name}</strong>
                  <p>{skill.detail ?? skill.project_path}</p>
                </div>
                <div className="meta-stack">
                  <span className={`badge ${skill.status === "up_to_date" ? "badge-active" : "badge-archived"}`}>
                    {skill.status}
                  </span>
                  <span>local {skill.project_version ?? "n/a"}</span>
                  <span>template {skill.template_version ?? "n/a"}</span>
                  <span>{skill.action}</span>
                </div>
              </button>
            ))}
          </div>
        </div>

        <div className="panel detail-scroll">
          {selectedSkill ? (
            <>
              <h2>{selectedSkill.name}</h2>
              <Metric label="Status" value={selectedSkill.status} />
              <Metric label="Action" value={selectedSkill.action} />
              <Metric label="Project version" value={selectedSkill.project_version ?? "missing"} />
              <Metric label="Template version" value={selectedSkill.template_version ?? "missing"} />
              <Metric label="Project path" value={selectedSkill.project_path} />
              <Metric label="Template path" value={selectedSkill.template_path ?? "not found"} />
              {selectedSkill.detail ? <Metric label="Detail" value={selectedSkill.detail} /> : null}
              <h3>SKILL.md</h3>
              {visibleDetail?.content ? (
                <>
                  <pre>{visibleDetail.content}</pre>
                  {visibleDetail.content_truncated ? <p className="muted">Content truncated.</p> : null}
                </>
              ) : (
                <p className="muted">No SKILL.md content is available for this skill.</p>
              )}
            </>
          ) : (
            <p className="muted">Select a skill to inspect its path and instructions.</p>
          )}
        </div>
      </section>
    </section>
  );
}
