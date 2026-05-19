import { Metric } from "../../components/Details";
import { formatDateTime } from "../../utils/format";
import type { ErrorItem } from "./errorItems";

interface ErrorsTabProps {
  errorItems: ErrorItem[];
  activeError: ErrorItem | null;
  selectedErrorIndex: number;
  onSelectError: (index: number) => void;
}

export function ErrorsTab({ errorItems, activeError, selectedErrorIndex, onSelectError }: ErrorsTabProps) {
  return (
    <section className="panel-grid">
      <div className="panel">
        <div className="list-view">
          {errorItems.length ? (
            errorItems.map((item, index) => (
              <button
                key={`${item.when ?? "session"}-${item.diagnostic.code}-${index}`}
                type="button"
                className={`list-item ${selectedErrorIndex === index ? "selected" : ""}`}
                onClick={() => onSelectError(index)}
              >
                <div>
                  <strong>{item.diagnostic.message}</strong>
                  <p>{item.diagnostic.source} · {item.diagnostic.component} · {item.diagnostic.operation}</p>
                </div>
                <div className="meta-stack">
                  <span className={`badge badge-${item.diagnostic.severity === "error" ? "archived" : "active"}`}>{item.diagnostic.severity}</span>
                  <span>{formatDateTime(item.when)}</span>
                </div>
              </button>
            ))
          ) : (
            <p className="muted">No diagnostics recorded for this project or browser session.</p>
          )}
        </div>
      </div>
      <div className="panel detail-scroll">
        {activeError ? (
          <>
            <h2>{activeError.diagnostic.code || "diagnostic"}</h2>
            <Metric label="When" value={formatDateTime(activeError.when)} />
            <Metric label="Severity" value={activeError.diagnostic.severity} />
            <Metric label="Source" value={activeError.diagnostic.source || "unknown"} />
            <Metric label="Component" value={activeError.diagnostic.component || "unknown"} />
            <Metric label="Operation" value={activeError.diagnostic.operation || "unknown"} />
            <section className="detail-section">
              <h3>Summary</h3>
              <p>{activeError.diagnostic.message}</p>
            </section>
            {activeError.diagnostic.explanation ? (
              <section className="detail-section">
                <h3>Explanation</h3>
                <p>{activeError.diagnostic.explanation}</p>
              </section>
            ) : null}
            {activeError.diagnostic.fix_hint ? (
              <section className="detail-section">
                <h3>How to fix</h3>
                <p>{activeError.diagnostic.fix_hint}</p>
              </section>
            ) : null}
            {activeError.diagnostic.doctor_hint || activeError.diagnostic.command_hint ? (
              <section className="detail-section">
                <h3>Commands</h3>
                {activeError.diagnostic.doctor_hint ? <code>{activeError.diagnostic.doctor_hint}</code> : null}
                {activeError.diagnostic.command_hint ? <code>{activeError.diagnostic.command_hint}</code> : null}
              </section>
            ) : null}
            {activeError.diagnostic.raw_error ? (
              <section className="detail-section">
                <h3>Raw error</h3>
                <pre>{activeError.diagnostic.raw_error}</pre>
              </section>
            ) : null}
          </>
        ) : (
          <p className="muted">Provider errors, query failures, watcher failures, and browser connection errors will appear here.</p>
        )}
      </div>
    </section>
  );
}
