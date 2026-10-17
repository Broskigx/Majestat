//! The invariant the adapter exists to keep, measured over the whole synthetic corpus.
//!
//! `PictureResolver::kind` names the source of a layer the manifest is describing but not
//! rendering. If it ever names something other than what `resolve` would produce, a render
//! manifest says one thing about a file and the frame beside it says another — which is
//! precisely the defect `.planning/STATE.md` recorded against a present-but-damaged still.
//!
//! So this test does not check that `kind` is *right* in isolation. It checks that `kind` and
//! `resolve` are the **same answer**, for every file in the corpus, under both policies.

use maj_media::NativeBackend;
use maj_media::corpus::{self, Expectation};
use maj_render::{MediaResolver, MissingMedia, PictureResolver, Placeholder, WorkspaceResolver};

/// A directory of this run's own.
fn scratch(name: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!("maj-render-resolver-{name}"));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a scratch directory");
    directory
}

#[test]
fn the_manifest_label_and_the_frame_agree_on_every_corpus_file() {
    let directory = scratch("agreement");
    let written = corpus::write_all(&directory).expect("the corpus writes");
    let resolver = MediaResolver::new(NativeBackend::new());

    let mut disagreements = Vec::new();
    for path in &written {
        let uri = path.to_string_lossy().to_string();
        let named = resolver.kind(&uri);
        let drawn = resolver
            .resolve(&uri, (64, 64), 0)
            .expect("the placeholder policy draws something for every condition");
        if named != drawn.kind() {
            disagreements.push(format!(
                "{}: the manifest would say `{named}` and the frame says `{}`",
                path.display(),
                drawn.kind()
            ));
        }
    }
    assert!(
        disagreements.is_empty(),
        "{} of {} files are named one thing and drawn another:\n{}",
        disagreements.len(),
        written.len(),
        disagreements.join("\n")
    );
}

#[test]
fn every_label_is_one_of_the_three_or_a_container_this_build_reads() {
    let directory = scratch("vocabulary");
    let written = corpus::write_all(&directory).expect("the corpus writes");
    let resolver = MediaResolver::new(NativeBackend::new());

    // The closed vocabulary, which is the reason `ContainerKind::key` is a `const fn` over an
    // enum: a manifest's words are finite and documented, and nothing leaks out of a parser.
    let allowed = [
        "png",
        "ppm",
        Placeholder::OFFLINE,
        Placeholder::DAMAGED,
        Placeholder::UNDECODABLE,
    ];
    for path in &written {
        let named = resolver.kind(&path.to_string_lossy());
        assert!(
            allowed.contains(&named),
            "{} was named `{named}`, which is not in the manifest's vocabulary",
            path.display()
        );
    }
}

#[test]
fn a_delivery_refuses_exactly_the_files_an_editor_would_have_drawn_a_placeholder_for() {
    let directory = scratch("strict");
    let written = corpus::write_all(&directory).expect("the corpus writes");
    let lenient = MediaResolver::new(NativeBackend::new());
    let strict = MediaResolver::with_stills(
        WorkspaceResolver::new().with_missing_media(MissingMedia::Refuse),
        NativeBackend::new(),
    );

    for path in &written {
        let uri = path.to_string_lossy().to_string();
        let drawn = lenient.resolve(&uri, (64, 64), 0).expect("something");
        let refused = strict.resolve(&uri, (64, 64), 0);
        let was_placeholder = Placeholder::KINDS.contains(&drawn.kind());
        assert_eq!(
            was_placeholder,
            refused.is_err(),
            "{}: an editor drew `{}` and a delivery {}",
            path.display(),
            drawn.kind(),
            if refused.is_err() {
                "refused it"
            } else {
                "accepted it"
            }
        );
        if was_placeholder {
            assert_eq!(strict.kind(&uri), "-", "a refusing resolver names nothing");
        }
    }
}

#[test]
fn the_three_conditions_are_told_apart_over_the_corpus_pathologies() {
    let directory = scratch("conditions");
    let written = corpus::write_all(&directory).expect("the corpus writes");
    let files = corpus::files();
    let resolver = MediaResolver::new(NativeBackend::new());

    let mut labels = std::collections::BTreeMap::new();
    // Absence is not a file, so the corpus cannot hold one: `offline` is the condition of a
    // path with nothing at it, and it is tested by naming one.
    let absent = directory.join("moved-to-another-drive.mov");
    labels.insert(resolver.kind(&absent.to_string_lossy()), 1_usize);
    for (file, path) in files.iter().zip(written.iter()) {
        let named = resolver.kind(&path.to_string_lossy());
        *labels.entry(named).or_insert(0_usize) += 1;
        if let Expectation::Refused("damaged") = file.expect {
            assert_eq!(
                named, "damaged",
                "{} is present and damaged, and has to be named so",
                file.name
            );
        }
    }
    // All three labels occur, which is what makes the distinction a measurement rather than a
    // definition: a resolver that answered `undecodable` for everything would pass a weaker
    // test than this one.
    for label in Placeholder::KINDS {
        assert!(
            labels.get(label).copied().unwrap_or(0) > 0,
            "no corpus file was named `{label}`, so the distinction is untested"
        );
    }
    assert!(labels.get("png").copied().unwrap_or(0) >= 2);
    assert!(labels.get("ppm").copied().unwrap_or(0) >= 2);
}

#[test]
fn a_file_that_goes_away_changes_its_answer_and_one_that_comes_back_changes_it_again() {
    let directory = scratch("moving");
    let still = corpus::files()
        .into_iter()
        .find(|file| file.name == "still-rgb-8bit.png")
        .expect("the corpus has a still");
    let path = directory.join(still.name);
    std::fs::write(&path, &still.bytes).expect("write");

    let resolver = MediaResolver::new(NativeBackend::new());
    let uri = path.to_string_lossy().to_string();
    assert_eq!(resolver.kind(&uri), "png");
    std::fs::remove_file(&path).expect("remove");
    assert_eq!(
        resolver.kind(&uri),
        "offline",
        "a cached answer must not outlive its file"
    );
    std::fs::write(&path, &still.bytes).expect("rewrite");
    assert_eq!(
        resolver.kind(&uri),
        "png",
        "and a relinked file must be noticed"
    );
}
