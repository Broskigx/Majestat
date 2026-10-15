//! Deterministic property tests for layer 0.
//!
//! Every case sequence is seeded from a fixed constant, so a failure reported here
//! is reproducible byte-for-byte on any machine. See `maj_time::testing` for why
//! this project does not use a randomised property-testing harness.
//!
//! Run with `cargo test -p maj-time --test properties`.

use maj_time::testing::DeterministicRng;
use maj_time::{FrameRange, Rational, RetimeMap, TimeError, Timebase, Timecode, TimecodeMode};

/// Number of sampled cases per property. Chosen so the whole file runs in well under
/// a second while still covering a wide slice of each input domain.
const CASES: usize = 20_000;

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

fn random_rational(rng: &mut DeterministicRng) -> Rational {
    Rational::new(rng.next_i64_in(-10_000, 10_000), rng.next_i64_in(1, 10_000)).unwrap()
}

fn all_timebases() -> [Timebase; 8] {
    [
        Timebase::FPS_23_976,
        Timebase::FPS_24,
        Timebase::FPS_25,
        Timebase::FPS_29_97,
        Timebase::FPS_30,
        Timebase::FPS_50,
        Timebase::FPS_59_94,
        Timebase::FPS_60,
    ]
}

#[test]
fn rationals_are_always_canonical() {
    let mut rng = DeterministicRng::new(0x00C0_FFEE);
    for case in 0..CASES {
        let num = rng.next_i64_in(-1_000_000, 1_000_000);
        let den = rng.next_i64_in(-1_000, 1_000);
        if den == 0 {
            assert_eq!(Rational::new(num, den), Err(TimeError::ZeroDenominator));
            continue;
        }
        let r = Rational::new(num, den).unwrap();
        assert!(r.den() > 0, "case {case}: denominator must be positive");
        assert_eq!(
            gcd(r.num().unsigned_abs(), r.den() as u64),
            1,
            "case {case}: {r} is not reduced"
        );
        if r.num() == 0 {
            assert_eq!(r.den(), 1, "case {case}: zero must be 0/1");
        }
    }
}

#[test]
fn reduction_preserves_value() {
    let mut rng = DeterministicRng::new(0x1234_5678);
    for _ in 0..CASES {
        let num = rng.next_i64_in(-100_000, 100_000);
        let den = rng.next_i64_in(1, 1_000);
        let r = Rational::new(num, den).unwrap();
        // Cross-multiply against the original ratio without any float involvement.
        assert_eq!(
            i128::from(r.num()) * i128::from(den),
            i128::from(num) * i128::from(r.den()),
            "reduction changed the value of {num}/{den}"
        );
    }
}

#[test]
fn addition_and_subtraction_are_exact_inverses() {
    let mut rng = DeterministicRng::new(0xAAAA_BBBB);
    for _ in 0..CASES {
        let a = random_rational(&mut rng);
        let b = random_rational(&mut rng);
        let sum = a.checked_add(b).unwrap();
        assert_eq!(sum.checked_sub(b).unwrap(), a, "({a} + {b}) - {b} != {a}");
        assert_eq!(a.checked_add(b).unwrap(), b.checked_add(a).unwrap());
    }
}

#[test]
fn multiplication_by_the_reciprocal_gives_one() {
    let mut rng = DeterministicRng::new(0xDEAD_BEEF);
    for _ in 0..CASES {
        let a = random_rational(&mut rng);
        if a.is_zero() {
            continue;
        }
        let product = a.checked_mul(a.checked_recip().unwrap()).unwrap();
        assert_eq!(product, Rational::from_integer(1), "case {a}");
    }
}

#[test]
fn ordering_agrees_with_exact_cross_multiplication() {
    let mut rng = DeterministicRng::new(0x0F0F_0F0F);
    for _ in 0..CASES {
        let a = random_rational(&mut rng);
        let b = random_rational(&mut rng);
        let lhs = i128::from(a.num()) * i128::from(b.den());
        let rhs = i128::from(b.num()) * i128::from(a.den());
        assert_eq!(
            a.cmp(&b),
            lhs.cmp(&rhs),
            "ordering disagreed for {a} vs {b}"
        );
    }
}

#[test]
fn floor_obeys_the_floor_inequality() {
    let mut rng = DeterministicRng::new(0x5555_AAAA);
    for _ in 0..CASES {
        let num = rng.next_i64_in(-1_000_000, 1_000_000);
        let den = rng.next_i64_in(1, 1_000);
        let r = Rational::new(num, den).unwrap();
        let floor = r.floor().unwrap();
        assert!(
            i128::from(floor) * i128::from(r.den()) <= i128::from(r.num()),
            "floor({r}) = {floor} is too high"
        );
        assert!(
            (i128::from(floor) + 1) * i128::from(r.den()) > i128::from(r.num()),
            "floor({r}) = {floor} is too low"
        );
    }
}

#[test]
fn non_drop_timecode_round_trips_at_every_rate() {
    let mut rng = DeterministicRng::new(0x1111_2222);
    for timebase in all_timebases() {
        let nominal = i64::from(timebase.nominal_fps());
        // 24 hours of frames at this rate.
        let ceiling = nominal * 60 * 60 * 24;
        for _ in 0..(CASES / 8) {
            let frames = rng.next_i64_in(0, ceiling);
            let tc = Timecode::from_frames(frames, timebase, TimecodeMode::NonDrop).unwrap();
            assert_eq!(
                tc.to_frames(timebase).unwrap(),
                frames,
                "non-drop round trip failed at {frames} on {timebase} ({tc})"
            );
        }
    }
}

#[test]
fn drop_frame_timecode_round_trips_at_both_ntsc_rates() {
    let mut rng = DeterministicRng::new(0x3333_4444);
    for timebase in [Timebase::FPS_29_97, Timebase::FPS_59_94] {
        let nominal = i64::from(timebase.nominal_fps());
        let ceiling = nominal * 60 * 60 * 24;
        for _ in 0..CASES {
            let frames = rng.next_i64_in(0, ceiling);
            let tc = Timecode::from_frames(frames, timebase, TimecodeMode::Drop).unwrap();
            assert_eq!(
                tc.to_frames(timebase).unwrap(),
                frames,
                "drop-frame round trip failed at {frames} on {timebase} ({tc})"
            );
        }
    }
}

#[test]
fn drop_frame_labels_are_never_ambiguous_near_a_minute_boundary() {
    let timebase = Timebase::FPS_29_97;
    for minute in 1..60u32 {
        // The tenth minute is the one where drop-frame omits nothing.
        if minute % 10 == 0 {
            continue;
        }
        // The first label that exists at this minute is ;02.
        let first = Timecode::from_hmsf(0, minute, 0, 2, true).unwrap();
        let frames = first.to_frames(timebase).unwrap();
        assert_eq!(
            Timecode::from_frames(frames, timebase, TimecodeMode::Drop).unwrap(),
            first,
            "minute {minute} did not round trip from its first valid label"
        );

        // ;00 and ;01 must be unreachable labels: they map to the previous minute's
        // tail, so converting back does not reproduce them.
        for omitted_frames_field in [0u32, 1] {
            let omitted = Timecode::from_hmsf(0, minute, 0, omitted_frames_field, true).unwrap();
            let as_frames = omitted.to_frames(timebase).unwrap();
            let relabelled =
                Timecode::from_frames(as_frames, timebase, TimecodeMode::Drop).unwrap();
            assert_ne!(
                relabelled, omitted,
                "minute {minute} label {omitted} exists but drop-frame omits it"
            );
        }
    }
}

#[test]
fn timecode_parsing_round_trips_through_display() {
    let mut rng = DeterministicRng::new(0x9999_8888);
    for timebase in all_timebases() {
        let nominal = i64::from(timebase.nominal_fps());
        let mode = TimecodeMode::preferred_for(timebase);
        for _ in 0..(CASES / 8) {
            let frames = rng.next_i64_in(0, nominal * 3_600 * 24);
            let tc = Timecode::from_frames(frames, timebase, mode).unwrap();
            let rendered = tc.to_string();
            let reparsed: Timecode = rendered
                .parse()
                .unwrap_or_else(|_| panic!("failed to reparse {rendered}"));
            assert_eq!(reparsed, tc, "display/parse changed {rendered}");
            assert_eq!(reparsed.to_frames(timebase).unwrap(), frames);
        }
    }
}

#[test]
fn frame_ranges_obey_set_laws() {
    let mut rng = DeterministicRng::new(0x7777_6666);
    for _ in 0..CASES {
        let a = FrameRange::new(
            rng.next_i64_in(-5_000, 5_000),
            rng.next_i64_in(-5_000, 5_000),
        );
        let b = FrameRange::new(
            rng.next_i64_in(-5_000, 5_000),
            rng.next_i64_in(-5_000, 5_000),
        );
        let (Ok(a), Ok(b)) = (a, b) else { continue };

        assert_eq!(a.len(), a.end() - a.start());
        assert_eq!(
            a.intersects(b),
            b.intersects(a),
            "intersects is not symmetric"
        );
        assert_eq!(
            a.intersects(b),
            a.intersection(b).is_some(),
            "intersects and intersection disagree for {a} and {b}"
        );
        assert_eq!(
            a.intersection(b),
            b.intersection(a),
            "intersection is not commutative"
        );
        assert_eq!(a.union_hull(b), b.union_hull(a), "hull is not commutative");
        assert!(a.union_hull(b).contains_range(a));
        assert!(a.union_hull(b).contains_range(b));

        if let Some(shared) = a.intersection(b) {
            assert!(shared.len() <= a.len().min(b.len()));
            assert!(a.contains(shared.start()) && b.contains(shared.start()));
        }
    }
}

#[test]
fn adjacent_ranges_never_intersect() {
    let mut rng = DeterministicRng::new(0x4646_4646);
    for _ in 0..CASES {
        let start = rng.next_i64_in(-1_000, 1_000);
        let len = rng.next_i64_in(0, 1_000);
        let a = FrameRange::from_start_len(start, len).unwrap();
        let b = FrameRange::from_start_len(a.end(), rng.next_i64_in(0, 1_000)).unwrap();
        assert!(a.is_adjacent(b));
        assert!(!a.intersects(b), "{a} and {b} illegally share a frame");
        assert_eq!(a.intersection(b), None);
        if !a.is_empty() && !b.is_empty() {
            assert_eq!(a.union_hull(b).len(), a.len() + b.len());
        }
    }
}

#[test]
fn retiming_is_monotonic_for_every_speed() {
    let mut rng = DeterministicRng::new(0x2222_3333);
    for _ in 0..2_000 {
        let speed_num = rng.next_i64_in(1, 400);
        let speed_den = rng.next_i64_in(1, 400);
        let speed = Rational::new(speed_num, speed_den).unwrap();
        let map = RetimeMap::new(
            rng.next_i64_in(-1_000, 1_000),
            rng.next_i64_in(-1_000, 1_000),
            speed,
        )
        .unwrap();

        let base = rng.next_i64_in(-500, 0);
        let mut previous = map.source_at(base);
        for timeline in base..(base + 500) {
            let current = map.source_at(timeline);
            assert!(
                current >= previous,
                "monotonicity broke at {timeline} with speed {speed}"
            );
            let step = i128::from(current) - i128::from(previous);
            assert!(
                step <= i128::from(speed.num()),
                "a single timeline frame advanced more than the speed allows"
            );
            previous = current;
        }
    }
}

#[test]
fn retiming_round_trips_for_whole_number_speeds() {
    let mut rng = DeterministicRng::new(0x8888_9999);
    for _ in 0..CASES {
        let speed = Rational::from_integer(rng.next_i64_in(1, 64));
        let map = RetimeMap::new(
            rng.next_i64_in(-10_000, 10_000),
            rng.next_i64_in(-10_000, 10_000),
            speed,
        )
        .unwrap();
        assert!(map.is_one_to_one());
        for _ in 0..8 {
            let timeline = rng.next_i64_in(-5_000, 5_000);
            assert_eq!(
                map.timeline_at(map.source_at(timeline)),
                timeline,
                "round trip failed at {timeline} with speed {speed}"
            );
        }
    }
}

#[test]
fn identity_retiming_never_moves_a_frame() {
    let mut rng = DeterministicRng::new(0xBBBB_CCCC);
    for _ in 0..CASES {
        let frame = rng.next_i64_in(-1_000_000, 1_000_000);
        assert_eq!(RetimeMap::identity().source_at(frame), frame);
        let anchored = RetimeMap::identity_at(frame);
        assert_eq!(anchored.source_at(frame), frame);
        assert!(anchored.is_identity());
    }
}

#[test]
fn source_ranges_are_never_shorter_than_they_should_be() {
    let mut rng = DeterministicRng::new(0x1212_1212);
    for _ in 0..2_000 {
        let speed = Rational::new(rng.next_i64_in(1, 8), rng.next_i64_in(1, 8)).unwrap();
        let map = RetimeMap::new(0, 0, speed).unwrap();
        let len = rng.next_i64_in(1, 500);
        let timeline = FrameRange::from_start_len(rng.next_i64_in(0, 500), len).unwrap();
        let source = map.source_range_for(timeline);

        if map.is_identity() && map.source_start() == 0 {
            assert_eq!(source.len(), timeline.len());
        } else {
            assert!(source.len() <= timeline.len() * speed.num());
            assert!(!source.is_empty());
        }
        // Every frame of the timeline range must land inside the reported source
        // range, or the range is wrong and a render would read the wrong pixels.
        for timeline_frame in timeline.start()..timeline.end() {
            assert!(
                source.contains(map.source_at(timeline_frame)),
                "source range {source} misses frame {} from timeline {timeline_frame}",
                map.source_at(timeline_frame)
            );
        }
    }
}

#[test]
fn the_test_prng_itself_replays() {
    let mut first = DeterministicRng::new(0xFFFF_0000);
    let mut second = DeterministicRng::new(0xFFFF_0000);
    for _ in 0..1_000 {
        assert_eq!(first.next_u64(), second.next_u64());
    }
}
