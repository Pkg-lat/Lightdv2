//! Request identity and authorization.
//!
//! Before this module the daemon validated a single process-wide secret and
//! extracted no identity, so "who owns this container" had no answer anywhere and
//! every authenticated caller could act on every resource. A `Principal` carries
//! the caller's identity through each request so handlers can answer that question.

use crate::container::state::ContainerState;
use subtle::ConstantTimeEq;

/// Role held by an authenticated caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    /// Holds the master token from config. Sees and controls everything —
    /// this is the panel's own operator identity.
    Admin,
    /// Holds a token minted by an admin. Scoped to resources it owns.
    Tenant(String),
}

/// An authenticated caller.
#[derive(Debug, Clone)]
pub struct Principal {
    pub role: Role,
}

impl Principal {
    pub fn admin() -> Self {
        Self { role: Role::Admin }
    }

    pub fn tenant(owner: impl Into<String>) -> Self {
        Self {
            role: Role::Tenant(owner.into()),
        }
    }

    pub fn is_admin(&self) -> bool {
        matches!(self.role, Role::Admin)
    }

    /// The owner identifier for a tenant, or None for an admin.
    pub fn owner_id(&self) -> Option<&str> {
        match &self.role {
            Role::Admin => None,
            Role::Tenant(id) => Some(id.as_str()),
        }
    }

    /// May this principal act on the given container?
    ///
    /// Admins may act on anything. A tenant may act only on containers it owns.
    /// Containers with an empty owner (rows written before ownership existed) match
    /// no tenant, so they fall back to admin-only — fail-closed.
    pub fn can_access(&self, container: &ContainerState) -> bool {
        match &self.role {
            Role::Admin => true,
            Role::Tenant(id) => !container.owner.is_empty() && container.owner == *id,
        }
    }
}

/// Compare two secrets in constant time.
///
/// `ct_eq` requires equal lengths, so length is compared separately; that leaks
/// only the length of the expected token, not its contents.
pub fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container_owned_by(owner: &str) -> ContainerState {
        ContainerState::new(
            "id".to_string(),
            "vol".to_string(),
            "cmd".to_string(),
            owner.to_string(),
        )
    }

    #[test]
    fn admin_can_access_any_container() {
        let admin = Principal::admin();
        assert!(admin.can_access(&container_owned_by("alice")));
        assert!(admin.can_access(&container_owned_by("")));
    }

    #[test]
    fn tenant_can_access_own_container_only() {
        let alice = Principal::tenant("alice");
        assert!(alice.can_access(&container_owned_by("alice")));
        assert!(!alice.can_access(&container_owned_by("bob")));
    }

    #[test]
    fn unowned_container_is_admin_only() {
        // Rows predating the ownership model deserialize with an empty owner. A
        // tenant must not inherit access to them.
        let alice = Principal::tenant("alice");
        assert!(!alice.can_access(&container_owned_by("")));
    }

    #[test]
    fn secret_eq_matches_exact_only() {
        assert!(secret_eq("lightd_abc", "lightd_abc"));
        assert!(!secret_eq("lightd_abc", "lightd_abd"));
        assert!(!secret_eq("lightd_abc", "lightd_abc1"));
        assert!(!secret_eq("", "lightd_abc"));
    }
}