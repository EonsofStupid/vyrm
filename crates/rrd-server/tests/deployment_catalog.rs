use rrd_estate::{LocalArgument, LocalDeploymentCatalog, LocalShutdown};
use std::process::Command;

#[test]
fn generator_authenticates_the_real_server_and_refuses_overwrite() {
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("deployments.json");
    let server = std::fs::canonicalize(env!("CARGO_BIN_EXE_rrd-server")).unwrap();
    let generator = env!("CARGO_BIN_EXE_rrd-deployment-catalog");
    let first = Command::new(generator)
        .args(["--output", output.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );

    let catalog = LocalDeploymentCatalog::load_json(&output).unwrap();
    let deployment = &catalog.deployments["rrd-server"];
    assert_eq!(deployment.executable, server);
    assert_eq!(deployment.version, env!("CARGO_PKG_VERSION"));
    assert!(deployment.arguments.contains(&LocalArgument::InstanceId));
    assert!(matches!(
        deployment.shutdown,
        LocalShutdown::RequestFile {
            timeout_ms: 5_000,
            ..
        }
    ));

    let second = Command::new(generator)
        .args(["--output", output.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("already exists"));
    assert!(LocalDeploymentCatalog::load_json(&output).is_ok());
}
