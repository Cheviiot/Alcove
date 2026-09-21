// SPDX-License-Identifier: GPL-3.0-only
//! Engine-independent saved decisions; CEF only supplies the requested kinds.
use crate::domain::policy::{
    AppPolicyV2, Origin, PermissionDecision, PermissionKind, ProxyMode, ProxyPolicy,
};
use anyhow::{bail, Result};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

pub trait PolicyStore {
    fn permissions(
        &self,
        origin: &Origin,
        kinds: &[PermissionKind],
        decision: PermissionDecision,
    ) -> Result<AppPolicyV2>;
    fn allow_navigation(&self, origin: Origin) -> Result<AppPolicyV2>;
}

pub fn proxy_arguments(proxy: &ProxyPolicy) -> Result<Vec<String>> {
    Ok(match proxy.mode {
        ProxyMode::System => vec![],
        ProxyMode::NoProxy => vec!["--no-proxy-server".into()],
        ProxyMode::Custom => {
            let uri = proxy
                .uri
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("proxy URI missing"))?;
            let parsed = url::Url::parse(uri)?;
            if !matches!(parsed.scheme(), "http" | "https" | "socks4" | "socks5") {
                bail!("Use an explicit http, https, socks4 or socks5 proxy for Chromium");
            }
            vec![format!("--proxy-server={uri}")]
        }
    })
}

pub struct Policy {
    pub current: RefCell<AppPolicyV2>,
    session: RefCell<BTreeSet<(Origin, PermissionKind)>>,
    store: Option<Rc<dyn PolicyStore>>,
}

impl Policy {
    pub fn new(current: AppPolicyV2, store: Option<Rc<dyn PolicyStore>>) -> Rc<Self> {
        Rc::new(Self {
            current: RefCell::new(current),
            session: RefCell::default(),
            store,
        })
    }
    pub fn decision(&self, origin: &Origin, kinds: &[PermissionKind]) -> PermissionDecision {
        let current = self.current.borrow();
        if kinds.is_empty()
            || kinds
                .iter()
                .any(|k| current.decision(origin, *k) == PermissionDecision::Block)
        {
            return PermissionDecision::Block;
        }
        let session = self.session.borrow();
        if kinds.iter().all(|k| {
            current.decision(origin, *k) == PermissionDecision::Allow
                || session.contains(&(origin.clone(), *k))
        }) {
            PermissionDecision::Allow
        } else {
            PermissionDecision::Ask
        }
    }
    pub fn allow_session(&self, origin: &Origin, kinds: &[PermissionKind]) {
        self.session
            .borrow_mut()
            .extend(kinds.iter().map(|k| (origin.clone(), *k)));
    }
    pub fn save(
        &self,
        origin: &Origin,
        kinds: &[PermissionKind],
        decision: PermissionDecision,
    ) -> Result<()> {
        if let Some(store) = &self.store {
            self.current
                .replace(store.permissions(origin, kinds, decision)?);
        } else {
            for kind in kinds {
                self.current
                    .borrow_mut()
                    .set_decision(origin.clone(), *kind, decision);
            }
        }
        Ok(())
    }
    pub fn allow_navigation(&self, origin: Origin) -> Result<()> {
        if let Some(store) = &self.store {
            self.current.replace(store.allow_navigation(origin)?);
        } else {
            self.current
                .borrow_mut()
                .navigation
                .allowed_origins
                .insert(origin);
        }
        Ok(())
    }
}

pub fn permission_kinds(event: &Value) -> Result<Vec<PermissionKind>> {
    use PermissionKind::*;
    let mask = event["permissions"].as_u64().unwrap_or(0);
    let supported: &[(u64, PermissionKind)] = if event["media"] == true {
        &[(1, Microphone), (2, Camera)]
    } else {
        &[
            (1 << 2, Camera),
            (1 << 4, Clipboard),
            (1 << 5, ThirdPartyStorage),
            (1 << 8, Geolocation),
            (1 << 12, Microphone),
            (1 << 15, Notifications),
            (1 << 17, PointerLock),
            (1 << 20, ThirdPartyStorage),
        ]
    };
    let mut known = 0;
    let mut kinds = BTreeSet::new();
    for &(flag, kind) in supported {
        if mask & flag != 0 {
            known |= flag;
            kinds.insert(kind);
        }
    }
    if mask == 0 || mask != known {
        bail!("unsupported Chromium permission request");
    }
    Ok(kinds.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn proxy_modes_are_explicit_and_unsupported_schemes_do_not_fall_back() {
        assert!(proxy_arguments(&ProxyPolicy::default()).unwrap().is_empty());
        assert_eq!(
            proxy_arguments(&ProxyPolicy {
                mode: ProxyMode::NoProxy,
                uri: None
            })
            .unwrap(),
            ["--no-proxy-server"]
        );
        assert_eq!(
            proxy_arguments(&ProxyPolicy {
                mode: ProxyMode::Custom,
                uri: Some("socks5://127.0.0.1:1080".into())
            })
            .unwrap(),
            ["--proxy-server=socks5://127.0.0.1:1080"]
        );
        assert!(proxy_arguments(&ProxyPolicy {
            mode: ProxyMode::Custom,
            uri: Some("socks4a://127.0.0.1:1080".into())
        })
        .is_err());
    }
    #[test]
    fn combined_requests_require_every_kind_and_block_overrides_session() {
        let origin: Origin = "https://example.org".parse().unwrap();
        let policy = Policy::new(AppPolicyV2::default(), None);
        let kinds = permission_kinds(&json!({"permissions":3,"media":true})).unwrap();
        policy.allow_session(&origin, &[PermissionKind::Camera]);
        assert_eq!(policy.decision(&origin, &kinds), PermissionDecision::Ask);
        policy.allow_session(&origin, &kinds);
        assert_eq!(policy.decision(&origin, &kinds), PermissionDecision::Allow);
        policy
            .save(
                &origin,
                &[PermissionKind::Microphone],
                PermissionDecision::Block,
            )
            .unwrap();
        assert_eq!(policy.decision(&origin, &kinds), PermissionDecision::Block);
        assert!(permission_kinds(&json!({"permissions":5,"media":true})).is_err());
        assert!(permission_kinds(&json!({"permissions":1 << 7})).is_err());
        assert_eq!(policy.decision(&origin, &[]), PermissionDecision::Block);
    }
}
