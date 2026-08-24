#[test]
fn generated_crd_is_namespaced_structural_and_status_enabled() {
    let crd = rrd_kubernetes::crd_document();
    assert_eq!(crd["apiVersion"], "apiextensions.k8s.io/v1");
    assert_eq!(crd["spec"]["group"], "rrflow.io");
    assert_eq!(crd["spec"]["scope"], "Namespaced");
    assert_eq!(crd["spec"]["names"]["plural"], "rrdinstances");
    assert_eq!(crd["spec"]["versions"][0]["name"], "v1alpha1");
    assert_eq!(
        crd["spec"]["versions"][0]["subresources"]["status"],
        serde_json::json!({})
    );
    let schema = &crd["spec"]["versions"][0]["schema"]["openAPIV3Schema"];
    assert_eq!(schema["properties"]["spec"]["type"], "object");
    assert!(schema["properties"]["spec"]["properties"]["image"].is_object());
    assert_eq!(
        schema["properties"]["spec"]["x-kubernetes-validations"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
}

#[test]
fn checked_deployment_artifacts_match_the_generated_contract_and_least_privilege_scope() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let checked: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("deploy/kubernetes/rrdinstances.rrflow.io-crd.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(checked, rrd_kubernetes::crd_document());

    let rbac: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("deploy/kubernetes/operator-rbac.json")).unwrap(),
    )
    .unwrap();
    let encoded = serde_json::to_string(&rbac).unwrap();
    assert!(!encoded.contains("\"secrets\""));
    assert!(!encoded.contains("\"pods/exec\""));
    assert!(!encoded.contains("\"*\""));
    assert!(encoded.contains("rrdinstances/status"));
    assert!(encoded.contains("statefulsets"));

    let example: rrd_kubernetes::RrdInstance = serde_json::from_slice(
        &std::fs::read(root.join("deploy/kubernetes/example-rrdinstance.json")).unwrap(),
    )
    .unwrap();
    example.spec.validate().unwrap();
}
