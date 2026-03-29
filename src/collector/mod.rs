pub mod claude;
pub mod codex;
pub mod process;
pub mod rate_limit;

pub use claude::ClaudeCollector;
pub use codex::CodexCollector;
pub use rate_limit::read_rate_limits;

use crate::model::AgentSession;
use std::collections::HashMap;

/// Trait for agent session collectors.
/// Implement this for each CLI tool (Claude Code, Codex, etc.)
pub trait Collector {
    fn collect(&mut self, shared: &SharedProcessData) -> Vec<AgentSession>;
}

/// Process data fetched once per tick and shared across all collectors.
/// Avoids duplicate ps/lsof calls.
pub struct SharedProcessData {
    pub process_info: HashMap<u32, process::ProcInfo>,
    pub children_map: HashMap<u32, Vec<u32>>,
    pub ports: HashMap<u32, Vec<u16>>,
}

impl SharedProcessData {
    pub fn fetch() -> Self {
        let process_info = process::get_process_info();
        let children_map = process::get_children_map(&process_info);
        let ports = process::get_listening_ports();
        Self { process_info, children_map, ports }
    }
}

impl Collector for ClaudeCollector {
    fn collect(&mut self, shared: &SharedProcessData) -> Vec<AgentSession> {
        ClaudeCollector::collect(self, shared)
    }
}

impl Collector for CodexCollector {
    fn collect(&mut self, shared: &SharedProcessData) -> Vec<AgentSession> {
        CodexCollector::collect(self, shared)
    }
}

/// Aggregates sessions from multiple collectors (Claude, Codex, etc.)
pub struct MultiCollector {
    collectors: Vec<Box<dyn Collector>>,
}

impl MultiCollector {
    pub fn new() -> Self {
        Self {
            collectors: vec![
                Box::new(ClaudeCollector::new()),
                Box::new(CodexCollector::new()),
            ],
        }
    }

    /// Register an additional collector (e.g., for Codex support)
    #[allow(dead_code)]
    pub fn add<C: Collector + 'static>(&mut self, collector: C) {
        self.collectors.push(Box::new(collector));
    }

    pub fn collect(&mut self) -> Vec<AgentSession> {
        let shared = SharedProcessData::fetch();
        let mut all = Vec::new();
        for c in &mut self.collectors {
            all.extend(c.collect(&shared));
        }
        all.sort_by_key(|s| std::cmp::Reverse(s.started_at));
        all
    }
}
