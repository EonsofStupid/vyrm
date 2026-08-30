# CI execution contract

RRFlow has one candidate CI chain. The thin caller is
`.github/workflows/ci.yml`; `.github/workflows/ci-reusable.yml` owns every
check. A branch push does not start a second copy of the same work.

The caller accepts pull requests, merge-queue candidates, and explicit manual
dispatches. Its concurrency key cancels stale executions for the same pull
request or ref. The reusable workflow reduces every partition into one stable
`ci-gate` check for branch protection:

- supervised RRD and Connectome topology smoke on the standard Linux runner;
- repository policy, formatting, architecture, evaluation, dependency, and
  binary-budget gates on a heavy Linux runner;
- five default-feature engine suites on isolated heavy Linux runners:
  `kernel-storage`, `query-retrieval`, `engine-authority`, `service-protocol`,
  and `product-operations`;
- isolated optional-feature qualification plus a real PostgreSQL/pgvector
  integration;
- daemon portability on GitHub-hosted Windows and macOS runners; and
- one always-evaluated gate which fails unless every partition succeeded.

The five engine suites cover every workspace package exactly once, but package
names do not define CI ownership. A failure is reported against the subsystem
whose behavior is being qualified. This keeps the hosted fallback below its
measured link/disk ceiling without turning all 22 implementation crates into
separate product checks. `scripts/ci/check_workflow.py` enforces complete suite
and optional-feature coverage, immutable dependencies, bounded jobs, safe
runner routing, and exact gate reduction.

Every job has a timeout. Every external action and service image is pinned by
an immutable digest or commit. Checkout credentials are not persisted.

## Runner trust boundary

The repository variables below contain JSON `runs-on` values:

| Variable | Physical pool value | Safe default |
| --- | --- | --- |
| `CI_SELF_HOSTED_ENABLED` | `true` | disabled |
| `CI_LINUX_STANDARD_RUNNER` | `["rrflow-standard"]` | `["ubuntu-latest"]` |
| `CI_LINUX_HEAVY_RUNNER` | `["rrflow-heavy"]` | `["ubuntu-latest"]` |

A pull request whose head repository differs from the base repository is
forced onto GitHub-hosted Linux even when the variables exist. Missing or
disabled `CI_SELF_HOSTED_ENABLED` also forces hosted Linux. The workflow does
not use `pull_request_target`. Same-repository branches, merge groups, and
manual proofs may use the private physical pool.

This routing expression is defense in depth, not permission to connect a
self-hosted pool to a public repository: a fork can propose a change to the
workflow itself. The installer queries GitHub and fails closed unless the
repository visibility is `PRIVATE`. Do not set the physical routing variables
or register the scale sets while the repository is public.

GitHub recommends ephemeral runners for autoscaling and identifies Actions
Runner Controller (ARC) as its reference Kubernetes implementation. The
checked-in deployment follows that model:

- ARC chart and controller image: `0.14.2`, digest pinned;
- `rrflow-heavy`: scale 0..1, 48 CPUs and 64 GiB total, isolated Docker daemon;
- `rrflow-standard`: scale 0..2, 16 CPUs and 20 GiB each; and
- maximum runner allocation: 80 CPUs and 104 GiB, leaving 8 CPUs and 24 GiB
  for the 88-CPU/128-GiB host, k3s, ARC, and filesystem cache.

Each job receives a fresh pod and an empty work volume. No runner worktree is
mounted from the host. The runner service account has no Kubernetes API
permissions, and only the heavy class has a privileged Docker sidecar.

References:

- [GitHub: self-hosted runners](https://docs.github.com/en/actions/reference/runners/self-hosted-runners)
- [GitHub: deploy ARC runner scale sets](https://docs.github.com/en/actions/how-tos/manage-runners/use-actions-runner-controller/deploy-runner-scale-sets)
- [GitHub: secure use of self-hosted runners](https://docs.github.com/en/actions/reference/security/secure-use)

## Physical-host bootstrap

Install a pinned single-node k3s `v1.36.3+k3s1` cluster on the physical host
and make its kubeconfig available to the administration machine. Do not share
this cluster with production workloads. Install Helm 4 and authenticate `gh`
as a repository administrator, then run:

```bash
KUBECONFIG=/path/to/physical-host.kubeconfig scripts/ci/install-arc.sh
```

The installer is idempotent. It first denies public-repository registration,
then creates separate controller and runner namespaces, streams the existing
GitHub token directly into a Kubernetes Secret, installs both pinned charts,
waits for the controller, installs the two scale sets, and sets the repository
routing variables. It does not write the token to argv or a values file.

Prove physical execution after installation:

```bash
gh workflow run ci.yml --repo EonsofStupid/rrflow \
  --ref agent/connectome-temporal-runtime-visualizer
gh run watch --repo EonsofStupid/rrflow --exit-status
```

The standard and heavy jobs must report `rrflow-standard` and `rrflow-heavy`
runner identities. Windows, macOS, the final gate, and all fork pull requests
must remain GitHub-hosted.

## Repository enforcement

After the first physical proof succeeds, protect `main` with these invariants:

- pull requests and the stable `pipeline / ci-gate` check are required;
- merge queue or an up-to-date branch is required;
- force pushes and deletion are denied; and
- Actions must require immutable full-SHA references.

The repository CI-policy check rejects trigger duplication, unsafe fork
routing, unpinned actions or runner images, missing timeouts, incomplete suite
or feature coverage, missing gate reduction, or a runner layout which exceeds
the documented host allocation. The Rust workspace architecture test remains
focused on engine ownership, dependency direction, and storage-opening
authority rather than GitHub Actions implementation details.
