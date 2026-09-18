//! A tenant: one agent launch UZE made into a space's own directory, on
//! whatever branch that directory is on.
//!
//! # Not a task
//!
//! A task has a slot, a branch of its own and a way home for its work;
//! seven of its nine states presuppose that branch. A tenant has none of
//! it: it works where the operator works, its commits land on the branch
//! the root is on, and there is nothing for UZE to deliver, name or park.
//! What the two share is exactly identity and conversation — the
//! [`AgentId`] its launch carries — and nothing else, which is why a tenant
//! is a record of its own beside the tasks rather than a task with fields
//! that would have to lie.
//!
//! # Lifetime
//!
//! A tenant is live from its launch until the space's panes are reconciled
//! and none of them echoes its identity. It never ends itself: ending is a
//! record the application writes from what the client reports, the same
//! way a task is released.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::task::{AgentId, now_unix};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Tenant {
    pub id: AgentId,
    /// The harness the tenant runs — the fact that tells two tenants of one
    /// directory apart, since neither has work of its own to be named by.
    pub harness: String,
    /// The space's root, canonical: the directory the tenant works in and
    /// the key of the store that records it.
    pub root: PathBuf,
    pub created_at_unix: u64,
    /// When no live pane carried the tenant any more. `None` while live.
    pub ended_at_unix: Option<u64>,
}

impl Tenant {
    pub fn new(harness: &str, root: &Path) -> Self {
        Self {
            id: AgentId::generate(),
            harness: harness.to_owned(),
            root: root.to_path_buf(),
            created_at_unix: now_unix(),
            ended_at_unix: None,
        }
    }

    pub fn is_live(&self) -> bool {
        self.ended_at_unix.is_none()
    }

    /// Records that the tenant's last pane is gone. Idempotent: the first
    /// ending stands.
    pub fn end(&mut self) {
        if self.ended_at_unix.is_none() {
            self.ended_at_unix = Some(now_unix());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_tenant_is_live_and_ends_once() {
        let mut tenant = Tenant::new("claude-code", Path::new("/work/project"));
        assert!(tenant.is_live());
        assert_eq!(tenant.harness, "claude-code");
        tenant.end();
        let ended = tenant.ended_at_unix;
        assert!(ended.is_some());
        tenant.end();
        assert_eq!(tenant.ended_at_unix, ended, "the first ending stands");
    }

    #[test]
    fn two_tenants_of_one_directory_are_two_identities() {
        let first = Tenant::new("claude-code", Path::new("/work/project"));
        let second = Tenant::new("claude-code", Path::new("/work/project"));
        assert_ne!(first.id, second.id);
    }
}
