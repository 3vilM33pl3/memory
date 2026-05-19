import { Metric } from "../../components/Details";
import type { ProjectOverviewResponse } from "../../types";
import { formatDateTime } from "../../utils/format";

interface WatchersTabProps {
  overview: ProjectOverviewResponse;
  project: string;
}

export function WatchersTab({ overview, project }: WatchersTabProps) {
  return (
    <section className="panel-stack">
      <div className="panel">
        <h2>Watcher presence</h2>
        {overview.watchers ? (
          <>
            <Metric label="Active watchers" value={String(overview.watchers.active_count)} />
            <Metric label="Unhealthy watchers" value={String(overview.watchers.unhealthy_count)} />
            <Metric label="Stale after" value={`${overview.watchers.stale_after_seconds}s`} />
            <Metric label="Last heartbeat" value={formatDateTime(overview.watchers.last_heartbeat_at)} />
          </>
        ) : (
          <p className="muted">No watcher presence data.</p>
        )}
      </div>
      <div className="panel">
        <h2>Watchers</h2>
        {overview.watchers?.watchers.length ? (
          overview.watchers.watchers.map((watcher) => (
            <div key={watcher.watcher_id} className="watcher-card">
              <strong>{watcher.hostname}</strong>
              <p>{watcher.repo_root}</p>
              <div className="stats-row">
                <span>pid {watcher.pid}</span>
                <span>{watcher.mode}</span>
                <span className={`badge ${watcher.health === "healthy" ? "badge-active" : "badge-archived"}`}>{watcher.health}</span>
                <span>{watcher.managed_by_service ? "managed" : "manual"}</span>
                <span>started {formatDateTime(watcher.started_at)}</span>
                <span>{formatDateTime(watcher.last_heartbeat_at)}</span>
                <span>restarts {watcher.restart_attempt_count}</span>
                <span className="muted">{watcher.watcher_id}</span>
              </div>
              <p className="muted">Host service {watcher.host_service_id}</p>
              {watcher.agent_session_id ? (
                <p className="muted">{watcher.agent_cli} session {watcher.agent_session_id} · agent pid {watcher.agent_pid ?? "n/a"}</p>
              ) : null}
              {watcher.last_restart_attempt_at ? (
                <p className="muted">Last restart attempt {formatDateTime(watcher.last_restart_attempt_at)}</p>
              ) : null}
            </div>
          ))
        ) : (
          <p className="muted">
            No watcher presence reported. Start one with{" "}
            <code>memory watcher run --project {project}</code> or enable the watcher manager with{" "}
            <code>memory watcher manager enable</code>.
          </p>
        )}
      </div>
    </section>
  );
}
