import type { RuntimeSkillStatus } from "../types";

interface RuntimeSkillsStatusProps {
  serviceVersion: string;
  skills?: RuntimeSkillStatus | null;
  onOpenSkills: () => void;
}

export function RuntimeSkillsStatus({
  serviceVersion,
  skills,
  onOpenSkills,
}: RuntimeSkillsStatusProps) {
  return (
    <button type="button" className="runtime-skills-status" onClick={onOpenSkills}>
      <span>Skills</span>
      <span className="skill-status-summary">
        v{skills?.bundle_version ?? serviceVersion} {skills?.status ?? "unknown"}
        {skills?.summary ? ` ${skills.summary}` : ""}
      </span>
    </button>
  );
}
