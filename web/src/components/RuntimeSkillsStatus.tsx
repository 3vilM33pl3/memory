import { useState } from "react";

import type { RuntimeSkillStatus } from "../types";

interface RuntimeSkillsStatusProps {
  serviceVersion: string;
  skillFilter: string;
  skills?: RuntimeSkillStatus | null;
  onSkillFilterChange: (value: string) => void;
}

export function RuntimeSkillsStatus({
  serviceVersion,
  skillFilter,
  skills,
  onSkillFilterChange,
}: RuntimeSkillsStatusProps) {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const details = skills?.details ?? [];

  return (
    <>
      <span className="runtime-skills-status">
        <label className="status-filter">
          Skills
          <select
            value={skillFilter}
            onChange={(event) => {
              onSkillFilterChange(event.target.value);
              setDetailsOpen(true);
            }}
          >
            <option value="memory-layer">Memory Layer</option>
            <option value="all">All skills</option>
          </select>
        </label>
        <button
          type="button"
          className="skill-status-summary"
          aria-expanded={detailsOpen}
          onClick={() => setDetailsOpen((current) => !current)}
        >
          v{skills?.bundle_version ?? serviceVersion} {skills?.status ?? "unknown"}
          {skills?.summary ? ` ${skills.summary}` : ""}
        </button>
      </span>

      {detailsOpen ? (
        <section className="skill-details-panel" aria-label="Skill details">
          <div className="skill-details-header">
            <strong>{skillFilter === "all" ? "All skills" : "Memory Layer skill"}</strong>
            <button type="button" onClick={() => setDetailsOpen(false)}>
              Close
            </button>
          </div>
          {details.length > 0 ? (
            <div className="skill-details-list">
              {details.map((skill) => (
                <article className="skill-detail-row" key={skill.id}>
                  <div className="skill-detail-main">
                    <strong>{skill.name}</strong>
                    <span className={`skill-detail-status skill-detail-status-${skill.status}`}>
                      {skill.status}
                    </span>
                  </div>
                  {skill.description ? <p>{skill.description}</p> : null}
                  <div className="skill-detail-meta">
                    <span>version {skill.version ?? "missing"}</span>
                    <code>{skill.path}</code>
                  </div>
                </article>
              ))}
            </div>
          ) : (
            <p className="skill-details-empty">
              No skill details are available until the repo root is resolved.
            </p>
          )}
        </section>
      ) : null}
    </>
  );
}
