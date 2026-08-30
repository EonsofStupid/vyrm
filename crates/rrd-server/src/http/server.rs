use super::*;

pub struct RrdHttpServer {
    listener: TcpListener,
    app: Router,
    tls: Option<TlsAcceptor>,
}

/// In-memory verifier material for RRD-issued HS256 session credentials.
/// Only its SHA-256 is persisted in the security authority.
#[derive(Clone)]
pub struct RrdJwtVerificationKey {
    bytes: Arc<[u8]>,
}

impl RrdJwtVerificationKey {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() < 32 || bytes.len() > 64 * 1024 {
            return Err(HttpError::Contract(
                "JWT verification key must contain 32..=65536 bytes".into(),
            ));
        }
        Ok(Self {
            bytes: Arc::from(bytes),
        })
    }

    pub(super) fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A server identity whose verifier requires a trusted client certificate.
///
/// The inner Rustls configuration is private so a remote RRD listener cannot
/// accidentally be constructed with anonymous TLS.
#[derive(Clone)]
pub struct RrdMutualTlsServerConfig {
    inner: Arc<ServerConfig>,
}

impl RrdMutualTlsServerConfig {
    pub fn new(
        certificate_chain: Vec<CertificateDer<'static>>,
        private_key: PrivateKeyDer<'static>,
        client_roots: RootCertStore,
    ) -> Result<Self> {
        if certificate_chain.is_empty() || client_roots.is_empty() {
            return Err(HttpError::Tls(
                "RRD mTLS requires a certificate chain and client trust roots".into(),
            ));
        }
        let verifier = WebPkiClientVerifier::builder(Arc::new(client_roots))
            .build()
            .map_err(|error| HttpError::Tls(format!("RRD client verifier: {error}")))?;
        let mut inner = ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
            .with_client_cert_verifier(verifier)
            .with_single_cert(certificate_chain, private_key)
            .map_err(|error| HttpError::Tls(format!("RRD server identity: {error}")))?;
        inner.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Self {
            inner: Arc::new(inner),
        })
    }
}

pub(super) struct AppState {
    pub(super) service: RrdEngine,
    pub(super) capabilities: ServiceCapabilities,
    pub(super) project_root: Option<std::path::PathBuf>,
    pub(super) jwt_verification_key: Option<RrdJwtVerificationKey>,
}

impl RrdHttpServer {
    pub fn bind(engine: RrdEngine, bind: SocketAddr) -> Result<Self> {
        if !bind.ip().is_loopback() {
            return Err(HttpError::RemoteBindDenied(bind));
        }
        Self::bind_inner(engine, bind, None, None, None)
    }

    pub fn bind_with_jwt(
        engine: RrdEngine,
        bind: SocketAddr,
        jwt_verification_key: RrdJwtVerificationKey,
    ) -> Result<Self> {
        if !bind.ip().is_loopback() {
            return Err(HttpError::RemoteBindDenied(bind));
        }
        Self::bind_inner(engine, bind, None, None, Some(jwt_verification_key))
    }

    pub fn bind_project(
        engine: RrdEngine,
        project: ProjectAuthorityBinding,
        bind: SocketAddr,
    ) -> Result<Self> {
        if !bind.ip().is_loopback() {
            return Err(HttpError::RemoteBindDenied(bind));
        }
        Self::bind_inner(engine, bind, None, Some(project), None)
    }

    pub fn bind_project_with_jwt(
        engine: RrdEngine,
        project: ProjectAuthorityBinding,
        bind: SocketAddr,
        jwt_verification_key: RrdJwtVerificationKey,
    ) -> Result<Self> {
        if !bind.ip().is_loopback() {
            return Err(HttpError::RemoteBindDenied(bind));
        }
        Self::bind_inner(
            engine,
            bind,
            None,
            Some(project),
            Some(jwt_verification_key),
        )
    }

    pub fn bind_mtls(
        engine: RrdEngine,
        bind: SocketAddr,
        tls: RrdMutualTlsServerConfig,
    ) -> Result<Self> {
        Self::bind_inner(engine, bind, Some(TlsAcceptor::from(tls.inner)), None, None)
    }

    pub fn bind_project_mtls(
        engine: RrdEngine,
        project: ProjectAuthorityBinding,
        bind: SocketAddr,
        tls: RrdMutualTlsServerConfig,
    ) -> Result<Self> {
        Self::bind_inner(
            engine,
            bind,
            Some(TlsAcceptor::from(tls.inner)),
            Some(project),
            None,
        )
    }

    pub fn bind_project_mtls_with_jwt(
        engine: RrdEngine,
        project: ProjectAuthorityBinding,
        bind: SocketAddr,
        tls: RrdMutualTlsServerConfig,
        jwt_verification_key: RrdJwtVerificationKey,
    ) -> Result<Self> {
        Self::bind_inner(
            engine,
            bind,
            Some(TlsAcceptor::from(tls.inner)),
            Some(project),
            Some(jwt_verification_key),
        )
    }

    fn bind_inner(
        engine: RrdEngine,
        bind: SocketAddr,
        tls: Option<TlsAcceptor>,
        project: Option<ProjectAuthorityBinding>,
        jwt_verification_key: Option<RrdJwtVerificationKey>,
    ) -> Result<Self> {
        let instance = engine.instance_id().clone();
        if let Some(project) = &project {
            project
                .validate()
                .map_err(|error| HttpError::Contract(error.to_string()))?;
            if project.instance_id != instance {
                return Err(HttpError::Contract(
                    "project authority and engine instance identities differ".into(),
                ));
            }
            if engine
                .project_authority_binding()
                .map_err(|error| HttpError::Contract(error.to_string()))?
                .as_ref()
                != Some(project)
            {
                return Err(HttpError::Contract(
                    "project authority is not persisted by this engine".into(),
                ));
            }
        }
        let backend = engine
            .readiness(0)
            .map(|readiness| readiness.backend)
            .map_err(|error| HttpError::Contract(error.to_string()))?;
        let security_enforced = engine
            .security_enforced()
            .map_err(|error| HttpError::Contract(error.to_string()))?;
        if tls.is_some() && !security_enforced {
            return Err(HttpError::RemoteSecurityRequired);
        }
        if jwt_verification_key.is_some() && !security_enforced {
            return Err(HttpError::Contract(
                "RRD JWT verification requires an initialized security authority".into(),
            ));
        }
        let tls_enabled = tls.is_some();
        let jwt_enabled = jwt_verification_key.is_some();
        let listener =
            TcpListener::bind(bind).map_err(|error| HttpError::Bind(error.to_string()))?;
        listener.set_nonblocking(true)?;
        let capabilities = capabilities(
            &instance,
            backend,
            security_enforced,
            tls_enabled,
            jwt_enabled,
        );
        capabilities
            .validate()
            .map_err(|error| HttpError::Contract(error.to_string()))?;
        let project_root = project.map(|binding| std::path::PathBuf::from(binding.project_root));
        let state = Arc::new(AppState {
            service: engine,
            capabilities,
            project_root,
            jwt_verification_key,
        });
        let endpoint_catalogue = rrd_contract::endpoint_catalogue();
        let subscription_stream = endpoint_catalogue
            .websocket_endpoints
            .iter()
            .find(|endpoint| endpoint.operation.as_str() == "subscription-stream")
            .ok_or_else(|| {
                HttpError::Contract(
                    "endpoint catalogue omitted the subscription-stream dispatch".into(),
                )
            })?;
        let app = Router::new()
            .route(
                &subscription_stream.path,
                get(subscription_websocket_upgrade),
            )
            .fallback(any(dispatch))
            .with_state(state);
        Ok(Self { listener, app, tls })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.listener
            .local_addr()
            .expect("bound RRD listener has a local address")
    }

    pub fn is_tls(&self) -> bool {
        self.tls.is_some()
    }

    pub async fn serve(self) -> Result<()> {
        if self.tls.is_some() {
            return self.serve_until(std::future::pending()).await;
        }
        let listener = tokio::net::TcpListener::from_std(self.listener)?;
        axum::serve(listener, self.app).await.map_err(HttpError::Io)
    }

    pub async fn serve_until<F>(self, shutdown: F) -> Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let listener = tokio::net::TcpListener::from_std(self.listener)?;
        if let Some(acceptor) = self.tls {
            return serve_mtls(listener, self.app, acceptor, shutdown).await;
        }
        axum::serve(listener, self.app)
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(HttpError::Io)
    }
}

async fn serve_mtls<F>(
    listener: tokio::net::TcpListener,
    app: Router,
    acceptor: TlsAcceptor,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let acceptor = acceptor.clone();
                let app = app.clone();
                connections.spawn(async move {
                    let stream = acceptor.accept(stream).await.map_err(io::Error::other)?;
                    let service = TowerToHyperService::new(app);
                    let builder = http1::Builder::new();
                    builder
                        .serve_connection(TokioIo::new(stream), service)
                        .with_upgrades()
                        .await
                        .map_err(io::Error::other)
                });
            }
            completed = connections.join_next(), if !connections.is_empty() => {
                match completed {
                    Some(Ok(Err(error))) => {
                        tracing::debug!(error = %error, "RRD TLS connection closed");
                    }
                    Some(Err(error)) => {
                        tracing::warn!(error = %error, "RRD TLS connection task failed");
                    }
                    Some(Ok(Ok(()))) | None => {}
                }
            }
        }
    }
    if tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while connections.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        connections.abort_all();
    }
    Ok(())
}
