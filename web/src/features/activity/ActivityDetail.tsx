import type { ReactNode } from "react";

import { KeyValueList } from "../../components/Details";
import { RichText } from "../../components/RichText";
import type { ActivityEvent } from "../../types";
import { activityDurationLabel, activityTokenLabel, formatDateTime, formatTokens } from "../../utils/format";

export function ActivityDetail({ event }: { event: ActivityEvent }) {
  const details = event.details;
  const eventRows: [string, string][] = [
    ["Tokens", activityTokenLabel(event)],
    ["Duration", activityDurationLabel(event)],
    ["Provider", event.provider ?? "n/a"],
    ["Model", event.model ?? "n/a"],
    ["Source", event.source ?? "n/a"],
    ["Operation", event.operation_id ?? "n/a"],
  ];
  if (event.token_usage) {
    eventRows.push(
      ["Input tokens", formatTokens(event.token_usage.input_tokens)],
      ["Output tokens", formatTokens(event.token_usage.output_tokens)],
      ["Cache read", formatTokens(event.token_usage.cache_read_tokens)],
      ["Cache write", formatTokens(event.token_usage.cache_write_tokens)],
    );
  }

  if (!details) {
    return (
      <>
        <div className="detail-section">
          <h3>Execution</h3>
          <KeyValueList items={eventRows} empty="No execution metadata recorded." />
        </div>
        <p className="muted">No structured details recorded.</p>
      </>
    );
  }
  const rows: [string, string][] = [];
  const sections: ReactNode[] = [];

  switch (details.type) {
    case "checkpoint":
      rows.push(["Marked at", formatDateTime(details.marked_at)], ["Repo root", details.repo_root], ["Note", details.note ?? "n/a"], ["Branch", details.git_branch ?? "n/a"], ["HEAD", details.git_head ?? "n/a"]);
      break;
    case "plan":
      rows.push(["Action", details.action], ["Title", details.title], ["Thread", details.thread_key], ["Completed", `${details.completed_items}/${details.total_items}`], ["Verified complete", String(details.verified_complete)], ["Source path", details.source_path ?? "n/a"]);
      if (details.remaining_items.length) {
        sections.push(<ActivityList key="remaining" title="Remaining items" items={details.remaining_items} />);
      }
      break;
    case "scan":
      rows.push(["Dry run", String(details.dry_run)], ["Candidates", String(details.candidate_count)], ["Files", String(details.files_considered)], ["Commits", String(details.commits_considered)], ["Index reused", String(details.index_reused)], ["Report", details.report_path], ["Capture", details.capture_id ?? "n/a"], ["Curate run", details.curate_run_id ?? "n/a"]);
      break;
    case "graph_extract":
      rows.push(
        ["Repo root", details.repo_root],
        ["Extraction run", details.extraction_run_id ?? "n/a"],
        ["Dry run", String(details.dry_run)],
        ["Reused existing run", String(details.reused_existing_run)],
        ["Index reused", String(details.index_reused)],
        ["Analyzer", details.analyzer_version],
        ["Strategy", details.strategy_version],
        ["Symbols", String(details.symbol_count)],
        ["References", String(details.reference_count)],
        ["Resolved", String(details.resolved_reference_count)],
        ["Unresolved", String(details.unresolved_reference_count)],
        ["Ambiguous", String(details.ambiguous_reference_count)],
        ["Graph nodes", String(details.graph_node_count)],
        ["Graph edges", String(details.graph_edge_count)],
        ["Evidence", String(details.evidence_count)],
        ["HEAD", details.git_head ?? "n/a"],
        ["Since", details.since ?? "n/a"],
      );
      break;
    case "commit_sync":
      rows.push(["Imported", String(details.imported_count)], ["Updated", String(details.updated_count)], ["Received", String(details.total_received)], ["Newest", details.newest_commit ?? "n/a"], ["Oldest", details.oldest_commit ?? "n/a"]);
      break;
    case "bundle_transfer":
      rows.push(["Bundle", details.bundle_id], ["Items", String(details.item_count)], ["Source project", details.source_project ?? "n/a"]);
      break;
    case "query":
      rows.push(["Query", details.query], ["Top K", String(details.top_k)], ["Results", String(details.result_count)], ["Confidence", details.confidence.toFixed(2)], ["Insufficient evidence", String(details.insufficient_evidence)], ["Duration", `${details.total_duration_ms} ms`]);
      rows.push(["Graph status", details.graph_status ?? "n/a"], ["Graph candidates", String(details.graph_candidates)], ["Graph augmented", String(details.graph_augmented_candidates)], ["Graph duration", `${details.graph_duration_ms} ms`], ["Graph result count", String(details.graph_result_count)], ["Graph connections", String(details.graph_connection_count)]);
      if (details.graph_connections.length) {
        sections.push(
          <section className="detail-section" key="graph-connections">
            <h3>Graph connections</h3>
            {details.graph_connections.map((connection, index) => (
              <div key={`${connection.file_path}-${index}`} className="relation-row">
                <span className="badge">+{connection.score_boost.toFixed(2)}</span>
                <span>{connection.reason}</span>
                <span className="muted">{connection.file_path}</span>
              </div>
            ))}
          </section>,
        );
      }
      if (details.answer) sections.push(<ActivityText key="answer" title="Answer" text={details.answer} />);
      if (details.error) rows.push(["Error", details.error]);
      break;
    case "llm_audit":
      rows.push(["Operation", details.operation], ["Request", details.request_summary], ["Status", details.status], ["Redacted", String(details.redacted)], ["Truncated", String(details.truncated)], ["Error", details.error ?? "n/a"]);
      if (details.messages.length) {
        sections.push(
          <section className="detail-section" key="llm-audit-messages">
            <h3>Messages</h3>
            {details.messages.map((message, index) => (
              <div key={`${message.role}-${index}`} className="source-card">
                <strong>{message.role}{message.truncated ? " (truncated)" : ""}</strong>
                <pre>{message.content}</pre>
              </div>
            ))}
          </section>,
        );
      }
      break;
    case "watcher_health":
      rows.push(["Watcher", details.watcher_id], ["Hostname", details.hostname], ["Health", details.health], ["Previous health", details.previous_health ?? "n/a"], ["Managed by service", String(details.managed_by_service)], ["Restart attempts", String(details.restart_attempt_count)], ["Recovered after attempts", details.recovered_after_restart_attempts?.toString() ?? "n/a"], ["Agent CLI", details.agent_cli ?? "n/a"], ["Agent session", details.agent_session_id ?? "n/a"], ["Agent PID", details.agent_pid?.toString() ?? "n/a"], ["Message", details.message ?? "n/a"]);
      break;
    case "memory_replacement":
      rows.push(["Old memory", details.old_memory_id], ["Old summary", details.old_summary], ["New memory", details.new_memory_id], ["New summary", details.new_summary], ["Automatic", String(details.automatic)], ["Policy", details.policy]);
      break;
    case "capture_task":
      rows.push(["Session", details.session_id], ["Task", details.task_id], ["Raw capture", details.raw_capture_id], ["Idempotency", details.idempotency_key], ["Task title", details.task_title ?? "n/a"], ["Writer", details.writer_id]);
      break;
    case "curate":
      rows.push(["Run", details.run_id], ["Input captures", String(details.input_count)], ["Output memories", String(details.output_count)], ["Replacements", String(details.replaced_count)], ["Queued proposals", String(details.proposal_count)]);
      break;
    case "reindex":
      rows.push(["Reindexed entries", String(details.reindexed_entries)]);
      break;
    case "reembed":
      rows.push(["Re-embedded chunks", String(details.reembedded_chunks)]);
      break;
    case "archive":
      rows.push(["Archived count", String(details.archived_count)], ["Max confidence", details.max_confidence.toFixed(2)], ["Max importance", String(details.max_importance)]);
      break;
    case "delete_memory":
      rows.push(["Deleted", String(details.deleted)], ["Deleted summary", details.summary]);
      break;
    case "diagnostic":
      rows.push(["Code", details.diagnostic.code], ["Severity", details.diagnostic.severity], ["Source", details.diagnostic.source], ["Component", details.diagnostic.component], ["Operation", details.diagnostic.operation], ["Message", details.diagnostic.message], ["Doctor", details.diagnostic.doctor_hint ?? "n/a"], ["Command", details.diagnostic.command_hint ?? "n/a"]);
      if (details.diagnostic.explanation) sections.push(<ActivityText key="diag-explanation" title="Explanation" text={details.diagnostic.explanation} />);
      if (details.diagnostic.fix_hint) sections.push(<ActivityText key="diag-fix" title="How to fix" text={details.diagnostic.fix_hint} />);
      if (details.diagnostic.raw_error) sections.push(<ActivityText key="diag-raw" title="Raw error" text={details.diagnostic.raw_error} />);
      break;
  }

  return (
    <>
      <div className="detail-section">
        <h3>Execution</h3>
        <KeyValueList items={eventRows} empty="No execution metadata recorded." />
      </div>
      <div className="detail-section">
        <h3>Details</h3>
        <KeyValueList items={rows} empty="No structured details recorded." />
        {sections}
      </div>
    </>
  );
}

function ActivityList({ title, items }: { title: string; items: string[] }) {
  return (
    <section className="detail-section">
      <h3>{title}</h3>
      <ul>{items.map((item) => <li key={item}>{item}</li>)}</ul>
    </section>
  );
}

function ActivityText({ title, text }: { title: string; text: string }) {
  return (
    <section className="detail-section">
      <h3>{title}</h3>
      <RichText text={text} />
    </section>
  );
}
