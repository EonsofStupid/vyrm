//! Filename-level entities: definition sites that exist as names in the tree
//! rather than as declaration lines. A module file declares its stem, an entry
//! file declares its directory, a Svelte component declares its filename, and
//! a real declaration line is never shadowed by the synthesized one.

use vyrm_graph::{Index, Profile};

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("package.json"), "{}").unwrap();
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

#[test]
fn a_module_file_is_the_definition_site_of_its_stem() {
    // Nothing inside terminology.ts is named terminology — the reference
    // repository's actual shape for this query class.
    let dir = project(&[
        ("src/utils/terminology.ts", "export function translateQuant(): string { return \"\"; }"),
        ("src/caller.ts", "import { translateQuant } from \"./utils/terminology\";\nterminology;"),
    ]);
    let index = indexed(&dir);

    let routed = index.route("terminology", 5);
    assert!(
        !routed.is_empty() && routed[0].path.ends_with("src/utils/terminology.ts"),
        "the module file must rank first as definer"
    );
    assert!(
        !routed[0].justification.defines.is_empty(),
        "the stem must be a definition, not a reference"
    );
}

#[test]
fn an_entry_file_declares_its_directory_not_its_stem() {
    let dir = project(&[(
        "src/widgets/index.ts",
        "export function makeWidget(): number { return 1; }",
    )]);
    let index = indexed(&dir);

    let routed = index.route("widgets", 5);
    assert!(
        !routed.is_empty()
            && routed[0].path.ends_with("src/widgets/index.ts")
            && !routed[0].justification.defines.is_empty(),
        "index.ts must define its directory name"
    );
    assert!(
        index.route("index", 5).iter().all(|r| r.justification.defines.is_empty()),
        "the meaningless stem `index` must not become a definition"
    );
}

#[test]
fn a_compound_extension_declares_the_first_stem_segment() {
    let dir = project(&[("src/runes/hardwareState.svelte.ts", "export const state = 1;")]);
    let index = indexed(&dir);

    let routed = index.route("hardwareState", 5);
    assert!(
        !routed.is_empty() && !routed[0].justification.defines.is_empty(),
        "hardwareState.svelte.ts must define hardwareState"
    );
}

#[test]
fn a_non_identifier_stem_declares_nothing() {
    let dir = project(&[("src/routes/+page.svelte", "<script lang=\"ts\">let x = 1;</script>")]);
    let index = indexed(&dir);

    assert!(
        index.route("+page", 5).is_empty(),
        "a route-syntax stem must not become an entity"
    );
}

#[test]
fn a_real_declaration_is_not_shadowed_by_the_synthesized_line() {
    // parser.ts declares `parser` on line 3. The synthesized filename entity
    // must yield to it: exactly one definition, at the real line.
    let dir = project(&[(
        "src/parser.ts",
        "// header\n// more header\nexport const parser = 1;",
    )]);
    let index = indexed(&dir);

    let routed = index.route("parser", 5);
    assert_eq!(routed[0].justification.defines.len(), 1, "duplicate definition injected");
}
