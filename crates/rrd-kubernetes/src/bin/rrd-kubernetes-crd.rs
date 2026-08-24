fn main() {
    let document =
        serde_json::to_string_pretty(&rrd_kubernetes::crd_document()).expect("CRD serializes");
    println!("{document}");
}
