//! The harness registry and its drift alarm. `PLAN.md` Step P.
//!
//! One TOML per harness adapter, embedded and parsed at load: the registry is
//! data, and a new harness is a file, not a fork. Each adapter declares its
//! integration surface (hooks? MCP? which context file?) and the billing
//! modes its providers sell, because harness x provider x billing are
//! orthogonal axes and pricing reads from the effectiveness ledger per
//! configuration.
//!
//! **The alarm is `resolve_as_of`, not a scheduler.** An adapter's
//! verification is a bi-temporal claim with `valid_to` set 21 days out.
//! Preflight resolves it at session start; an expired verification simply
//! stops resolving, and the warning surfaces in the injected context. This
//! space kills its own members — Gemini CLI shipped in this registry only as
//! a closed interval — so an adapter list without an expiry is wrong within
//! a quarter.

use serde::Deserialize;
use vyrm_core::{Claim, ClaimReader, Millis, Predicate, Producer, Subject};
use vyrm_store::Engine;

/// How long a verification claim stays in force: 21 days, in milliseconds.
/// Expiry is the alarm interval the operator asked for ("make noise every
/// few weeks").
pub const VERIFICATION_TTL_MS: Millis = 21 * 24 * 60 * 60 * 1000;

/// Predicate carried by verification claims.
pub const VERIFIED_PREDICATE: &str = "integration-verified";

#[derive(Debug, Clone, Deserialize)]
pub struct Harness {
    pub name: String,
    pub display: String,
    /// Full hook lifecycle: pre-reasoning injection and tool gating.
    pub hooks: bool,
    pub mcp_client: bool,
    pub mcp_server: bool,
    /// Convention file the harness reads natively.
    pub context_file: String,
    #[serde(default)]
    pub config_path: Option<String>,
    /// Billing modes the harness's providers sell: "subscription" and/or
    /// "per_usage".
    pub billing: Vec<String>,
    pub providers: Vec<String>,
    pub notes: String,
    /// Set when the harness no longer exists. A retired harness stays in the
    /// registry as history and refuses `init`.
    #[serde(default)]
    pub retired: Option<String>,
}

impl Harness {
    /// Claim subject under which this adapter's verification is recorded.
    pub fn subject(&self) -> Subject {
        Subject::new(format!("harness-{}", self.name))
            .expect("registry names are valid identifiers")
    }

    /// What the runtime cannot offer on this harness, stated rather than
    /// silent.
    pub fn degradations(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.hooks {
            return out;
        }
        out.push(format!(
            "{}: no hook lifecycle — recall is on-demand (MCP via vyrmd), not injected before reasoning",
            self.name
        ));
        if !self.mcp_client {
            out.push(format!("{}: no MCP client — CLI-only integration", self.name));
        }
        out
    }
}

pub struct Registry {
    harnesses: Vec<Harness>,
}

/// Verification state of one adapter at an instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    /// A verification claim is in force.
    Current { until: Option<Millis> },
    /// The last verification's interval has passed.
    Expired { days: u64 },
    /// Never audited.
    Never,
}

impl Registry {
    /// The embedded registry. A parse failure here is a build defect, caught
    /// by the test below, so loading does not return `Result`.
    pub fn builtin() -> Registry {
        let harnesses = [
            include_str!("../registry/claude-code.toml"),
            include_str!("../registry/codex-cli.toml"),
            include_str!("../registry/opencode.toml"),
            include_str!("../registry/grok-cli.toml"),
            include_str!("../registry/kimi-cli.toml"),
            include_str!("../registry/zcode.toml"),
            include_str!("../registry/gemini-cli.toml"),
        ]
        .into_iter()
        .map(|raw| toml::from_str(raw).expect("embedded registry row parses"))
        .collect();
        Registry { harnesses }
    }

    pub fn all(&self) -> &[Harness] {
        &self.harnesses
    }

    pub fn get(&self, name: &str) -> Option<&Harness> {
        self.harnesses.iter().find(|h| h.name == name)
    }

    /// Records a verification for `name`: a claim valid for
    /// [`VERIFICATION_TTL_MS`] from `now`, superseding the previous one. The
    /// evidence names what was checked (version, doc, endpoint), because an
    /// audit without evidence is an assertion.
    pub fn record_verification<E: Engine>(
        &self,
        store: &E,
        name: &str,
        now: Millis,
        evidence: &str,
        actor: &str,
    ) -> Result<Claim, vyrm_store::Error> {
        let harness = self
            .get(name)
            .ok_or_else(|| vyrm_store::Error::Substrate(format!("no harness named {name:?} in the registry")))?;
        let mut claim = Claim::new(
            harness.subject(),
            Predicate::new(VERIFIED_PREDICATE).expect("static predicate"),
            evidence,
            now,
            now,
            Producer { actor: actor.to_owned(), on_behalf_of: None, session: None },
        );
        claim.valid_to = Some(now + VERIFICATION_TTL_MS);
        store.assert(&claim)?;
        Ok(claim)
    }

    /// Verification state at `now`, resolved bi-temporally: current means a
    /// claim is in force, expired means the newest interval has closed.
    pub fn verification<E: Engine>(
        &self,
        store: &E,
        harness: &Harness,
        now: Millis,
    ) -> Result<Verification, vyrm_store::Error> {
        let subject = harness.subject();
        let predicate = Predicate::new(VERIFIED_PREDICATE).expect("static predicate");
        if let Some(claim) = store.as_of(&subject, &predicate, now)? {
            return Ok(Verification::Current { until: claim.valid_to });
        }
        let history = store.history(&subject, &predicate)?;
        match history.iter().filter_map(|c| c.valid_to).max() {
            Some(closed) if closed <= now => Ok(Verification::Expired {
                days: (now - closed) / (24 * 60 * 60 * 1000),
            }),
            _ => Ok(Verification::Never),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_row_parses_and_axes_are_populated() {
        let registry = Registry::builtin();
        assert_eq!(registry.all().len(), 7);
        for harness in registry.all() {
            assert!(!harness.billing.is_empty(), "{} declares no billing mode", harness.name);
            assert!(!harness.providers.is_empty(), "{} declares no provider", harness.name);
            harness.subject(); // must be a valid identifier
        }
        assert!(
            registry.get("gemini-cli").unwrap().retired.is_some(),
            "the closed interval is the registry's first bi-temporal fact"
        );
        assert!(registry.get("claude-code").unwrap().hooks, "the fast path has hooks");
    }
}
