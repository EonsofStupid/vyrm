//! Ranking: reference-graph centrality and the line-budget fill.
//!
//! Centrality must break ties between definers of the same name in favor of the
//! one the repository leans on, must never route a file unrelated to the query,
//! and must never displace a declaration site with a heavily central caller.
//! The budget fill must always return the top-ranked file and must fill
//! first-fit in rank order.

use vyrm_graph::{Index, Profile};

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"t\"\n").unwrap();
    for (name, body) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }
    dir
}

fn indexed(dir: &tempfile::TempDir) -> Index {
    let profile = Profile::attune(dir.path()).unwrap();
    Index::build(&profile).unwrap()
}

/// A body of `lines` total lines whose first line is `head`. Padding lines use
/// a term nothing defines, so they add lines without adding routing signal.
fn padded(head: &str, lines: usize) -> String {
    let mut body = String::from(head);
    for _ in 1..lines {
        body.push_str("\n// padding_line_without_signal");
    }
    body
}

#[test]
fn centrality_ranks_the_leaned_on_definer_above_an_equivalent_one() {
    // a.rs and b.rs both define `widget` once. a.rs also defines `alpha_util`,
    // which three other files reference, so rank flows to a.rs; b.rs defines
    // nothing anyone references.
    let dir = project(&[
        ("src/a.rs", "pub fn widget() {}\npub fn alpha_util() {}"),
        ("src/b.rs", "pub fn widget() {}\npub fn beta_util() {}"),
        ("src/c.rs", "pub fn c() { crate::a::alpha_util(); }"),
        ("src/d.rs", "pub fn d() { crate::a::alpha_util(); }"),
        ("src/e.rs", "pub fn e() { crate::a::alpha_util(); }"),
    ]);
    let index = indexed(&dir);

    let routed = index.route("widget", 5);
    assert_eq!(routed.len(), 2, "only the two definers mention widget");
    assert!(
        routed[0].path.ends_with("src/a.rs"),
        "the definer the repository references must rank first, got {}",
        routed[0].path.display()
    );
    assert!(
        routed[0].centrality > routed[1].centrality,
        "a.rs centrality {} must exceed b.rs centrality {}",
        routed[0].centrality,
        routed[1].centrality
    );
}

#[test]
fn centrality_never_routes_a_file_unrelated_to_the_query() {
    // a.rs is maximally central, but only b.rs mentions `lonely_name`.
    let dir = project(&[
        ("src/a.rs", "pub fn hub_util() {}"),
        ("src/b.rs", "pub fn lonely_name() {}"),
        ("src/c.rs", "pub fn c() { crate::a::hub_util(); }"),
        ("src/d.rs", "pub fn d() { crate::a::hub_util(); }"),
    ]);
    let index = indexed(&dir);

    let routed = index.route("lonely_name", 5);
    assert_eq!(routed.len(), 1, "an unrelated file was routed on centrality alone");
    assert!(routed[0].path.ends_with("src/b.rs"));
}

#[test]
fn a_declaration_still_outranks_a_central_heavy_caller() {
    // def.rs declares `gizmo` and nothing references def.rs otherwise. hub.rs
    // references `gizmo` on many lines and is the most central file in the
    // repository. The declaration must still rank first: reference weight is
    // capped and the centrality bonus is bounded below the definition weight.
    let mut hub = String::from("pub fn hub_util() {}");
    for _ in 0..30 {
        hub.push_str("\npub fn caller() { crate::def::gizmo(); }");
    }
    let dir = project(&[
        ("src/def.rs", "pub fn gizmo() {}"),
        ("src/hub.rs", hub.as_str()),
        ("src/c.rs", "pub fn c() { crate::hub::hub_util(); }"),
        ("src/d.rs", "pub fn d() { crate::hub::hub_util(); }"),
        ("src/e.rs", "pub fn e() { crate::hub::hub_util(); }"),
    ]);
    let index = indexed(&dir);

    let routed = index.route("gizmo", 5);
    assert!(
        routed[0].path.ends_with("src/def.rs"),
        "the declaration site was displaced by a central caller: {}",
        routed[0].path.display()
    );
}

#[test]
fn budget_fill_always_includes_the_top_ranked_file() {
    let dir = project(&[("src/big.rs", &padded("pub fn wanted_name() {}", 500))]);
    let index = indexed(&dir);

    let routed = index.route_budget("wanted_name", 10);
    assert_eq!(routed.len(), 1, "the top-ranked file must be routed even over budget");
    assert_eq!(routed[0].lines, 500);
}

#[test]
fn budget_fill_skips_an_oversized_file_and_takes_a_smaller_one() {
    // Rank order: definer first, then the reference-heavy 900-line file, then a
    // lightly referencing 50-line file. At a 200-line budget the 900-line file
    // must be skipped and the 50-line file still taken.
    let mut heavy = padded("pub fn heavy_top() {}", 870);
    for _ in 0..30 {
        heavy.push_str("\npub fn h() { crate::def::target_name(); }");
    }
    let dir = project(&[
        ("src/def.rs", &padded("pub fn target_name() {}", 100)),
        ("src/heavy.rs", &heavy),
        ("src/light.rs", &padded("pub fn l() { crate::def::target_name(); }", 50)),
    ]);
    let index = indexed(&dir);

    let ranked = index.route("target_name", 5);
    assert_eq!(ranked.len(), 3);
    assert!(ranked[0].path.ends_with("src/def.rs"));
    assert!(ranked[1].path.ends_with("src/heavy.rs"), "heavy must rank above light");

    let routed = index.route_budget("target_name", 200);
    let paths: Vec<_> = routed.iter().map(|r| r.path.clone()).collect();
    assert_eq!(paths.len(), 2, "expected def.rs and light.rs, got {paths:?}");
    assert!(paths[0].ends_with("src/def.rs"));
    assert!(paths[1].ends_with("src/light.rs"), "the smaller file was not back-filled");
    assert!(routed.iter().map(|r| r.lines).sum::<usize>() <= 200);
}
