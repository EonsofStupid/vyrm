use crate::{
    applied_contract_sha256, desired_resources, DesiredInstanceInput, DesiredResource,
    KubernetesResourceKind, RrdInstance, RrdInstancePhase, RrdInstanceStatus,
    RRD_INSTANCE_FINALIZER, RRD_OPERATOR_FIELD_MANAGER,
};
use futures::StreamExt;
use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::Service;
use k8s_openapi::api::networking::v1::NetworkPolicy;
use k8s_openapi::api::policy::v1::PodDisruptionBudget;
use kube::api::{DeleteParams, Patch, PatchParams};
use kube::core::NamespaceResourceScope;
use kube::runtime::controller::{Action, Controller};
use kube::runtime::finalizer::{finalizer, Event};
use kube::runtime::watcher;
use kube::{Api, Client, ResourceExt};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

pub type Result<T> = std::result::Result<T, ControllerError>;

#[derive(Debug, thiserror::Error)]
pub enum ControllerError {
    #[error("Kubernetes API failed: {0}")]
    Kube(#[from] kube::Error),
    #[error("RRD Kubernetes contract failed: {0}")]
    Contract(String),
    #[error("RRD Kubernetes finalizer failed: {0}")]
    Finalizer(String),
}

#[derive(Clone)]
struct Context {
    client: Client,
}

pub async fn run() -> Result<()> {
    let client = Client::try_default().await?;
    let instances = Api::<RrdInstance>::all(client.clone());
    let stateful_sets = Api::<StatefulSet>::all(client.clone());
    Controller::new(instances, watcher::Config::default())
        .owns(stateful_sets, watcher::Config::default())
        .run(
            reconcile,
            error_policy,
            Arc::new(Context {
                client: client.clone(),
            }),
        )
        .for_each(|result| async move {
            match result {
                Ok(object) => tracing::info!(?object, "RRD Kubernetes reconciliation completed"),
                Err(error) => {
                    tracing::error!(error = %error, "RRD Kubernetes reconciliation failed")
                }
            }
        })
        .await;
    Ok(())
}

async fn reconcile(instance: Arc<RrdInstance>, context: Arc<Context>) -> Result<Action> {
    let namespace = instance
        .namespace()
        .ok_or_else(|| ControllerError::Contract("RRD instance has no namespace".into()))?;
    let api = Api::<RrdInstance>::namespaced(context.client.clone(), &namespace);
    finalizer(&api, RRD_INSTANCE_FINALIZER, instance, |event| async {
        match event {
            Event::Apply(instance) => apply(instance, &context).await,
            Event::Cleanup(instance) => cleanup(instance, &context).await,
        }
    })
    .await
    .map_err(|error| ControllerError::Finalizer(error.to_string()))
}

async fn apply(instance: Arc<RrdInstance>, context: &Context) -> Result<Action> {
    let namespace = instance
        .namespace()
        .ok_or_else(|| ControllerError::Contract("RRD instance has no namespace".into()))?;
    let name = instance.name_any();
    let uid = instance
        .metadata
        .uid
        .as_deref()
        .ok_or_else(|| ControllerError::Contract("RRD instance has no UID".into()))?;
    let generation = instance.metadata.generation.unwrap_or_default();
    let input = DesiredInstanceInput {
        namespace: &namespace,
        name: &name,
        uid,
        generation,
        spec: &instance.spec,
    };
    let resources = match desired_resources(&input) {
        Ok(resources) => resources,
        Err(message) => {
            patch_status(
                context,
                &namespace,
                &name,
                RrdInstanceStatus {
                    observed_generation: generation,
                    phase: RrdInstancePhase::Blocked,
                    ready_replicas: 0,
                    endpoint: None,
                    applied_contract_sha256: String::new(),
                    message,
                },
            )
            .await?;
            return Ok(Action::await_change());
        }
    };
    let contract_sha256 = applied_contract_sha256(&resources);
    for resource in &resources {
        apply_resource(context.client.clone(), &namespace, resource).await?;
    }
    let stateful_sets = Api::<StatefulSet>::namespaced(context.client.clone(), &namespace);
    let stateful_set = stateful_sets.get(&name).await?;
    let ready_replicas = stateful_set
        .status
        .as_ref()
        .and_then(|status| status.ready_replicas)
        .unwrap_or_default();
    let ready = ready_replicas == 1;
    patch_status(
        context,
        &namespace,
        &name,
        RrdInstanceStatus {
            observed_generation: generation,
            phase: if ready {
                RrdInstancePhase::Ready
            } else {
                RrdInstancePhase::Reconciling
            },
            ready_replicas,
            endpoint: Some(format!("https://{name}.{namespace}.svc:9477")),
            applied_contract_sha256: contract_sha256,
            message: if ready {
                "secured single-node RRD is ready".into()
            } else {
                "waiting for the secured RRD pod".into()
            },
        },
    )
    .await?;
    Ok(Action::requeue(Duration::from_secs(30)))
}

async fn cleanup(instance: Arc<RrdInstance>, context: &Context) -> Result<Action> {
    let namespace = instance
        .namespace()
        .ok_or_else(|| ControllerError::Contract("RRD instance has no namespace".into()))?;
    let name = instance.name_any();
    let peer_name = format!("{name}-peer");
    delete::<NetworkPolicy>(context.client.clone(), &namespace, &name).await?;
    delete::<PodDisruptionBudget>(context.client.clone(), &namespace, &name).await?;
    delete::<StatefulSet>(context.client.clone(), &namespace, &name).await?;
    delete::<Service>(context.client.clone(), &namespace, &name).await?;
    delete::<Service>(context.client.clone(), &namespace, &peer_name).await?;
    Ok(Action::await_change())
}

async fn apply_resource(client: Client, namespace: &str, resource: &DesiredResource) -> Result<()> {
    let parameters = PatchParams::apply(RRD_OPERATOR_FIELD_MANAGER).force();
    match resource.kind {
        KubernetesResourceKind::HeadlessService | KubernetesResourceKind::ClientService => {
            Api::<Service>::namespaced(client, namespace)
                .patch(&resource.name, &parameters, &Patch::Apply(&resource.body))
                .await?;
        }
        KubernetesResourceKind::StatefulSet => {
            Api::<StatefulSet>::namespaced(client, namespace)
                .patch(&resource.name, &parameters, &Patch::Apply(&resource.body))
                .await?;
        }
        KubernetesResourceKind::PodDisruptionBudget => {
            Api::<PodDisruptionBudget>::namespaced(client, namespace)
                .patch(&resource.name, &parameters, &Patch::Apply(&resource.body))
                .await?;
        }
        KubernetesResourceKind::NetworkPolicy => {
            Api::<NetworkPolicy>::namespaced(client, namespace)
                .patch(&resource.name, &parameters, &Patch::Apply(&resource.body))
                .await?;
        }
    }
    Ok(())
}

async fn patch_status(
    context: &Context,
    namespace: &str,
    name: &str,
    status: RrdInstanceStatus,
) -> Result<()> {
    Api::<RrdInstance>::namespaced(context.client.clone(), namespace)
        .patch_status(
            name,
            &PatchParams::apply(RRD_OPERATOR_FIELD_MANAGER),
            &Patch::Merge(json!({"status": status})),
        )
        .await?;
    Ok(())
}

async fn delete<K>(client: Client, namespace: &str, name: &str) -> Result<()>
where
    K: Clone
        + serde::de::DeserializeOwned
        + std::fmt::Debug
        + kube::Resource<DynamicType = (), Scope = NamespaceResourceScope>
        + Send
        + Sync
        + 'static,
{
    let api = Api::<K>::namespaced(client, namespace);
    match api.delete(name, &DeleteParams::background()).await {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(error)) if error.code == 404 => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn error_policy(
    _instance: Arc<RrdInstance>,
    error: &ControllerError,
    _context: Arc<Context>,
) -> Action {
    tracing::warn!(error = %error, "RRD Kubernetes reconcile will retry");
    Action::requeue(Duration::from_secs(10))
}
