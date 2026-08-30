use rrd_contract::{
    CanonicalId, ResourceId, ResourceKind, ResourcePath, SdkConformanceCorpus, SecurityAction,
};
use rrd_core::{
    digest, RuntimeCommit, RuntimeMutation, RuntimePropertySchema, RuntimeRecordSchema,
    RuntimeSchemaRegistry, RuntimeType, RuntimeValueType, ScopeId,
};
use rrd_engine::{load_or_create_token_key, InstanceBinding, InstanceManifest, RrdEngine};
use rrd_estate::{EstateRepository, MutationContext};
use rrd_security::{
    Principal, PrincipalKind, ResourceGrant, SecurityRepository, SecurityState, SECURITY_FORMAT,
};
use rrd_server::RrdHttpServer;
use rrd_store::{Engine, PersistentEngine};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task::JoinHandle;

const LANGUAGES: [&str; 6] = ["rust", "typescript", "python", "go", "java", "dotnet"];

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct HarnessManifest {
    format_version: u16,
    corpus_sha256: String,
    corpus_path: String,
    base_url: String,
    retry_base_urls: BTreeMap<String, String>,
    incompatible_version_url: String,
    shutdown_path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = Arguments::parse()?;
    let corpus_bytes = std::fs::read(&arguments.corpus)?;
    let corpus: SdkConformanceCorpus = serde_json::from_slice(&corpus_bytes)?;
    corpus.validate()?;

    let temporary = tempfile::tempdir()?;
    let project = temporary.path().join("project");
    std::fs::create_dir_all(&project)?;
    InstanceManifest::ensure_dedicated_as(&project, corpus.identity.instance.as_str())?;
    let binding = InstanceBinding::discover(&project)?;
    let root = binding.expected_store();
    let storage = PersistentEngine::open(&root)?;
    seed_schema(&storage, &corpus)?;
    seed_estate(&storage, &corpus)?;
    seed_security(&storage, &corpus)?;
    drop(storage);

    let token_key = load_or_create_token_key(&root.join("RRD.SERVER.SECRET"))?;
    let engine = RrdEngine::open_bound_with_token_key(
        &binding,
        corpus.identity.instance.clone(),
        token_key,
        2,
    )?;
    let authority = binding.authority_binding()?;
    let server =
        RrdHttpServer::bind_project(engine, authority, "127.0.0.1:0".parse::<SocketAddr>()?)?;
    let server_address = server.local_addr();
    let (server_shutdown, receiver) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = receiver.await;
    }));

    let mut retry_base_urls = BTreeMap::new();
    let mut proxy_tasks = Vec::new();
    for language in LANGUAGES {
        let (address, task) = spawn_drop_once_proxy(server_address).await?;
        retry_base_urls.insert(language.to_owned(), loopback_url(address));
        proxy_tasks.push(task);
    }
    let (version_address, version_task) =
        spawn_incompatible_version_server(corpus.incompatible_protocol_version).await?;

    let manifest = HarnessManifest {
        format_version: corpus.format_version,
        corpus_sha256: digest::sha256_hex(&corpus_bytes),
        corpus_path: arguments.corpus.to_string_lossy().into_owned(),
        base_url: loopback_url(server_address),
        retry_base_urls,
        incompatible_version_url: loopback_url(version_address),
        shutdown_path: arguments.shutdown.to_string_lossy().into_owned(),
    };
    write_owner_only_json(&arguments.manifest, &manifest)?;

    while !arguments.shutdown.is_file() {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let _ = server_shutdown.send(());
    server_task.await??;
    for task in proxy_tasks {
        task.abort();
        let _ = task.await;
    }
    version_task.abort();
    let _ = version_task.await;
    Ok(())
}

struct Arguments {
    corpus: PathBuf,
    manifest: PathBuf,
    shutdown: PathBuf,
}

impl Arguments {
    fn parse() -> Result<Self, String> {
        let mut corpus = None;
        let mut manifest = None;
        let mut shutdown = None;
        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            let value = arguments
                .next()
                .ok_or_else(|| format!("{argument} requires a path"))?;
            match argument.as_str() {
                "--corpus" => corpus = Some(PathBuf::from(value)),
                "--manifest" => manifest = Some(PathBuf::from(value)),
                "--shutdown" => shutdown = Some(PathBuf::from(value)),
                _ => return Err(format!("unknown argument {argument}")),
            }
        }
        Ok(Self {
            corpus: corpus.ok_or("--corpus is required")?,
            manifest: manifest.ok_or("--manifest is required")?,
            shutdown: shutdown.ok_or("--shutdown is required")?,
        })
    }
}

fn seed_schema(
    storage: &PersistentEngine,
    corpus: &SdkConformanceCorpus,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = RuntimeSchemaRegistry::empty(1, "SDK conformance schema");
    registry.records.insert(
        RuntimeType::new("document")?,
        RuntimeRecordSchema {
            properties: BTreeMap::from([(
                "title".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            )]),
            ..RuntimeRecordSchema::default()
        },
    );
    storage.commit_runtime(&RuntimeCommit {
        scope: ScopeId::new(format!("instance:{}", corpus.identity.instance))?,
        at: 100,
        actor: "sdk-conformance-harness".into(),
        expected_cursor: 0,
        mutations: vec![RuntimeMutation::Schema { registry }],
    })?;
    Ok(())
}

fn seed_estate(
    storage: &PersistentEngine,
    corpus: &SdkConformanceCorpus,
) -> Result<(), Box<dyn std::error::Error>> {
    EstateRepository::new(storage, corpus.identity.estate.clone()).create(&MutationContext {
        at: 110,
        actor: "sdk-conformance-harness".into(),
        request_id: "request-sdk-estate".into(),
        operation_id: CanonicalId::new("sdk-estate-create")?,
    })?;
    Ok(())
}

fn seed_security(
    storage: &PersistentEngine,
    corpus: &SdkConformanceCorpus,
) -> Result<(), Box<dyn std::error::Error>> {
    let instance_resource = ResourcePath {
        segments: vec![ResourceId::new(
            ResourceKind::Instance,
            corpus.identity.instance.as_str(),
        )?],
    };
    let estate_resource = ResourcePath {
        segments: vec![ResourceId::new(
            ResourceKind::Estate,
            corpus.identity.estate.as_str(),
        )?],
    };
    let actions = [
        SecurityAction::SessionCreate,
        SecurityAction::SessionRenew,
        SecurityAction::SessionClose,
        SecurityAction::TransactionBegin,
        SecurityAction::TransactionPreview,
        SecurityAction::TransactionCommit,
        SecurityAction::TransactionAbort,
        SecurityAction::QueryExecute,
        SecurityAction::VectorCollectionEnsure,
        SecurityAction::VectorCollectionList,
        SecurityAction::VectorSearch,
        SecurityAction::ChangefeedRead,
        SecurityAction::ChangefeedFollow,
        SecurityAction::BackupCreate,
        SecurityAction::BackupList,
        SecurityAction::EstateRead,
    ];
    let mut grants = actions
        .into_iter()
        .map(|action| ResourceGrant {
            action,
            resource_prefix: instance_resource.clone(),
            data_policy: None,
        })
        .collect::<Vec<_>>();
    grants.push(ResourceGrant {
        action: SecurityAction::EstateRead,
        resource_prefix: estate_resource,
        data_policy: None,
    });
    let principal = Principal {
        id: corpus.identity.principal.clone(),
        kind: PrincipalKind::Service,
        credential_sha256: digest::sha256_hex(corpus.identity.api_key.as_bytes()),
        credential_revision: 1,
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        role_ids: Default::default(),
        grants,
    };
    SecurityRepository::new(storage, corpus.identity.instance.clone()).initialize(
        SecurityState {
            format_version: SECURITY_FORMAT,
            revision: 1,
            principals: BTreeMap::from([(principal.id.clone(), principal)]),
            roles: BTreeMap::new(),
            identity_bindings: BTreeMap::new(),
            jwt_issuers: BTreeMap::new(),
        },
        120,
        "sdk-conformance-harness",
        "request-sdk-security",
        "operation-sdk-security",
    )?;
    Ok(())
}

async fn spawn_drop_once_proxy(
    upstream: SocketAddr,
) -> Result<(SocketAddr, JoinHandle<()>), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let mut drop_next = true;
        loop {
            let Ok((mut downstream, _)) = listener.accept().await else {
                break;
            };
            if drop_next {
                drop_next = false;
                drop(downstream);
                continue;
            }
            tokio::spawn(async move {
                let Ok(mut upstream_stream) = tokio::net::TcpStream::connect(upstream).await else {
                    return;
                };
                let _ = tokio::io::copy_bidirectional(&mut downstream, &mut upstream_stream).await;
            });
        }
    });
    Ok((address, task))
}

async fn spawn_incompatible_version_server(
    protocol_version: u16,
) -> Result<(SocketAddr, JoinHandle<()>), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut request = vec![0_u8; 8 * 1024];
                let _ = stream.read(&mut request).await;
                let body = format!(
                    "{{\"protocol\":\"rrd\",\"protocol_version\":{protocol_version},\"request_id\":\"version-mismatch\",\"operation_id\":\"capabilities-read\",\"outcome\":{{\"status\":\"ok\",\"payload\":{{}}}}}}"
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    Ok((address, task))
}

fn loopback_url(address: SocketAddr) -> String {
    format!("http://{address}/")
}

fn write_owner_only_json(
    path: &Path,
    value: &impl Serialize,
) -> Result<(), Box<dyn std::error::Error>> {
    let encoded = serde_json::to_vec_pretty(value)?;
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?
    };
    #[cfg(not(unix))]
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&encoded)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
