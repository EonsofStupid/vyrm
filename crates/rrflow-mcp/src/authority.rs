use crate::config::RuntimeConfig;
use rrd_client::{ClientConfig, RequestOptions, RrdClient, Session};
use rrd_contract::{
    runtime_tool_arguments_sha256, runtime_tool_invocation_sha256, CanonicalId, CapabilityStatus,
    CloseSession, CorrelationId, CreateSession, LifecycleEnforcementLevelV1, RequestContext,
    ResourceId, ResourceKind, ResourcePath, RuntimeToolCatalogue, RuntimeToolInvocation,
    SessionLimits, RUNTIME_TOOL_CATALOGUE_VERSION,
};
use rrd_engine::{
    runtime_tool_contract_catalogue, InstanceBinding, Invocation, InvocationCredential, RrdEngine,
};
use serde_json::Value;
use std::fs::File;
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_API_KEY_BYTES: u64 = 64 * 1024;
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(crate) enum RuntimeAuthority {
    Embedded {
        engine: Box<RrdEngine>,
        project_root: PathBuf,
        catalogue: RuntimeToolCatalogue,
        security_enforced: bool,
    },
    Daemon(Box<DaemonAuthority>),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct RuntimeAuthorityProfile {
    pub mode: &'static str,
    pub execution_authority: &'static str,
    pub storage_access: &'static str,
    pub caller_authentication: &'static str,
    pub security_enforced: bool,
    pub tool_policy_enforced: bool,
    pub enforcement_level: LifecycleEnforcementLevelV1,
    pub planning_enforced: bool,
    pub mutation_enforced: bool,
    pub host_tool_interception: bool,
}

pub(crate) struct DaemonAuthority {
    runtime: tokio::runtime::Runtime,
    client: RrdClient,
    session: Session,
    catalogue: RuntimeToolCatalogue,
    principal: CanonicalId,
    api_key: String,
}

impl RuntimeAuthority {
    pub(crate) fn open(config: RuntimeConfig) -> Result<Self, Box<dyn std::error::Error>> {
        match config {
            RuntimeConfig::Embedded {
                database,
                project_root,
            } => {
                let binding = InstanceBinding::discover(&project_root)?;
                binding.verify_store_path(&database)?;
                let engine = RrdEngine::open_bound(&binding)?;
                let security_enforced = engine.security_enforced()?;
                Ok(Self::Embedded {
                    engine: Box::new(engine),
                    project_root: binding.project_root,
                    catalogue: runtime_tool_contract_catalogue(),
                    security_enforced,
                })
            }
            RuntimeConfig::Daemon {
                url,
                instance,
                principal,
                api_key_file,
            } => {
                let address = parse_loopback_url(&url)?;
                let api_key = read_api_key(&api_key_file)?;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                let client = RrdClient::connect_local(address, instance, ClientConfig::default())?;
                let capabilities = runtime.block_on(client.capabilities())?;
                let security_enforced = capabilities.capabilities.iter().any(|capability| {
                    capability.name.as_str() == "security-policy"
                        && matches!(
                            capability.status,
                            CapabilityStatus::Experimental | CapabilityStatus::Available
                        )
                });
                if !security_enforced {
                    return Err(
                        "daemon MCP requires an initialized RRD security policy; unsecured development sessions are not authenticated"
                            .into(),
                    );
                }
                let session = runtime.block_on(client.create_session(
                    principal.clone(),
                    &api_key,
                    CreateSession {
                        limits: SessionLimits {
                            idle_timeout_ms: 15 * 60 * 1_000,
                            absolute_timeout_ms: 60 * 60 * 1_000,
                            max_open_transactions: 8,
                        },
                    },
                    request_options(true)?,
                ))?;
                let catalogue = runtime
                    .block_on(client.runtime_tool_catalogue(&session, request_options(false)?))?;
                if catalogue != runtime_tool_contract_catalogue() {
                    return Err("daemon runtime-tool catalogue differs from this MCP build".into());
                }
                Ok(Self::Daemon(Box::new(DaemonAuthority {
                    runtime,
                    client,
                    session,
                    catalogue,
                    principal,
                    api_key,
                })))
            }
        }
    }

    pub(crate) fn catalogue(&self) -> &RuntimeToolCatalogue {
        match self {
            Self::Embedded { catalogue, .. } => catalogue,
            Self::Daemon(authority) => &authority.catalogue,
        }
    }

    pub(crate) fn profile(&self) -> RuntimeAuthorityProfile {
        let (mode, execution_authority, storage_access, caller_authentication, security_enforced) =
            match self {
                Self::Embedded {
                    security_enforced, ..
                } => (
                    "embedded",
                    "embedded_rrd_engine",
                    "exclusive_bound_engine",
                    "none",
                    *security_enforced,
                ),
                Self::Daemon(_) => (
                    "daemon",
                    "rrd_server_via_rrd_client",
                    "none",
                    "principal_api_key_session",
                    true,
                ),
            };
        RuntimeAuthorityProfile {
            mode,
            execution_authority,
            storage_access,
            caller_authentication,
            security_enforced,
            tool_policy_enforced: true,
            enforcement_level: LifecycleEnforcementLevelV1::Cooperative,
            planning_enforced: false,
            mutation_enforced: false,
            host_tool_interception: false,
        }
    }

    pub(crate) fn call(&mut self, name: &str, arguments: &Value) -> Result<String, String> {
        let tool = CanonicalId::new(name).map_err(|error| error.to_string())?;
        let descriptor = self
            .catalogue()
            .tools
            .iter()
            .find(|descriptor| descriptor.name == tool)
            .ok_or_else(|| format!("unknown tool {name:?}"))?;
        let mutation = descriptor.mutation;
        match self {
            Self::Embedded {
                engine,
                project_root,
                ..
            } => invoke_embedded(engine, project_root, tool, arguments, mutation),
            Self::Daemon(authority) => {
                let options = request_options(mutation).map_err(|error| error.to_string())?;
                let first = authority
                    .runtime
                    .block_on(authority.client.invoke_runtime_tool(
                        &authority.session,
                        &authority.catalogue,
                        &tool,
                        arguments.clone(),
                        options.clone(),
                    ));
                let result = match first {
                    Err(error) if rrd_client::is_unauthenticated(&error) => {
                        authority
                            .refresh_session()
                            .map_err(|error| error.to_string())?;
                        authority
                            .runtime
                            .block_on(authority.client.invoke_runtime_tool(
                                &authority.session,
                                &authority.catalogue,
                                &tool,
                                arguments.clone(),
                                options,
                            ))
                    }
                    result => result,
                };
                result
                    .map(|result| result.content)
                    .map_err(|error| error.to_string())
            }
        }
    }

    pub(crate) fn close(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let Self::Daemon(authority) = self else {
            return Ok(());
        };
        authority.runtime.block_on(authority.client.close_session(
            &authority.session,
            CloseSession {},
            request_options(true)?,
        ))?;
        Ok(())
    }
}

impl DaemonAuthority {
    fn refresh_session(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let session = self.runtime.block_on(self.client.create_session(
            self.principal.clone(),
            &self.api_key,
            CreateSession {
                limits: SessionLimits {
                    idle_timeout_ms: 15 * 60 * 1_000,
                    absolute_timeout_ms: 60 * 60 * 1_000,
                    max_open_transactions: 8,
                },
            },
            request_options(true)?,
        ))?;
        let catalogue = self.runtime.block_on(
            self.client
                .runtime_tool_catalogue(&session, request_options(false)?),
        )?;
        if catalogue != self.catalogue {
            return Err("daemon runtime-tool catalogue changed while MCP was connected".into());
        }
        self.session = session;
        Ok(())
    }
}

fn request_options(mutation: bool) -> rrd_client::Result<RequestOptions> {
    let coordinate = next_coordinate();
    if mutation {
        RequestOptions::mutation(
            &format!("request-{coordinate}"),
            &format!("operation-{coordinate}"),
            &format!("idempotency-{coordinate}"),
        )
    } else {
        RequestOptions::read(
            &format!("request-{coordinate}"),
            &format!("operation-{coordinate}"),
        )
    }
}

fn invoke_embedded(
    engine: &RrdEngine,
    project_root: &Path,
    tool: CanonicalId,
    arguments: &Value,
    mutation: bool,
) -> Result<String, String> {
    let observed_at_unix_ms = now();
    let coordinate = next_coordinate();
    let request = RuntimeToolInvocation {
        catalogue_version: RUNTIME_TOOL_CATALOGUE_VERSION,
        arguments_sha256: runtime_tool_arguments_sha256(arguments)
            .map_err(|error| error.to_string())?,
        tool,
        arguments: arguments.clone(),
    };
    let request_sha256 =
        runtime_tool_invocation_sha256(&request).map_err(|error| error.to_string())?;
    let context = RequestContext {
        request_id: CorrelationId::new(format!("request-{coordinate}"))
            .map_err(|error| error.to_string())?,
        operation_id: CorrelationId::new(format!("operation-{coordinate}"))
            .map_err(|error| error.to_string())?,
        idempotency_key: mutation
            .then(|| CorrelationId::new(format!("idempotency-{coordinate}")))
            .transpose()
            .map_err(|error| error.to_string())?,
        deadline_unix_ms: Some(observed_at_unix_ms.saturating_add(60_000)),
    };
    let resource = ResourcePath {
        segments: vec![
            ResourceId::new(ResourceKind::Instance, engine.instance_id().as_str())
                .map_err(|error| error.to_string())?,
        ],
    };
    engine
        .invoke_runtime_tool(
            project_root,
            &request,
            Invocation {
                context,
                resource,
                observed_at_unix_ms,
                attempt: 1,
                request_sha256,
            },
            InvocationCredential::Anonymous,
        )
        .map(|result| result.content)
        .map_err(|error| error.to_string())
}

fn next_coordinate() -> String {
    let sequence = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("rrflow-mcp-{}-{sequence}", now())
}

fn parse_loopback_url(url: &str) -> Result<SocketAddr, Box<dyn std::error::Error>> {
    let address: SocketAddr = url
        .strip_prefix("http://")
        .ok_or("daemon URL must use loopback http:// in the local profile")?
        .parse()?;
    if !address.ip().is_loopback() {
        return Err("daemon cleartext URL must resolve to a loopback address".into());
    }
    Ok(address)
}

fn read_api_key(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    if !path.is_absolute() {
        return Err("--api-key-file must be an absolute operator-owned path".into());
    }
    let resolved = std::fs::canonicalize(path)?;
    let metadata = std::fs::metadata(&resolved)?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > MAX_API_KEY_BYTES
    {
        return Err("API key source must be a non-empty bounded regular file".into());
    }
    ensure_private(&resolved, &metadata)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(resolved)?
        .take(MAX_API_KEY_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_API_KEY_BYTES || !bytes.iter().all(|byte| byte.is_ascii_graphic()) {
        return Err("API key must be bounded visible ASCII without trailing whitespace".into());
    }
    String::from_utf8(bytes).map_err(Into::into)
}

#[cfg(unix)]
fn ensure_private(path: &Path, metadata: &std::fs::Metadata) -> std::io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let mode = metadata.permissions().mode();
    let parent = std::fs::metadata(path.parent().unwrap_or(Path::new("/")))?;
    if mode & 0o077 != 0 || metadata.uid() != parent.uid() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "API key file must be owner-only and owned with its parent",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private(_path: &Path, _metadata: &std::fs::Metadata) -> std::io::Result<()> {
    Ok(())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .max(1) as u64
}
