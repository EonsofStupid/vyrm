use std::path::Path;
use vyrm_cluster::{run_vyrm_node, VyrmNodeConfig};

fn main() {
    let result = (|| {
        let path = std::env::args_os()
            .nth(1)
            .ok_or("usage: vyrm-cluster-node <config.json>")?;
        let config = VyrmNodeConfig::load(Path::new(&path)).map_err(|error| error.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("build node runtime: {error}"))?;
        runtime
            .block_on(run_vyrm_node(config))
            .map_err(|error| error.to_string())
    })();
    if let Err(error) = result {
        eprintln!("vyrm-cluster-node: {error}");
        std::process::exit(1);
    }
}
