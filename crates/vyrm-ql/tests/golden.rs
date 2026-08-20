use serde::Serialize;
use vyrm_ql::parse;

#[derive(Serialize)]
struct Vector {
    source: &'static str,
    canonical: String,
    ast: vyrm_ql::Query,
}

#[test]
fn query_ast_matches_golden_vectors() {
    let sources = [
        "FROM record:document AT VALID 100 KNOWN HEAD WHERE status = \"open\" PROJECT id, status LIMIT 10 EXPLAIN CONTRACT",
        "FROM relation:depends_on AT VALID $when KNOWN $cursor PROJECT *",
        "FROM claim:status AT VALID 50 KNOWN 7 PROJECT subject, object",
    ];
    let vectors = sources
        .into_iter()
        .map(|source| {
            let ast = parse(source).unwrap();
            Vector {
                source,
                canonical: ast.canonical(),
                ast,
            }
        })
        .collect::<Vec<_>>();
    let actual = format!("{}\n", serde_json::to_string_pretty(&vectors).unwrap());
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/query-vectors.json");
    if std::env::var_os("VYRM_UPDATE_GOLDENS").is_some() {
        std::fs::create_dir_all(format!("{}/fixtures", env!("CARGO_MANIFEST_DIR"))).unwrap();
        std::fs::write(path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(path)
        .unwrap_or_else(|_| panic!("missing {path}; run with VYRM_UPDATE_GOLDENS=1"));
    assert_eq!(actual, expected);
}
