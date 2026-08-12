//! Routing precision against a plain text scan.
//!
//! The reading rule ("read the returned files in full") only pays if routing is
//! precise. This example measures that directly: for each query, how many files
//! each approach returns, how many lines that is to read, and whether the
//! declaration site is ranked first.
//!
//! ```text
//! cargo run --release -p vyrm-graph --example route_vs_scan -- <repo-root> <query>...
//! ```

use vyrm_graph::attune_and_index;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("usage: route_vs_scan <repo-root> <query>...");
    let queries: Vec<String> = args.collect();
    let queries = if queries.is_empty() {
        vec!["claim_key".into(), "append_batch".into(), "Store".into()]
    } else {
        queries
    };

    let started = std::time::Instant::now();
    let (profile, index) =
        attune_and_index(std::path::Path::new(&root)).expect("attune and index");
    let build_ms = started.elapsed().as_millis();

    println!("{}", profile.render());
    println!(
        "  indexed:   {} file(s), {} distinct symbol(s), {} ms\n",
        index.file_count(),
        index.symbol_count(),
        build_ms
    );

    // Incremental maintenance: the cost of staying current versus rebuilding.
    let mut index = index;
    let noop = index.refresh(&profile).expect("refresh");
    println!("  refresh (no changes): {}", noop.render());
    let grounding = index.ground(&profile).expect("ground");
    println!("  {}\n", grounding.render());

    println!(
        "{:<24} {:>10} {:>10} {:>12} {:>12} {:>8}",
        "query", "scan files", "routed", "scan lines", "routed lines", "top=def"
    );
    println!("{}", "-".repeat(82));

    let (mut scan_total, mut routed_total) = (0usize, 0usize);
    for query in &queries {
        let scanned = index.text_scan(query);
        let routed = index.route(query, 5);

        let scan_lines: usize = scanned
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .map(|t| t.lines().count())
            .sum();
        let routed_lines: usize = routed.iter().map(|r| r.lines).sum();

        let top_defines = routed
            .first()
            .map(|r| !r.justification.defines.is_empty())
            .unwrap_or(false);

        scan_total += scan_lines;
        routed_total += routed_lines;

        println!(
            "{:<24} {:>10} {:>10} {:>12} {:>12} {:>8}",
            query,
            scanned.len(),
            routed.len(),
            scan_lines,
            routed_lines,
            if top_defines { "yes" } else { "NO" },
        );
    }

    println!("{}", "-".repeat(82));
    let ratio = if routed_total == 0 {
        0.0
    } else {
        scan_total as f64 / routed_total as f64
    };
    println!(
        "{:<24} {:>10} {:>10} {:>12} {:>12}",
        "TOTAL", "", "", scan_total, routed_total
    );
    println!("\nlines to read, scan / routed: {ratio:.2}x");

    // Budget fill. The routed-lines column is bounded by the budget by
    // construction, so its ratio against the scan is not evidence of ranking
    // quality on its own — the columns that carry evidence are top=def (did the
    // declaration survive the tighter cut) and files (what filled the budget).
    const LINE_BUDGET: usize = 1_000;
    println!("\nbudget fill at {LINE_BUDGET} lines:");
    println!(
        "{:<24} {:>10} {:>12} {:>8}",
        "query", "routed", "routed lines", "top=def"
    );
    println!("{}", "-".repeat(58));
    let mut budget_total = 0usize;
    for query in &queries {
        let routed = index.route_budget(query, LINE_BUDGET);
        let routed_lines: usize = routed.iter().map(|r| r.lines).sum();
        let top_defines = routed
            .first()
            .map(|r| !r.justification.defines.is_empty())
            .unwrap_or(false);
        budget_total += routed_lines;
        println!(
            "{:<24} {:>10} {:>12} {:>8}",
            query,
            routed.len(),
            routed_lines,
            if top_defines { "yes" } else { "NO" },
        );
    }
    println!("{}", "-".repeat(58));
    let budget_ratio = if budget_total == 0 {
        0.0
    } else {
        scan_total as f64 / budget_total as f64
    };
    println!("{:<24} {:>10} {:>12}", "TOTAL", "", budget_total);
    println!("\nlines to read, scan / budget-routed: {budget_ratio:.2}x");

    // One worked example, so the justification format is visible.
    if let Some(query) = queries.first() {
        println!("\njustification for \"{query}\":");
        for routed in index.route(query, 5) {
            println!(
                "  {:>6.1}  c={:.2}  {:<58} {}",
                routed.score,
                routed.centrality,
                routed
                    .path
                    .strip_prefix(&profile.root)
                    .unwrap_or(&routed.path)
                    .display()
                    .to_string()
                    .chars()
                    .take(58)
                    .collect::<String>(),
                routed.justification.render()
            );
        }
    }
}
