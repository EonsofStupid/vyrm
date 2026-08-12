//! Persistence against per-process rebuild.
//!
//! The claim under test: loading the stored projection and refreshing it costs
//! less than rebuilding it, because deserialization replaces parsing. Both
//! paths run in one process, so the page cache is equally warm and the delta
//! isolates parse cost against deserialize cost. Route parity between the
//! built and the loaded index is asserted, not assumed.
//!
//! ```text
//! cargo run --release -p vyrm-graph --example route_persisted -- <store-path> <repo-root> <query>...
//! ```

use vyrm_graph::{Index, Profile};
use vyrm_store::Store;

fn main() {
    let mut args = std::env::args().skip(1);
    let store_path = args.next().expect("usage: route_persisted <store-path> <repo-root> <query>...");
    let root = args.next().expect("usage: route_persisted <store-path> <repo-root> <query>...");
    let queries: Vec<String> = args.collect();

    // Paths inside the index are joined from the root as spelled, so the root
    // is canonicalized to make the stored projection stable across differing
    // invocations of the same tree.
    let root = std::fs::canonicalize(&root).expect("repo root exists");
    let projection = format!("graph_index/{}", root.display());

    let store = Store::open(std::path::Path::new(&store_path)).expect("open store");
    let profile = Profile::attune(&root).expect("attune");

    let started = std::time::Instant::now();
    let built = Index::build(&profile).expect("build");
    let build_ms = started.elapsed().as_millis();

    let started = std::time::Instant::now();
    let bytes = built.to_bytes();
    store.put_projection(&projection, &bytes).expect("store projection");
    let save_ms = started.elapsed().as_millis();

    let started = std::time::Instant::now();
    let loaded_bytes = store
        .get_projection(&projection)
        .expect("read projection")
        .expect("projection just stored");
    let mut loaded = Index::from_bytes(&loaded_bytes).expect("deserialize");
    let load_ms = started.elapsed().as_millis();

    let started = std::time::Instant::now();
    let refresh = loaded.refresh(&profile).expect("refresh");
    let refresh_ms = started.elapsed().as_millis();

    for query in &queries {
        assert_eq!(
            loaded.route(query, 5),
            built.route(query, 5),
            "route parity failed for {query:?}: the loaded projection is not the built one"
        );
    }

    println!("root:                {}", root.display());
    println!("files / symbols:     {} / {}", built.file_count(), built.symbol_count());
    println!("projection size:     {} bytes", bytes.len());
    println!("rebuild (parse):     {build_ms} ms");
    println!("save  (serialize+put): {save_ms} ms");
    println!("load  (get+deserialize): {load_ms} ms");
    println!("refresh after load:  {refresh_ms} ms ({})", refresh.render());
    println!(
        "\nload+refresh vs rebuild: {} ms vs {build_ms} ms ({:.1}x)",
        load_ms + refresh_ms,
        if load_ms + refresh_ms > 0 {
            build_ms as f64 / (load_ms + refresh_ms) as f64
        } else {
            f64::INFINITY
        }
    );
    println!(
        "route parity:        {} quer{} identical between built and loaded",
        queries.len(),
        if queries.len() == 1 { "y" } else { "ies" }
    );
}
