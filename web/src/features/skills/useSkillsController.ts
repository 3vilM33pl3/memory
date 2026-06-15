import { useEffect, useState } from "react";

import { getSkill, getSkills, repairSkills } from "../../api";
import type {
  SkillContentResponse,
  SkillInventoryFilter,
  SkillInventoryReport,
  SkillVersionInfo,
} from "../../types";

interface SkillsControllerOptions {
  activeTab: string;
  effectiveRepoRoot: string;
  skillFilter: SkillInventoryFilter;
  setSkillFilter: (filter: SkillInventoryFilter) => void;
  setStatusMessage: (message: string) => void;
  recordLocalDiagnostic: (component: string, operation: string, message: string) => void;
}

export function useSkillsController({
  activeTab,
  effectiveRepoRoot,
  skillFilter,
  setSkillFilter,
  setStatusMessage,
  recordLocalDiagnostic,
}: SkillsControllerOptions) {
  const [skillInventory, setSkillInventory] = useState<SkillInventoryReport | null>(null);
  const [skillDetail, setSkillDetail] = useState<SkillContentResponse | null>(null);
  const [selectedSkillIndex, setSelectedSkillIndex] = useState(0);
  const [skillsLoading, setSkillsLoading] = useState(false);
  const [skillsOperation, setSkillsOperation] = useState<string | null>(null);
  const [skillsError, setSkillsError] = useState<string | null>(null);

  const skills = skillInventory?.skills ?? [];
  const selectedSkill: SkillVersionInfo | null = skills[selectedSkillIndex] ?? null;
  const skillsBusy = skillsLoading || skillsOperation !== null;

  useEffect(() => {
    if (activeTab !== "skills") return;
    void refreshSkills();
  }, [activeTab, effectiveRepoRoot, skillFilter]);

  useEffect(() => {
    if (activeTab !== "skills" || !selectedSkill) {
      setSkillDetail(null);
      return;
    }
    void loadSkill(selectedSkill.name);
  }, [activeTab, effectiveRepoRoot, skillFilter, selectedSkill?.name]);

  async function refreshSkills(quiet = false) {
    if (!effectiveRepoRoot) {
      setSkillsError("Repo root is not resolved.");
      setSkillInventory(null);
      setSkillDetail(null);
      return;
    }
    setSkillsLoading(true);
    setSkillsError(null);
    try {
      const inventory = await getSkills(effectiveRepoRoot, skillFilter);
      setSkillInventory(inventory);
      setSelectedSkillIndex((current) => {
        if (!inventory.skills.length) return 0;
        return Math.min(current, inventory.skills.length - 1);
      });
      if (!quiet) setStatusMessage(`Loaded ${inventory.skills.length} skill(s).`);
    } catch (error) {
      const message = (error as Error).message;
      setSkillsError(message);
      recordLocalDiagnostic("skills", "refresh", message);
      setStatusMessage(`Skills unavailable: ${message}`);
    } finally {
      setSkillsLoading(false);
    }
  }

  async function loadSkill(skillName: string) {
    if (!effectiveRepoRoot) return;
    try {
      setSkillDetail(await getSkill(effectiveRepoRoot, skillName, skillFilter));
    } catch (error) {
      const message = (error as Error).message;
      setSkillDetail(null);
      recordLocalDiagnostic("skills", "read", message);
    }
  }

  async function handleRepairSkills() {
    if (!effectiveRepoRoot) {
      setSkillsError("Repo root is not resolved.");
      return;
    }
    setSkillsOperation("repairing skills");
    setSkillsError(null);
    try {
      const report = await repairSkills(effectiveRepoRoot, skillFilter);
      setSkillInventory(report.inventory);
      setStatusMessage(
        `Repaired skills: ${report.inventory.summary}${
          report.backup_root ? `; backup ${report.backup_root}` : ""
        }.`,
      );
      const selected =
        report.inventory.skills[Math.min(selectedSkillIndex, report.inventory.skills.length - 1)];
      if (selected) await loadSkill(selected.name);
    } catch (error) {
      const message = (error as Error).message;
      setSkillsError(message);
      recordLocalDiagnostic("skills", "repair", message);
      setStatusMessage(`Skill repair failed: ${message}`);
    } finally {
      setSkillsOperation(null);
    }
  }

  function handleSkillFilterChange(filter: SkillInventoryFilter) {
    setSelectedSkillIndex(0);
    setSkillDetail(null);
    setSkillFilter(filter);
  }

  return {
    skillInventory,
    skillDetail,
    selectedSkill,
    selectedSkillIndex,
    setSelectedSkillIndex,
    skillFilter,
    setSkillFilter: handleSkillFilterChange,
    skillsLoading,
    skillsOperation,
    skillsBusy,
    skillsError,
    refreshSkills,
    handleRepairSkills,
  };
}
