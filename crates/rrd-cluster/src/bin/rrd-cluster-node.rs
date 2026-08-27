use rrd_cluster::{run_rrflow_runtime, RrdNodeConfig};
use std::path::Path;

fn main() {
    let result = (|| {
        let path = std::env::args_os()
            .nth(1)
            .ok_or("usage: rrd-cluster-node <config.json>")?;
        let config = RrdNodeConfig::load(Path::new(&path)).map_err(|error| error.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("build node runtime: {error}"))?;
        runtime
            .block_on(run_rrflow_runtime(config))
            .map_err(|error| error.to_string())
    })();
    if let Err(error) = result {
        eprintln!("rrd-cluster-node: {error}");
        std::process::exit(1);
    }
}
