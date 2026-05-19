import { useEffect } from "react";

import { PRIMARY_TABS, type Tab } from "../tabs";
import type { EmbeddingBackendInfo } from "../types";

interface GlobalShortcutsOptions {
  tab: Tab;
  setTab: (tab: Tab) => void;
  helpOpen: boolean;
  setHelpOpen: (updater: (current: boolean) => boolean) => void;
  searchRef: React.RefObject<HTMLInputElement | null>;
  queryRef: React.RefObject<HTMLInputElement | null>;
  project: string;
  refreshProject: (project: string) => Promise<void>;
  selectedEmbeddingBackend: EmbeddingBackendInfo | null;
  embeddingBusy: boolean;
  refreshEmbeddings: () => Promise<void>;
  handleToggleEmbeddingSearch: (backend: EmbeddingBackendInfo) => Promise<void>;
  handleToggleEmbeddingCreation: (backend: EmbeddingBackendInfo) => Promise<void>;
  handleReembedEmbeddingBackend: (backend: EmbeddingBackendInfo) => Promise<void>;
  handleReindexEmbeddingBackend: (backend: EmbeddingBackendInfo) => Promise<void>;
}

export function useGlobalShortcuts({
  tab,
  setTab,
  helpOpen,
  setHelpOpen,
  searchRef,
  queryRef,
  project,
  refreshProject,
  selectedEmbeddingBackend,
  embeddingBusy,
  refreshEmbeddings,
  handleToggleEmbeddingSearch,
  handleToggleEmbeddingCreation,
  handleReembedEmbeddingBackend,
  handleReindexEmbeddingBackend,
}: GlobalShortcutsOptions) {
  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      const target = e.target as HTMLElement;
      const inInput = target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT";
      if (!inInput && (e.key === "h" || e.key === "H")) {
        e.preventDefault();
        setHelpOpen((current) => !current);
        return;
      }
      if (!inInput && e.key === "Escape" && helpOpen) {
        e.preventDefault();
        setHelpOpen(() => false);
        return;
      }
      if (inInput) return;
      const tabIndex = parseInt(e.key, 10) - 1;
      if (tabIndex >= 0 && tabIndex < PRIMARY_TABS.length) {
        e.preventDefault();
        setTab(PRIMARY_TABS[tabIndex]);
        return;
      }
      if (e.key === "/" && tab === "memories") {
        e.preventDefault();
        searchRef.current?.focus();
        return;
      }
      if (e.key === "?" && tab === "query") {
        e.preventDefault();
        queryRef.current?.focus();
        return;
      }
      if (tab === "embeddings" && selectedEmbeddingBackend) {
        if (handleEmbeddingKey(e, selectedEmbeddingBackend)) return;
      }
      if (e.key === "r") {
        e.preventDefault();
        void refreshProject(project);
      }
    }

    function handleEmbeddingKey(e: KeyboardEvent, backend: EmbeddingBackendInfo): boolean {
      if (e.key === "r") {
        e.preventDefault();
        void refreshEmbeddings();
        return true;
      }
      if (embeddingBusy) return false;
      if (e.key === "Enter") {
        e.preventDefault();
        void handleToggleEmbeddingSearch(backend);
        return true;
      }
      if (e.key === "c") {
        e.preventDefault();
        void handleToggleEmbeddingCreation(backend);
        return true;
      }
      if (e.key === "e") {
        e.preventDefault();
        void handleReembedEmbeddingBackend(backend);
        return true;
      }
      if (e.key === "I") {
        e.preventDefault();
        void handleReindexEmbeddingBackend(backend);
        return true;
      }
      return false;
    }

    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [
    embeddingBusy,
    handleReembedEmbeddingBackend,
    handleReindexEmbeddingBackend,
    handleToggleEmbeddingCreation,
    handleToggleEmbeddingSearch,
    helpOpen,
    project,
    queryRef,
    refreshEmbeddings,
    refreshProject,
    searchRef,
    selectedEmbeddingBackend,
    setHelpOpen,
    setTab,
    tab,
  ]);
}
