use rrd_estate::{LocalArgument, LocalDeploymentCatalog, LocalShutdown};
use std::process::Command;

#[test]
fn generator_authenticates_an_explicit_executable_and_refuses_overwrite() {
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("deployments.json");
    let server = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let generator = env!("CARGO_BIN_EXE_rrd-deployment-catalog");
    let first = Command::new(generator)
        .args([
            "--server",
            server.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
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
    assert!(deployment
        .preparation_arguments
        .contains(&LocalArgument::InstanceId));
    assert!(deployment.arguments.windows(2).any(|arguments| {
        arguments
            == [
                LocalArgument::Literal("--root".into()),
                LocalArgument::InstanceRoot,
            ]
    }));
    assert!(matches!(
        deployment.shutdown,
        LocalShutdown::RequestFile {
            timeout_ms: 5_000,
            ..
        }
    ));

    let second = Command::new(generator)
        .args([
            "--server",
            server.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("already exists"));
    assert!(LocalDeploymentCatalog::load_json(&output).is_ok());
}
