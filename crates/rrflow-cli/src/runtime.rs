use crate::command::{Execution, RuntimeAction, RuntimeAuthorityArgs, RuntimeMode};
use rrd_client::{ClientConfig, RequestOptions, RrdClient, Session};
use rrd_contract::{
    runtime_tool_arguments_sha256, runtime_tool_invocation_sha256, CanonicalId, CapabilityStatus,
    CloseSession, CorrelationId, CreateSession, RequestContext, ResourceId, ResourceKind,
    ResourcePath, RuntimeToolCatalogue, RuntimeToolInvocation, SessionLimits,
    RUNTIME_TOOL_CATALOGUE_VERSION,
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

const MAX_API_KEY_BYTES: u64 = 64 * 1024;
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

enum RuntimeAuthority {
    Embedded {
        engine: Box<RrdEngine>,
        project_root: PathBuf,
        catalogue: RuntimeToolCatalogue,
    },
    Daemon(Box<DaemonAuthority>),
}

struct DaemonAuthority {
    runtime: tokio::runtime::Runtime,
    client: RrdClient,
    session: Session,
    catalogue: RuntimeToolCatalogue,
}

pub(crate) fn execute(
    database: Option<&Path>,
    options: &RuntimeAuthorityArgs,
    action: &RuntimeAction,
    now: u64,
    json: bool,
) -> Result<Execution, Box<dyn std::error::Error>> {
    let mut authority = RuntimeAuthority::open(database, options)?;
    let result = (|| -> Result<Execution, Box<dyn std::error::Error>> {
        match action {
            RuntimeAction::List => {
                let catalogue = authority.catalogue();
                let text = if json {
                    serde_json::to_string_pretty(catalogue)?
                } else {
                    catalogue
                        .tools
                        .iter()
                        .map(|tool| format!("{}\t{}", tool.name, tool.description))
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                Ok(text.into())
            }
            RuntimeAction::Call { tool, arguments } => {
                let arguments: Value = serde_json::from_str(arguments)?;
                if !arguments.is_object() {
                    return Err("--arguments must be a JSON object".into());
                }
                let content = authority.call(tool, arguments, now)?;
                let text = if json {
                    serde_json::to_string_pretty(&serde_json::json!({ "content": content }))?
                } else {
                    content
                };
                Ok(text.into())
            }
        }
    })();
    let close = authority.close();
    match (result, close) {
        (Ok(execution), Ok(())) => Ok(execution),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

impl RuntimeAuthority {
    fn open(
        database: Option<&Path>,
        options: &RuntimeAuthorityArgs,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        match options.mode {
            RuntimeMode::Embedded => {
                if options.url.is_some()
                    || options.instance.is_some()
                    || options.principal.is_some()
                    || options.api_key_file.is_some()
                {
                    return Err(
                        "embedded runtime mode does not accept daemon authority coordinates".into(),
                    );
                }
                let project_root = options.root.as_deref().unwrap_or_else(|| Path::new("."));
                let database = database.unwrap_or_else(|| Path::new(".rrflow/rrd"));
                let binding = InstanceBinding::discover(project_root)?;
                binding.verify_store_path(database)?;
                let engine = RrdEngine::open_bound(&binding)?;
                Ok(Self::Embedded {
                    engine: Box::new(engine),
                    project_root: binding.project_root,
                    catalogue: runtime_tool_contract_catalogue(),
                })
            }
            RuntimeMode::Daemon => {
                if database.is_some() || options.root.is_some() {
                    return Err("daemon runtime mode does not accept --db or --root".into());
                }
                let url = options
                    .url
                    .as_deref()
                    .ok_or("daemon runtime mode requires --url")?;
                let instance = CanonicalId::new(
                    options
                        .instance
                        .as_deref()
                        .ok_or("daemon runtime mode requires --instance")?,
                )?;
                let principal = CanonicalId::new(
                    options
                        .principal
                        .as_deref()
                        .ok_or("daemon runtime mode requires --principal")?,
                )?;
                let api_key_file = options
                    .api_key_file
                    .as_deref()
                    .ok_or("daemon runtime mode requires --api-key-file")?;
                let address = parse_loopback_url(url)?;
                let api_key = read_api_key(api_key_file)?;
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
                        "daemon CLI requires an initialized RRD security policy; unsecured development sessions are not authenticated"
                            .into(),
                    );
                }
                let session = runtime.block_on(client.create_session(
                    principal,
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
                    let _ = runtime.block_on(client.close_session(
                        &session,
                        CloseSession {},
                        request_options(true)?,
                    ));
                    return Err("daemon runtime-tool catalogue differs from this CLI build".into());
                }
                Ok(Self::Daemon(Box::new(DaemonAuthority {
                    runtime,
                    client,
                    session,
                    catalogue,
                })))
            }
        }
    }

    fn catalogue(&self) -> &RuntimeToolCatalogue {
        match self {
            Self::Embedded { catalogue, .. } => catalogue,
            Self::Daemon(authority) => &authority.catalogue,
        }
    }

    fn call(
        &mut self,
        name: &str,
        arguments: Value,
        now: u64,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let tool = CanonicalId::new(name)?;
        let mutation = self
            .catalogue()
            .tools
            .iter()
            .find(|descriptor| descriptor.name == tool)
            .ok_or_else(|| format!("unknown runtime tool {name:?}"))?
            .mutation;
        match self {
            Self::Embedded {
                engine,
                project_root,
                ..
            } => invoke_embedded(engine, project_root, tool, arguments, mutation, now)
                .map_err(Into::into),
            Self::Daemon(authority) => authority
                .runtime
                .block_on(authority.client.invoke_runtime_tool(
                    &authority.session,
                    &authority.catalogue,
                    &tool,
                    arguments,
                    request_options(mutation)?,
                ))
                .map(|result| result.content)
                .map_err(Into::into),
        }
    }

    fn close(&mut self) -> Result<(), Box<dyn std::error::Error>> {
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
    arguments: Value,
    mutation: bool,
    now: u64,
) -> rrd_engine::Result<String> {
    let coordinate = next_coordinate();
    let request = RuntimeToolInvocation {
        catalogue_version: RUNTIME_TOOL_CATALOGUE_VERSION,
        arguments_sha256: runtime_tool_arguments_sha256(&arguments)
            .map_err(|error| rrd_engine::ServiceError::Contract(error.to_string()))?,
        tool,
        arguments,
    };
    let request_sha256 = runtime_tool_invocation_sha256(&request)
        .map_err(|error| rrd_engine::ServiceError::Contract(error.to_string()))?;
    let context = RequestContext {
        request_id: CorrelationId::new(format!("request-{coordinate}"))
            .map_err(|error| rrd_engine::ServiceError::Contract(error.to_string()))?,
        operation_id: CorrelationId::new(format!("operation-{coordinate}"))
            .map_err(|error| rrd_engine::ServiceError::Contract(error.to_string()))?,
        idempotency_key: mutation
            .then(|| CorrelationId::new(format!("idempotency-{coordinate}")))
            .transpose()
            .map_err(|error| rrd_engine::ServiceError::Contract(error.to_string()))?,
        deadline_unix_ms: Some(now.saturating_add(60_000)),
    };
    let resource = ResourcePath {
        segments: vec![
            ResourceId::new(ResourceKind::Instance, engine.instance_id().as_str())
                .map_err(|error| rrd_engine::ServiceError::Contract(error.to_string()))?,
        ],
    };
    engine
        .invoke_runtime_tool(
            project_root,
            &request,
            Invocation {
                context,
                resource,
                observed_at_unix_ms: now,
                attempt: 1,
                request_sha256,
            },
            InvocationCredential::Anonymous,
        )
        .map(|result| result.content)
}

fn next_coordinate() -> String {
    let sequence = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("rrflow-cli-{}-{sequence}", std::process::id())
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
