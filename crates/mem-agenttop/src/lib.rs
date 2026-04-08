mod collector;
mod model;

use chrono::{DateTime, Utc};

// Adapted from graykode/abtop (MIT) to provide structured agent/session data
// for Memory Layer's native TUI.
pub use model::{AgentSession, ChildProcess, OrphanPort, RateLimitInfo, SessionStatus, SubAgent};

#[derive(Debug, Clone)]
pub struct AgentSnapshot {
    pub collected_at: DateTime<Utc>,
    pub sessions: Vec<AgentSession>,
    pub orphan_ports: Vec<OrphanPort>,
    pub rate_limits: Vec<RateLimitInfo>,
}

pub struct AgentTop {
    collector: collector::MultiCollector,
}

impl AgentTop {
    pub fn new() -> Self {
        Self {
            collector: collector::MultiCollector::new(),
        }
    }

    pub fn collect_snapshot(&mut self) -> AgentSnapshot {
        AgentSnapshot {
            collected_at: Utc::now(),
            sessions: self.collector.collect(),
            orphan_ports: self.collector.orphan_ports.clone(),
            rate_limits: self.collector.agent_rate_limits(),
        }
    }
}

impl Default for AgentTop {
    fn default() -> Self {
        Self::new()
    }
}
