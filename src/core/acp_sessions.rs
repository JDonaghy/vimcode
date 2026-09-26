//! Local index of past ACP sessions (#1459).
//!
//! The agent-client-protocol has no `session/list` method — an agent can
//! only be asked to *resume* a session it already knows the id of
//! (`session/load`), never to enumerate what it has. So the `:AiSessions`
//! picker's list comes from vimcode's own bookkeeping: every session this
//! client has itself created is recorded here (id, agent name, cwd, first
//! prompt, timestamp) the moment `session/new` succeeds
//! (`Engine::poll_acp`'s `SessionCreated` handler). This is deliberately
//! *not* an attempt to mirror agent-side history in general — an id created
//! by a different ACP client entirely (e.g. the agent's own CLI) is
//! invisible to vimcode and cannot appear here; only sessions vimcode itself
//! started are resumable through this picker.
//!
//! Also doubles as the cache for whether the active agent advertised
//! `agentCapabilities.loadSession` on its most recent `initialize` — so
//! `:AiSessions` can answer "this agent doesn't support resume" without
//! having to spawn the agent first just to ask (`Engine::poll_acp`'s
//! `Initialized` handler records it every time).
//!
//! Persisted to `<vimcode config dir>/acp_sessions.json`, gated by the same
//! `crate::core::session::{saves_suppressed, loads_suppressed}` flags
//! `HistoryState` uses, plus the same `#[cfg(test)]` "always `Default`"
//! guard on `load`/no-op on `save` so `cargo test --lib` never touches a
//! real `~/.config/vimcode/` (see `HistoryState::load`'s doc for the
//! cross-test corruption that guard exists to prevent).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Max sessions kept per (agent, workspace) pair — oldest (by
/// `updated_at`) dropped first. This is a "resume yesterday's chat" picker,
/// not an archive, so it stays deliberately small.
const MAX_RECORDS_PER_SCOPE: usize = 20;

/// One session vimcode itself created, remembered so it can be offered
/// again by `:AiSessions` in a later run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AcpSessionRecord {
    pub session_id: String,
    pub agent_name: String,
    pub cwd: PathBuf,
    /// The literal text of the first `session/prompt` sent on this session
    /// — shown in the picker as the entry's identifying detail, since a raw
    /// session id means nothing to a human.
    pub first_prompt: String,
    /// Unix seconds, for "most recent first" ordering.
    pub updated_at: u64,
}

/// The whole persisted index: every remembered session across every agent
/// and workspace, plus the learned `loadSession` capability per agent name.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AcpSessionIndex {
    #[serde(default)]
    pub records: Vec<AcpSessionRecord>,
    /// Learned `agentCapabilities.loadSession` per agent name (registry
    /// entry name, or the legacy single-agent setting's label — whatever
    /// `Engine::acp_active_agent_name` returns). Absent = never learned yet
    /// (no `initialize` response seen this run or any prior one).
    #[serde(default)]
    pub load_session_by_agent: HashMap<String, bool>,
}

fn index_path() -> PathBuf {
    super::paths::vimcode_config_dir().join("acp_sessions.json")
}

impl AcpSessionIndex {
    /// Load the persisted index, or `Default` if absent/unreadable/
    /// malformed. Always `Default` in `--lib` unit test builds — see the
    /// module doc.
    pub fn load() -> Self {
        #[cfg(test)]
        return Self::default();

        #[cfg_attr(test, allow(unreachable_code))]
        {
            if super::session::loads_suppressed() {
                return Self::default();
            }
            std::fs::read_to_string(index_path())
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        }
    }

    /// Persist to disk. A no-op in `--lib` unit test builds and whenever
    /// `crate::core::session::suppress_disk_saves` has been called (the
    /// integration-test lane) — see the module doc.
    pub fn save(&self) {
        #[cfg(test)]
        return;

        #[cfg_attr(test, allow(unreachable_code))]
        {
            if super::session::saves_suppressed() {
                return;
            }
            let path = index_path();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(path, json);
            }
        }
    }

    /// Record a freshly-created session, or refresh `updated_at` if
    /// `session_id` is already known (shouldn't normally happen — session
    /// ids are agent-assigned and unique per `session/new` call — but keeps
    /// this idempotent rather than accumulating duplicates on a replay).
    /// Trims the oldest entry in this exact (agent, cwd) scope once it
    /// exceeds [`MAX_RECORDS_PER_SCOPE`], leaving every other scope
    /// untouched.
    pub fn record_session(
        &mut self,
        session_id: &str,
        agent_name: &str,
        cwd: &Path,
        first_prompt: &str,
    ) {
        let now = now_unix_secs();
        if let Some(existing) = self.records.iter_mut().find(|r| r.session_id == session_id) {
            existing.updated_at = now;
            return;
        }
        self.records.push(AcpSessionRecord {
            session_id: session_id.to_string(),
            agent_name: agent_name.to_string(),
            cwd: cwd.to_path_buf(),
            first_prompt: first_prompt.to_string(),
            updated_at: now,
        });

        let mut scope_indices: Vec<usize> = self
            .records
            .iter()
            .enumerate()
            .filter(|(_, r)| r.agent_name == agent_name && r.cwd == cwd)
            .map(|(i, _)| i)
            .collect();
        if scope_indices.len() > MAX_RECORDS_PER_SCOPE {
            scope_indices.sort_by_key(|&i| self.records[i].updated_at);
            let drop_count = scope_indices.len() - MAX_RECORDS_PER_SCOPE;
            let mut to_drop: Vec<usize> = scope_indices[..drop_count].to_vec();
            // Remove highest index first so earlier removals don't shift
            // the indices still queued for removal.
            to_drop.sort_unstable_by(|a, b| b.cmp(a));
            for idx in to_drop {
                self.records.remove(idx);
            }
        }
    }

    /// Sessions recorded for `agent_name`/`cwd`, most-recently-updated
    /// first — the order `:AiSessions` lists them in.
    pub fn sessions_for(&self, agent_name: &str, cwd: &Path) -> Vec<AcpSessionRecord> {
        let mut matches: Vec<AcpSessionRecord> = self
            .records
            .iter()
            .filter(|r| r.agent_name == agent_name && r.cwd == cwd)
            .cloned()
            .collect();
        matches.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
        matches
    }

    /// Record whether `agent_name` advertised `loadSession` on its most
    /// recent `initialize` response.
    pub fn set_load_session_capability(&mut self, agent_name: &str, supported: bool) {
        self.load_session_by_agent
            .insert(agent_name.to_string(), supported);
    }

    /// The learned `loadSession` capability for `agent_name`, or `None` if
    /// this client has never seen an `initialize` response from it (in this
    /// run or a previous one).
    pub fn load_session_capability(&self, agent_name: &str) -> Option<bool> {
        self.load_session_by_agent.get(agent_name).copied()
    }
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_session_is_idempotent_by_id_and_lists_most_recent_first() {
        let mut idx = AcpSessionIndex::default();
        let cwd = PathBuf::from("/work/proj");
        idx.record_session("s1", "claude", &cwd, "hello");
        // `updated_at` is second-resolution (Unix seconds), so the two
        // records need a full second between them to land in different
        // buckets and prove the ordering, not just millisecond jitter.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        idx.record_session("s2", "claude", &cwd, "world");

        let sessions = idx.sessions_for("claude", &cwd);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "s2", "most recent first");
        assert_eq!(sessions[1].session_id, "s1");

        // Re-recording the same id must not duplicate it.
        idx.record_session("s1", "claude", &cwd, "hello");
        assert_eq!(idx.sessions_for("claude", &cwd).len(), 2);
    }

    #[test]
    fn sessions_for_is_scoped_by_agent_and_cwd() {
        let mut idx = AcpSessionIndex::default();
        idx.record_session("s1", "claude", Path::new("/a"), "p1");
        idx.record_session("s2", "gemini", Path::new("/a"), "p2");
        idx.record_session("s3", "claude", Path::new("/b"), "p3");

        assert_eq!(idx.sessions_for("claude", Path::new("/a")).len(), 1);
        assert_eq!(idx.sessions_for("gemini", Path::new("/a")).len(), 1);
        assert_eq!(idx.sessions_for("claude", Path::new("/b")).len(), 1);
        assert!(idx.sessions_for("claude", Path::new("/c")).is_empty());
    }

    #[test]
    fn record_session_trims_oldest_past_the_per_scope_cap() {
        let mut idx = AcpSessionIndex::default();
        let cwd = PathBuf::from("/work");
        for i in 0..(MAX_RECORDS_PER_SCOPE + 5) {
            idx.record_session(&format!("s{i}"), "claude", &cwd, "p");
        }
        let sessions = idx.sessions_for("claude", &cwd);
        assert_eq!(sessions.len(), MAX_RECORDS_PER_SCOPE);
        // The earliest ids should have been dropped, keeping the latest.
        assert!(sessions.iter().any(|r| r.session_id == "s24"));
        assert!(!sessions.iter().any(|r| r.session_id == "s0"));
    }

    #[test]
    fn load_session_capability_is_unknown_until_recorded() {
        let mut idx = AcpSessionIndex::default();
        assert_eq!(idx.load_session_capability("claude"), None);
        idx.set_load_session_capability("claude", true);
        assert_eq!(idx.load_session_capability("claude"), Some(true));
        idx.set_load_session_capability("claude", false);
        assert_eq!(idx.load_session_capability("claude"), Some(false));
    }

    #[test]
    fn serde_round_trip_preserves_records_and_capabilities() {
        let mut idx = AcpSessionIndex::default();
        idx.record_session("s1", "claude", Path::new("/work"), "hi");
        idx.set_load_session_capability("claude", true);
        let json = serde_json::to_string(&idx).unwrap();
        let back: AcpSessionIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(back, idx);
    }
}
