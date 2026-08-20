use vyrm_core::{Claim, Predicate, Producer, Subject};
use vyrm_store::{Engine, PersistentBackend, PersistentEngine, Store};

fn claim(object: &str) -> Claim {
    Claim::new(
        Subject::new("default-engine").unwrap(),
        Predicate::new("status").unwrap(),
        object,
        1,
        1,
        Producer {
            actor: "test".into(),
            on_behalf_of: None,
            session: None,
        },
    )
}

#[test]
fn missing_paths_default_to_native_and_reopen_by_authenticated_marker() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("nested").join("store");
    let engine = PersistentEngine::open(&path).unwrap();
    assert_eq!(engine.backend(), PersistentBackend::Native);
    assert!(path.join("CURRENT").is_file());
    Engine::append_batch(&engine, &[claim("native")]).unwrap();
    drop(engine);

    let reopened = PersistentEngine::open(&path).unwrap();
    assert_eq!(reopened.backend(), PersistentBackend::Native);
    assert_eq!(Engine::sequence(&reopened).unwrap(), 1);
}

#[test]
fn existing_empty_directories_are_unclaimed_and_initialize_as_native() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("empty-store");
    std::fs::create_dir(&path).unwrap();
    let engine = PersistentEngine::open(&path).unwrap();
    assert_eq!(engine.backend(), PersistentBackend::Native);
    assert!(path.join("CURRENT").is_file());
}

#[test]
fn partial_native_identity_fails_closed_instead_of_opening_fjall() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("partial-native");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("MANIFEST.LOCK"), []).unwrap();
    assert!(PersistentEngine::open(&path).is_err());
}

#[test]
fn existing_fjall_directories_remain_on_the_compatibility_adapter() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("store");
    let fjall = Store::open(&path).unwrap();
    Engine::append_batch(&fjall, &[claim("fjall")]).unwrap();
    drop(fjall);

    let reopened = PersistentEngine::open(&path).unwrap();
    assert_eq!(reopened.backend(), PersistentBackend::FjallCompatibility);
    assert_eq!(Engine::sequence(&reopened).unwrap(), 1);
}
