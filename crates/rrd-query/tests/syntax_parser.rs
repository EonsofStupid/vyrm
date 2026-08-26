use rrd_core::{RuntimeType, RuntimeValue};
use rrd_query::{
    parse, ComparisonOperator, CursorExpr, Filter, Projection, Query, Source, TemporalSelector,
    TimeExpr, ValueExpr,
};

#[test]
fn parser_corpus_round_trips_to_one_canonical_form() {
    let corpus = [
        "FROM record:document AT VALID 100 KNOWN HEAD PROJECT *",
        "from relation:depends_on at valid $when known $cursor where state = \"open\" project id, from_id limit 20 explain contract",
        "FROM event:tool_result AT VALID 99 KNOWN 42 WHERE ok = true AND retries = 2 PROJECT cursor, ok",
        "FROM event:tool_result AT VALID 99 KNOWN 42 WHERE retries >= 2 AND retries < 5 PROJECT cursor, retries",
        "FROM claim:status AT VALID 1000 KNOWN HEAD WHERE object = \"ready\" PROJECT subject, object",
        "FROM claim AT VALID 0 KNOWN 0 PROJECT * LIMIT 1",
        "FROM series:metric AT VALID 1000 KNOWN HEAD WHERE series_id = \"latency\" PROJECT observed_at, value",
        "FROM geo:location AT VALID 1000 KNOWN HEAD PROJECT subject_id, geometry_kind",
        "FROM record:document AT VALID 100 KNOWN HEAD WHERE title MATCH \"frontier runtime\" PROJECT id, title LIMIT 5",
        "FROM traverse:depends_on START document:a DIRECTION OUTGOING DEPTH 4 AT VALID 1000 KNOWN HEAD PROJECT node_id, depth, path",
    ];
    for source in corpus {
        let first = parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
        let canonical = first.canonical();
        let second = parse(&canonical).unwrap();
        assert_eq!(first, second, "canonical query: {canonical}");
    }
}

#[test]
fn match_is_a_typed_canonical_comparison() {
    let query = parse(
        "FROM record:document AT VALID 100 KNOWN HEAD WHERE title match \"Frontier AI\" PROJECT title",
    )
    .unwrap();
    assert_eq!(query.filters[0].comparison, ComparisonOperator::Match);
    assert_eq!(
        query.canonical(),
        "FROM record:document AT VALID 100 KNOWN HEAD WHERE title MATCH \"Frontier AI\" PROJECT title"
    );
}

#[test]
fn typed_sdk_and_text_construct_the_same_ast() {
    let mut typed = Query::new(
        Source::Record {
            kind: RuntimeType::new("document").unwrap(),
        },
        TemporalSelector {
            valid_at: TimeExpr::Parameter("when".into()),
            known_at: CursorExpr::Head,
        },
    );
    typed.filters.push(Filter {
        field: "status".into(),
        comparison: ComparisonOperator::Equal,
        value: ValueExpr::Literal(RuntimeValue::String("open".into())),
    });
    typed.projection = Projection::Fields(vec!["id".into(), "status".into()]);
    typed.limit = Some(10);
    typed.explain_contract = true;

    let parsed = parse(
        "FROM record:document AT VALID $when KNOWN HEAD WHERE status = \"open\" PROJECT id, status LIMIT 10 EXPLAIN CONTRACT",
    )
    .unwrap();
    assert_eq!(parsed, typed);
}

#[test]
fn malformed_or_ambiguous_queries_fail_with_offsets() {
    for source in [
        "",
        "FROM record:doc",
        "FROM unknown:doc AT VALID 1 KNOWN HEAD PROJECT *",
        "FROM record:doc AT VALID latest KNOWN HEAD PROJECT *",
        "FROM record:doc AT VALID 1 KNOWN HEAD WHERE status open PROJECT *",
        "FROM record:doc AT VALID 1 KNOWN HEAD WHERE status ! \"open\" PROJECT *",
        "FROM record:doc AT VALID 1 KNOWN HEAD PROJECT id,",
        "FROM record:doc AT VALID 1 KNOWN HEAD PROJECT * LIMIT 0",
        "FROM record:doc AT VALID 1 KNOWN HEAD PROJECT * EXPLAIN",
        "FROM record:doc AT VALID 1 KNOWN HEAD PROJECT * GARBAGE",
        "FROM traverse:edge START node:a DIRECTION SIDEWAYS DEPTH 2 AT VALID 1 KNOWN HEAD PROJECT *",
        "FROM traverse:edge START node:a DIRECTION BOTH DEPTH 0 AT VALID 1 KNOWN HEAD PROJECT *",
        "FROM traverse:edge START node:a DIRECTION BOTH DEPTH 33 AT VALID 1 KNOWN HEAD PROJECT *",
    ] {
        let error = parse(source).expect_err(source);
        assert!(!error.message.is_empty());
        assert!(error.offset <= source.len());
    }
}

#[test]
fn strings_preserve_unicode_and_supported_escapes() {
    let query = parse(
        "FROM record:doc AT VALID 1 KNOWN HEAD WHERE title = \"Rrd \\\"α\\\"\\nline\" PROJECT title",
    )
    .unwrap();
    assert_eq!(parse(&query.canonical()).unwrap(), query);
}

#[test]
fn deterministic_mutation_corpus_never_panics() {
    let seed = "FROM record:document AT VALID 100 KNOWN HEAD WHERE status = \"open\" PROJECT id, status LIMIT 10 EXPLAIN CONTRACT";
    let alphabet = [' ', ':', ',', '=', '*', '$', '\"', '\\', 'α', '\0'];
    for index in 0..4096usize {
        let mut candidate = seed.to_owned();
        let boundary = candidate
            .char_indices()
            .nth(index % candidate.chars().count())
            .map_or(candidate.len(), |(offset, _)| offset);
        candidate.insert(boundary, alphabet[index % alphabet.len()]);
        let _ = parse(&candidate);
    }
}
