import type { ProjectMemoryBundlePreview, ProjectMemoryExportOptions, ProjectMemoryImportPreview } from "../../types";

interface BundlesTabProps {
  bundleOptions: ProjectMemoryExportOptions;
  exportPreview: ProjectMemoryBundlePreview | null;
  importPreview: ProjectMemoryImportPreview | null;
  onBundleOptionsChange: (next: ProjectMemoryExportOptions) => void;
  onImportFileChange: (file: File | null) => void;
  onPreviewExport: () => void;
  onDownloadExport: () => void;
  onPreviewImport: () => void;
  onApplyImport: () => void;
}

export function BundlesTab({
  bundleOptions,
  exportPreview,
  importPreview,
  onBundleOptionsChange,
  onImportFileChange,
  onPreviewExport,
  onDownloadExport,
  onPreviewImport,
  onApplyImport,
}: BundlesTabProps) {
  return (
    <section className="panel-grid">
      <div className="panel detail-scroll">
        <h2>Export bundle</h2>
        <label><input type="checkbox" checked={bundleOptions.include_archived} onChange={(event) => onBundleOptionsChange({ ...bundleOptions, include_archived: event.target.checked })} /> Include archived memories</label>
        <label><input type="checkbox" checked={bundleOptions.include_tags} onChange={(event) => onBundleOptionsChange({ ...bundleOptions, include_tags: event.target.checked })} /> Include tags</label>
        <label><input type="checkbox" checked={bundleOptions.include_relations} onChange={(event) => onBundleOptionsChange({ ...bundleOptions, include_relations: event.target.checked })} /> Include relations</label>
        <label><input type="checkbox" checked={bundleOptions.include_source_file_paths} onChange={(event) => onBundleOptionsChange({ ...bundleOptions, include_source_file_paths: event.target.checked })} /> Include source file paths</label>
        <label><input type="checkbox" checked={bundleOptions.include_git_commits} onChange={(event) => onBundleOptionsChange({ ...bundleOptions, include_git_commits: event.target.checked })} /> Include git commit hashes</label>
        <label><input type="checkbox" checked={bundleOptions.include_source_excerpts} onChange={(event) => onBundleOptionsChange({ ...bundleOptions, include_source_excerpts: event.target.checked })} /> Include source excerpts</label>
        <div className="actions-row">
          <button onClick={onPreviewExport} type="button">Preview export</button>
          <button onClick={onDownloadExport} type="button">Download bundle</button>
        </div>
        {exportPreview ? (
          <>
            <p className="muted">{exportPreview.memory_count} memories · {exportPreview.relation_count} relations · {exportPreview.warning_count} warnings</p>
            <pre className="code-block">{exportPreview.summary_markdown}</pre>
            {exportPreview.warnings.length ? (
              <ul className="warning-list">{exportPreview.warnings.map((warning) => <li key={warning}>{warning}</li>)}</ul>
            ) : null}
          </>
        ) : (
          <p className="muted">Export a versioned, shareable bundle of the current project's curated memories.</p>
        )}
      </div>
      <div className="panel detail-scroll">
        <h2>Import bundle</h2>
        <input type="file" accept=".zip,.mlbundle.zip" onChange={(event) => onImportFileChange(event.target.files?.[0] ?? null)} />
        <div className="actions-row">
          <button onClick={onPreviewImport} type="button">Preview import</button>
          <button onClick={onApplyImport} type="button">Import bundle</button>
        </div>
        {importPreview ? (
          <>
            <p className="muted">{importPreview.memory_count} memories · {importPreview.new_count} new · {importPreview.unchanged_count} unchanged · {importPreview.replacing_count} replacing</p>
            <pre className="code-block">{importPreview.summary_markdown}</pre>
            {importPreview.warnings.length ? (
              <ul className="warning-list">{importPreview.warnings.map((warning) => <li key={warning}>{warning}</li>)}</ul>
            ) : null}
          </>
        ) : (
          <p className="muted">Upload a bundle to preview and import it into the current project.</p>
        )}
      </div>
    </section>
  );
}
