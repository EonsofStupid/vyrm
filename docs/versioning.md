# RRFlow version policy

RRFlow's canonical current release-train version is `0.1.0`. The checked-in
`VERSION` file is the human- and automation-readable source of truth.

All Rust workspace crates inherit the same value from `[workspace.package]`.
The supported TypeScript, Python, Java, and .NET client packages use the exact
same release version. When the canonical Connectome repository is mounted at
`apps/connectome`, its JavaScript and Tauri package versions must match this
release train as well.

The product release version is independent of internal protocol, contract,
fixture, persisted-format, and schema identifiers such as `v1`. Those
identifiers describe compatibility domains; they do not announce a new RRFlow
or Connectome product release.

## Change control

- No agent, automation, dependency update, or generated-code task may change
  `VERSION` or a mirrored package version as a side effect.
- A version change requires the repository owner's explicit approval in the
  pull request that changes `VERSION`.
- The pull request must state the old version, new version, compatibility
  impact, migration requirements, and release/rollback plan.
- `scripts/check_version.py` and CI reject version drift. `.github/CODEOWNERS`
  requests owner review for every version authority and guard. Repository
  branch protection must require Code Owner review for that approval rule to
  be enforced by GitHub.

Until the canonical Connectome repository is mounted, this repository cannot
enforce Connectome's external manifests. They must not be silently rewritten
from a dirty or ambiguously named checkout.
