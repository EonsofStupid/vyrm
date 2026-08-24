# RRD Kubernetes operator v1alpha1

Status: executable controller and generated deployment contract; single-node
RRD only, not production or Multi-AZ qualification.

This slice follows Kubernetes's
[operator pattern](https://kubernetes.io/docs/concepts/extend-kubernetes/operator/):
`RrdInstance.spec` is desired state, the controller continuously reconciles
owned resources, and `RrdInstance.status` reports observed generation, phase,
readiness, endpoint, and the applied contract digest. The CRD is installed out
of band, consistent with kube-rs guidance, rather than granting the controller
permission to rewrite its own API.

## Shipped artifacts

- `rrd-kubernetes`: kube-rs watch/controller process with relist recovery,
  owned-StatefulSet watches, finalizer cleanup, server-side apply, status patch,
  and bounded retry.
- `rrd-kubernetes-crd`: deterministic CRD generator.
- `deploy/kubernetes/rrdinstances.rrflow.io-crd.json`: checked generated
  namespaced structural CRD with status subresource and CEL admission rules.
- `deploy/kubernetes/operator-rbac.json`: namespace, least-privilege service
  account/role/binding, one-replica operator Deployment template, and PDB.
- `deploy/kubernetes/example-rrdinstance.json`: example desired instance.

The example image digest is an explicit placeholder. A release pipeline must
build and publish an image containing `rrd-server`, `rrd-security-bootstrap`,
and `rrd-kubernetes`, then replace both placeholder references. No unpinned tag
is admitted by `RrdInstance` validation.

## Instance contract

`rrflow.io/v1alpha1` currently accepts:

- exact contract version and canonical RRD instance identity;
- one digest-pinned image;
- retained RWO PVC size and optional storage class;
- a TLS Secret name containing `tls.crt`, `tls.key`, and `ca.crt`;
- a ConfigMap containing `bootstrap.json` and a Secret containing every
  absolute credential-file target referenced by that manifest;
- one stable bootstrap timestamp.

CEL rules reject the wrong contract version, unpinned images, zero timestamps,
non-retained storage, invalid Secret/ConfigMap names, and storage quantities
outside the conservative positive Ki/Mi/Gi/Ti grammar before reconciliation.
The Rust controller repeats all safety validation before Kubernetes I/O.

## Reconciled resources

One RRD custom resource deterministically owns:

1. a headless Service for stable StatefulSet identity;
2. a ClusterIP client Service on TCP 9477;
3. a one-replica StatefulSet with retained PVC, `OrderedReady`, operator-
   controlled `OnDelete` upgrades, security-bootstrap init container, TLS 1.3
   mTLS RRD container, non-root execution, read-only root filesystem, dropped
   capabilities, resource bounds, and no service-account token;
4. a `maxUnavailable: 0` PodDisruptionBudget;
5. a default-deny NetworkPolicy that admits 9477 only from namespaces labeled
   `rrflow.io/rrd-client=true` and denies application egress.

Kubernetes recommends StatefulSets for stable identity and storage, and notes
that PVC retention is intentionally safer than automatic deletion. A PDB
protects voluntary disruption but does not constrain StatefulSet rolling
updates, which is why v1alpha1 also uses `OnDelete` rather than claiming an
automatic safe database upgrade. See the primary
[StatefulSet](https://kubernetes.io/docs/concepts/workloads/controllers/statefulset/)
and [disruption](https://kubernetes.io/docs/concepts/workloads/pods/disruptions/)
contracts.

The finalizer deletes owned workload/network objects while the StatefulSet
retention policy preserves PVCs. This uses the Kubernetes
[finalizer contract](https://kubernetes.io/docs/concepts/overview/working-with-objects/finalizers/);
it is not yet a backup-before-delete or restore workflow.

## Honest boundary

This does not claim Multi-AZ RRD. `vyrm-cluster` has a real Raft baseline, but
the public RRD service is not yet backed by that replicated state machine.
Running three independent `rrd-server` pods would create three databases, not
one database, so v1alpha1 renders exactly one replica and exposes no replica
field.

Still open: a published multi-architecture image and SBOM/signature pipeline,
kind/Kubernetes API-server conformance, certificate issuance/rotation, real
HTTPS readiness probes, backup/restore jobs and finalizer gates, safe upgrade
state machine, CSI snapshots, operator leader election/HA, telemetry, RRD-to-
Raft integration, topology/zone placement, and destructive Multi-AZ fault
qualification.
