//! The M2a gate, as a test: the corpus is written, probed, and compared against what the
//! generator declared — and then attacked, because "zero crashes" is a claim about hostile
//! input and the corpus's own pathologies are only the ones somebody thought of.
//!
//! The corpus is generated into a temporary directory rather than read from
//! `testdata/media-corpus/media`, deliberately. That directory is git-ignored, so a test that
//! depended on it would pass by being skipped on every machine that had not run the generator
//! — which is the shape of a gate that measures nothing. Here the files are written, probed
//! and thrown away, so the measurement happens on every run of `cargo test`.

use std::path::PathBuf;

use maj_media::corpus::{self, Expectation};
use maj_media::{MediaBackend, NativeBackend, ProbeCache};

/// A directory of this run's own, so two test binaries cannot write over each other.
fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("maj-media-corpus-{name}"));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a scratch directory");
    directory
}

#[test]
fn every_corpus_file_probes_exactly_as_its_generator_declared() {
    let directory = scratch("declared");
    let written = corpus::write_all(&directory).expect("the corpus writes");
    let files = corpus::files();
    assert_eq!(written.len(), files.len());

    let backend = NativeBackend::new();
    let mut wrong = Vec::new();
    for (file, path) in files.iter().zip(written.iter()) {
        if let Err(difference) = file.check(&backend.probe(path)) {
            wrong.push(difference);
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} corpus files probed differently from their declaration:\n{}",
        wrong.len(),
        files.len(),
        wrong.join("\n")
    );
}

#[test]
fn the_three_gate_axes_are_read_off_the_files_that_carry_them() {
    let directory = scratch("axes");
    let written = corpus::write_all(&directory).expect("the corpus writes");
    let files = corpus::files();
    let backend = NativeBackend::new();

    let probe_of = |name: &str| {
        let (_, path) = files
            .iter()
            .zip(written.iter())
            .find(|(file, _)| file.name == name)
            .unwrap_or_else(|| panic!("{name} is in the corpus"));
        backend.probe(path).unwrap_or_else(|error| {
            panic!("{name} should be readable: {error}");
        })
    };

    // Rotation, as a quarter-turn count and as the size it displays at.
    let turned = probe_of("rotate-90-h264.mp4");
    let stream = turned.primary_video().expect("a picture");
    assert_eq!(stream.rotation.degrees(), 90);
    assert_eq!(stream.coded_size, (1_920, 1_080));
    assert_eq!(stream.display_size(), (1_080, 1_920));

    // Field order, in both directions and in the single-field spelling.
    assert!(
        probe_of("interlaced-tff-25.mov")
            .primary_video()
            .expect("a picture")
            .field_order
            .is_interlaced()
    );
    assert_eq!(
        probe_of("interlaced-bff-2997.mov")
            .primary_video()
            .expect("a picture")
            .field_order
            .key(),
        "bottom-first"
    );
    assert_eq!(
        probe_of("interlaced-single-top-field-50.mov")
            .primary_video()
            .expect("a picture")
            .field_order
            .key(),
        "single-top"
    );

    // Variable frame rate: the per-frame timestamps, exactly, and addressable by index.
    let variable = probe_of("vfr-h264-three-durations.mp4");
    let stream = variable.primary_video().expect("a picture");
    assert!(stream.rate.is_variable());
    assert_eq!(stream.rate.frames(), Some(5));
    for (index, expected) in [0_i64, 1_001, 2_002, 4_004, 6_006].iter().enumerate() {
        assert_eq!(
            stream.rate.timestamp(index as i64, stream.stream_timebase),
            Some(*expected),
            "frame {index} of a variable-rate stream"
        );
    }
    assert_eq!(
        stream.rate.timestamp(5, stream.stream_timebase),
        None,
        "a frame past the end is not invented"
    );
}

#[test]
fn nothing_in_the_corpus_crashes_the_prober_at_any_length() {
    // Every prefix of every file, at a stride that lands inside every structure each file
    // has: a header, a table, a box boundary. Truncation is the pathology a prober meets
    // most and the one a corpus of whole files cannot test, so it is manufactured here —
    // thousands of hostile files out of twenty-six honest ones.
    let directory = scratch("prefixes");
    let backend = NativeBackend::new();
    let path = directory.join("prefix.bin");
    let mut attempts = 0_u32;
    for file in corpus::files() {
        let step = (file.bytes.len() / 64).max(1);
        let mut length = 0;
        while length <= file.bytes.len() {
            std::fs::write(&path, &file.bytes[..length]).expect("write");
            // The contract is that this returns, whatever the bytes are. A panic here fails
            // the gate by unwinding, which is exactly the signal wanted.
            let _ = backend.probe(&path);
            attempts += 1;
            length += step;
        }
    }
    assert!(
        attempts > 1_000,
        "only {attempts} truncations were tried, which is not a measurement"
    );
}

#[test]
fn a_byte_flipped_anywhere_in_a_header_never_crashes_the_prober() {
    // The other half of hostile: not short, but wrong. One flipped byte at a time through
    // the first kilobyte of each file, which is where every container keeps its structure.
    let directory = scratch("flips");
    let backend = NativeBackend::new();
    let path = directory.join("flipped.bin");
    let mut attempts = 0_u32;
    for file in corpus::files() {
        let reach = file.bytes.len().min(1_024);
        for position in (0..reach).step_by(7) {
            let mut damaged = file.bytes.clone();
            damaged[position] ^= 0xFF;
            std::fs::write(&path, &damaged).expect("write");
            let _ = backend.probe(&path);
            attempts += 1;
        }
    }
    assert!(
        attempts > 1_000,
        "only {attempts} flips were tried, which is not a measurement"
    );
}

#[test]
fn the_cache_answers_the_whole_corpus_from_one_probe_each() {
    let directory = scratch("cache");
    let written = corpus::write_all(&directory).expect("the corpus writes");
    let cache = ProbeCache::new(NativeBackend::new());

    for path in &written {
        let _ = cache.container(path);
    }
    let after_first_pass = cache.misses();
    for path in &written {
        let _ = cache.container(path);
    }
    assert_eq!(
        cache.misses(),
        after_first_pass,
        "the second pass over the corpus went to the backend again"
    );
    assert_eq!(cache.hits() as usize, written.len());
}

#[test]
fn a_damaged_file_and_an_absent_one_are_told_apart_without_a_decode() {
    let directory = scratch("labels");
    let written = corpus::write_all(&directory).expect("the corpus writes");
    let files = corpus::files();
    let cache = ProbeCache::new(NativeBackend::new());

    // The three labels, from the three conditions, over the corpus's own pathologies.
    let damaged = files
        .iter()
        .zip(written.iter())
        .filter(|(file, _)| matches!(file.expect, Expectation::Refused("damaged")))
        .count();
    assert!(damaged >= 4, "only {damaged} damaged files in the corpus");

    for (file, path) in files.iter().zip(written.iter()) {
        if let Expectation::Refused(label) = file.expect {
            assert_eq!(
                cache.container(path).expect_err("refused").label(),
                label,
                "{}",
                file.name
            );
        }
    }

    // The same file, removed: present-but-damaged and absent are different answers.
    let (_, damaged_path) = files
        .iter()
        .zip(written.iter())
        .find(|(file, _)| file.name == "png-failing-checksum.png")
        .expect("the damaged still");
    assert_eq!(
        cache.container(damaged_path).expect_err("damaged").label(),
        "damaged"
    );
    std::fs::remove_file(damaged_path).expect("remove");
    assert_eq!(
        cache.container(damaged_path).expect_err("offline").label(),
        "offline",
        "a file that was damaged and is now gone has to change its answer"
    );
}

#[test]
fn the_corpus_reports_its_own_size_and_what_it_does_not_cover() {
    let files = corpus::files();
    let covered = corpus::covered_axes();
    // Printed rather than only asserted: this is the number the phase has to state, and a
    // gate that states it is better than a note that repeats it.
    println!("corpus: {} files, {} axes", files.len(), covered.len());
    for (axis, why) in corpus::NOT_COVERED {
        println!("not covered: {axis} — {why}");
    }
    assert!(files.len() >= 20);
    assert!(!corpus::NOT_COVERED.is_empty());
}
