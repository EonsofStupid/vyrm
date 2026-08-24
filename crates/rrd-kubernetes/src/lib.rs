//! Kubernetes-native deployment contract for a secured RRD instance.
//!
//! The first version intentionally deploys one durable RRD process. The
//! separate `vyrm-cluster` consensus engine is not yet wired into the public
//! RRD data plane, so this crate refuses to represent multiple independent RRD
//! pods as a distributed database.

use kube::{CustomResource, CustomResourceExt};
use rrd_contract::CanonicalId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use vyrm_core::digest;

pub mod controller;

pub const RRD_INSTANCE_API_VERSION: &str = "rrflow.io/v1alpha1";
pub const RRD_INSTANCE_KIND: &str = "RrdInstance";
pub const RRD_INSTANCE_FINALIZER: &str = "rrflow.io/rrd-instance-protection";
pub const RRD_OPERATOR_FIELD_MANAGER: &str = "rrd-kubernetes-operator";
pub const RRD_CLIENT_PORT: u16 = 9477;
pub const RRD_KUBERNETES_CONTRACT_VERSION: u16 = 1;

#[derive(CustomResource, Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "rrflow.io",
    version = "v1alpha1",
    kind = "RrdInstance",
    plural = "rrdinstances",
    namespaced,
    status = "RrdInstanceStatus",
    shortname = "rrd"
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RrdInstanceSpec {
    pub contract_version: u16,
    pub instance_id: CanonicalId,
    /// Immutable image reference. Tags are rejected; a sha256 digest is required.
    pub image: String,
    pub storage: RrdStorageSpec,
    pub tls_secret: String,
    pub bootstrap_manifest_config_map: String,
    pub bootstrap_credential_secret: String,
    pub bootstrap_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RrdStorageSpec {
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_class_name: Option<String>,
    #[serde(default = "default_true")]
    pub retain_on_delete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum RrdInstancePhase {
    #[default]
    Pending,
    Reconciling,
    Ready,
    Degraded,
    Deleting,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RrdInstanceStatus {
    pub observed_generation: i64,
    pub phase: RrdInstancePhase,
    pub ready_replicas: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    pub applied_contract_sha256: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum KubernetesResourceKind {
    HeadlessService,
    ClientService,
    StatefulSet,
    PodDisruptionBudget,
    NetworkPolicy,
}

impl KubernetesResourceKind {
    pub const fn api_kind(self) -> &'static str {
        match self {
            Self::HeadlessService | Self::ClientService => "Service",
            Self::StatefulSet => "StatefulSet",
            Self::PodDisruptionBudget => "PodDisruptionBudget",
            Self::NetworkPolicy => "NetworkPolicy",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesiredResource {
    pub kind: KubernetesResourceKind,
    pub name: String,
    pub body: Value,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesiredInstanceInput<'a> {
    pub namespace: &'a str,
    pub name: &'a str,
    pub uid: &'a str,
    pub generation: i64,
    pub spec: &'a RrdInstanceSpec,
}

impl RrdInstanceSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.contract_version != RRD_KUBERNETES_CONTRACT_VERSION {
            return Err("unsupported RRD Kubernetes contract version".into());
        }
        validate_image(&self.image)?;
        validate_dns_name(&self.tls_secret, "tls_secret")?;
        validate_dns_name(
            &self.bootstrap_manifest_config_map,
            "bootstrap_manifest_config_map",
        )?;
        validate_dns_name(
            &self.bootstrap_credential_secret,
            "bootstrap_credential_secret",
        )?;
        if self.bootstrap_at_unix_ms == 0 {
            return Err("bootstrap_at_unix_ms must be greater than zero".into());
        }
        self.storage.validate()
    }
}

impl RrdStorageSpec {
    fn validate(&self) -> Result<(), String> {
        if !self.retain_on_delete {
            return Err("alpha Kubernetes storage must retain PVCs on deletion".into());
        }
        validate_quantity(&self.size)?;
        if let Some(class) = &self.storage_class_name {
            validate_dns_name(class, "storage_class_name")?;
        }
        Ok(())
    }
}

pub fn desired_resources(input: &DesiredInstanceInput<'_>) -> Result<Vec<DesiredResource>, String> {
    input.spec.validate()?;
    validate_dns_name(input.namespace, "namespace")?;
    validate_dns_name(input.name, "metadata.name")?;
    if input.uid.is_empty() || input.generation <= 0 {
        return Err("metadata UID and positive generation are required".into());
    }
    let labels = labels(input.name);
    let owner_references = vec![json!({
        "apiVersion": RRD_INSTANCE_API_VERSION,
        "kind": RRD_INSTANCE_KIND,
        "name": input.name,
        "uid": input.uid,
        "controller": true,
        "blockOwnerDeletion": true,
    })];
    let metadata = |name: &str| {
        json!({
            "name": name,
            "namespace": input.namespace,
            "labels": labels,
            "ownerReferences": owner_references,
        })
    };
    let peer_name = bounded_name(input.name, "peer")?;
    let data_claim_name = "rrd-data";
    let service_port = json!({
        "name": "https",
        "port": RRD_CLIENT_PORT,
        "protocol": "TCP",
        "targetPort": "https",
    });
    let headless = json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": metadata(&peer_name),
        "spec": {
            "clusterIP": "None",
            "publishNotReadyAddresses": true,
            "selector": labels,
            "ports": [service_port.clone()],
        },
    });
    let client = json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": metadata(input.name),
        "spec": {
            "type": "ClusterIP",
            "selector": labels,
            "ports": [service_port],
        },
    });

    let mut claim = json!({
        "metadata": {"name": data_claim_name},
        "spec": {
            "accessModes": ["ReadWriteOnce"],
            "resources": {"requests": {"storage": input.spec.storage.size}},
        },
    });
    if let Some(class) = &input.spec.storage.storage_class_name {
        claim["spec"]["storageClassName"] = Value::String(class.clone());
    }
    let container_security = json!({
        "allowPrivilegeEscalation": false,
        "capabilities": {"drop": ["ALL"]},
        "readOnlyRootFilesystem": true,
        "runAsNonRoot": true,
        "runAsUser": 65532,
        "runAsGroup": 65532,
    });
    let pod_security = json!({
        "runAsNonRoot": true,
        "runAsUser": 65532,
        "runAsGroup": 65532,
        "fsGroup": 65532,
        "fsGroupChangePolicy": "OnRootMismatch",
        "seccompProfile": {"type": "RuntimeDefault"},
    });
    let stateful_set = json!({
        "apiVersion": "apps/v1",
        "kind": "StatefulSet",
        "metadata": metadata(input.name),
        "spec": {
            "serviceName": peer_name,
            "replicas": 1,
            "podManagementPolicy": "OrderedReady",
            "persistentVolumeClaimRetentionPolicy": {
                "whenDeleted": "Retain",
                "whenScaled": "Retain",
            },
            "updateStrategy": {"type": "OnDelete"},
            "selector": {"matchLabels": labels},
            "template": {
                "metadata": {"labels": labels},
                "spec": {
                    "automountServiceAccountToken": false,
                    "terminationGracePeriodSeconds": 30,
                    "securityContext": pod_security,
                    "initContainers": [{
                        "name": "security-bootstrap",
                        "image": input.spec.image,
                        "imagePullPolicy": "IfNotPresent",
                        "command": ["rrd-security-bootstrap"],
                        "args": [
                            "--db", "/var/lib/rrd/data",
                            "--instance", input.spec.instance_id.as_str(),
                            "--manifest", "/etc/rrd/bootstrap/manifest/bootstrap.json",
                            "--at-unix-ms", input.spec.bootstrap_at_unix_ms.to_string(),
                        ],
                        "securityContext": container_security,
                        "volumeMounts": [
                            {"name": data_claim_name, "mountPath": "/var/lib/rrd"},
                            {"name": "bootstrap-manifest", "mountPath": "/etc/rrd/bootstrap/manifest", "readOnly": true},
                            {"name": "bootstrap-credentials", "mountPath": "/etc/rrd/bootstrap/credentials", "readOnly": true},
                        ],
                    }],
                    "containers": [{
                        "name": "rrd",
                        "image": input.spec.image,
                        "imagePullPolicy": "IfNotPresent",
                        "command": ["rrd-server"],
                        "args": [
                            "--db", "/var/lib/rrd/data",
                            "--instance", input.spec.instance_id.as_str(),
                            "--bind", format!("0.0.0.0:{RRD_CLIENT_PORT}"),
                            "--tls-cert", "/etc/rrd/tls/tls.crt",
                            "--tls-key", "/etc/rrd/tls/tls.key",
                            "--tls-client-ca", "/etc/rrd/tls/ca.crt",
                        ],
                        "ports": [{"name": "https", "containerPort": RRD_CLIENT_PORT, "protocol": "TCP"}],
                        "startupProbe": {"tcpSocket": {"port": "https"}, "failureThreshold": 30, "periodSeconds": 2},
                        "livenessProbe": {"tcpSocket": {"port": "https"}, "failureThreshold": 3, "periodSeconds": 10},
                        "readinessProbe": {"tcpSocket": {"port": "https"}, "failureThreshold": 2, "periodSeconds": 5},
                        "resources": {
                            "requests": {"cpu": "250m", "memory": "512Mi"},
                            "limits": {"cpu": "2", "memory": "4Gi"},
                        },
                        "securityContext": container_security,
                        "volumeMounts": [
                            {"name": data_claim_name, "mountPath": "/var/lib/rrd"},
                            {"name": "rrd-tls", "mountPath": "/etc/rrd/tls", "readOnly": true},
                        ],
                    }],
                    "volumes": [
                        {"name": "rrd-tls", "secret": {"secretName": input.spec.tls_secret, "defaultMode": 288}},
                        {"name": "bootstrap-manifest", "configMap": {"name": input.spec.bootstrap_manifest_config_map, "defaultMode": 292}},
                        {"name": "bootstrap-credentials", "secret": {"secretName": input.spec.bootstrap_credential_secret, "defaultMode": 288}},
                    ],
                },
            },
            "volumeClaimTemplates": [claim],
        },
    });
    let disruption_budget = json!({
        "apiVersion": "policy/v1",
        "kind": "PodDisruptionBudget",
        "metadata": metadata(input.name),
        "spec": {
            "maxUnavailable": 0,
            "unhealthyPodEvictionPolicy": "AlwaysAllow",
            "selector": {"matchLabels": labels},
        },
    });
    let network_policy = json!({
        "apiVersion": "networking.k8s.io/v1",
        "kind": "NetworkPolicy",
        "metadata": metadata(input.name),
        "spec": {
            "podSelector": {"matchLabels": labels},
            "policyTypes": ["Ingress", "Egress"],
            "ingress": [{
                "from": [{"namespaceSelector": {"matchLabels": {"rrflow.io/rrd-client": "true"}}}],
                "ports": [{"protocol": "TCP", "port": RRD_CLIENT_PORT}],
            }],
            "egress": [],
        },
    });

    Ok(vec![
        resource(KubernetesResourceKind::HeadlessService, peer_name, headless)?,
        resource(
            KubernetesResourceKind::ClientService,
            input.name.into(),
            client,
        )?,
        resource(
            KubernetesResourceKind::StatefulSet,
            input.name.into(),
            stateful_set,
        )?,
        resource(
            KubernetesResourceKind::PodDisruptionBudget,
            input.name.into(),
            disruption_budget,
        )?,
        resource(
            KubernetesResourceKind::NetworkPolicy,
            input.name.into(),
            network_policy,
        )?,
    ])
}

pub fn applied_contract_sha256(resources: &[DesiredResource]) -> String {
    let identities = resources
        .iter()
        .map(|resource| (&resource.kind, &resource.name, &resource.sha256))
        .collect::<Vec<_>>();
    digest::sha256_hex(&serde_json::to_vec(&identities).expect("desired identities serialize"))
}

/// Generated structural CRD plus API-server CEL admission rules that mirror
/// the controller's fail-closed validation for deployment-critical fields.
pub fn crd_document() -> Value {
    let mut document = serde_json::to_value(RrdInstance::crd()).expect("RRD CRD serializes");
    let spec =
        &mut document["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"];
    spec["x-kubernetes-validations"] = json!([
        {"rule": "self.contractVersion == 1", "message": "contractVersion must be 1"},
        {"rule": "self.bootstrapAtUnixMs > 0", "message": "bootstrapAtUnixMs must be positive"},
        {"rule": "self.storage.retainOnDelete == true", "message": "PVC retention is mandatory in v1alpha1"},
        {"rule": "self.image.matches('^.+@sha256:[0-9a-f]{64}$')", "message": "image must be pinned by canonical sha256 digest"},
        {"rule": "self.storage.size.matches('^[1-9][0-9]*(Ki|Mi|Gi|Ti)$')", "message": "storage size must be a positive binary quantity"},
        {"rule": "self.tlsSecret.matches('^[a-z0-9]([-a-z0-9]*[a-z0-9])?$')", "message": "tlsSecret must be one DNS label"},
        {"rule": "self.bootstrapManifestConfigMap.matches('^[a-z0-9]([-a-z0-9]*[a-z0-9])?$')", "message": "bootstrapManifestConfigMap must be one DNS label"},
        {"rule": "self.bootstrapCredentialSecret.matches('^[a-z0-9]([-a-z0-9]*[a-z0-9])?$')", "message": "bootstrapCredentialSecret must be one DNS label"}
    ]);
    for property in [
        "tlsSecret",
        "bootstrapManifestConfigMap",
        "bootstrapCredentialSecret",
    ] {
        spec["properties"][property]["minLength"] = json!(1);
        spec["properties"][property]["maxLength"] = json!(63);
    }
    document
}

fn resource(
    kind: KubernetesResourceKind,
    name: String,
    body: Value,
) -> Result<DesiredResource, String> {
    let encoded = serde_json::to_vec(&body).map_err(|error| error.to_string())?;
    Ok(DesiredResource {
        kind,
        name,
        sha256: digest::sha256_hex(&encoded),
        body,
    })
}

fn labels(name: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("app.kubernetes.io/name".into(), "rrd".into()),
        ("app.kubernetes.io/instance".into(), name.into()),
        (
            "app.kubernetes.io/managed-by".into(),
            RRD_OPERATOR_FIELD_MANAGER.into(),
        ),
    ])
}

fn bounded_name(name: &str, suffix: &str) -> Result<String, String> {
    let combined = format!("{name}-{suffix}");
    if combined.len() > 63 {
        return Err(format!("resource name {combined:?} exceeds 63 bytes"));
    }
    Ok(combined)
}

fn validate_dns_name(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 63
        || !value.is_ascii()
        || value.starts_with('-')
        || value.ends_with('-')
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'))
    {
        return Err(format!("{field} must be one lowercase DNS label"));
    }
    Ok(())
}

fn validate_quantity(value: &str) -> Result<(), String> {
    let number = ["Ki", "Mi", "Gi", "Ti"]
        .into_iter()
        .find_map(|suffix| value.strip_suffix(suffix));
    if number.is_none_or(|number| {
        number.is_empty()
            || number.starts_with('0')
            || !number.bytes().all(|byte| byte.is_ascii_digit())
            || number.parse::<u64>().ok().is_none_or(|value| value == 0)
    }) {
        return Err("storage size must be a positive binary Kubernetes quantity".into());
    }
    Ok(())
}

fn validate_image(image: &str) -> Result<(), String> {
    let Some((repository, digest)) = image.rsplit_once("@sha256:") else {
        return Err("RRD image must be pinned by sha256 digest".into());
    };
    if repository.is_empty()
        || repository.len() > 512
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("RRD image repository or sha256 digest is invalid".into());
    }
    Ok(())
}

const fn default_true() -> bool {
    true
}
