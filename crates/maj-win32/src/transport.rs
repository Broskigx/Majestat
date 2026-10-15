//! The transport clock: the part of playback that exists without a playback engine.
//!
//! There is no decoder and no audio until M2, M3 and M9, so this is not playback of media. What
//! it is, exactly: a clock that advances the playhead at the sequence's own rate, re-renders the
//! preview through the certified path, and counts how many frames a second that actually
//! manages. The status bar says which of those things it is, and `docs/UI.md` says it again, so
//! a moving picture is never mistaken for a decoded one.
//!
//! The clock is driven by the window's timer, and the only thing it does to the document is
//! move the playhead — which is a view action, so a minute of playing appends nothing to the
//! command log.

/// The rates `J` and `L` walk through, in multiples of real time.
const RATES: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// How often the preview is allowed to be re-rendered while the clock runs. The timeline repaints
/// every tick; the frame does not, because a frame costs a render and the eye does not notice.
const PREVIEW_INTERVAL: f64 = 1.0 / 30.0;

/// How long the frames-per-second counter averages over.
const SAMPLE_SECONDS: f64 = 0.5;

/// The clock, and what has been measured of it.
#[derive(Debug)]
pub(crate) struct Transport {
    /// Whether the clock is running.
    pub(crate) playing: bool,
    /// How fast, in multiples of real time. Negative runs backwards.
    pub(crate) rate: f64,
    /// Whether reaching the end starts again.
    pub(crate) loops: bool,
    /// The frame the clock was started from.
    anchor_frame: i64,
    /// The moment it was started.
    anchor_time: f64,
    /// The sequence's rate, in frames per second.
    fps: f64,
    /// When the preview was last refreshed.
    last_preview: f64,
    /// How many previews have been drawn in this sample.
    frames: u32,
    /// When this sample began.
    sample_time: f64,
    /// The measured rate, in preview frames per second.
    pub(crate) measured_fps: f64,
    /// How many times the clock has been asked for a frame, for the report.
    pub(crate) ticks: u64,
}

impl Transport {
    /// A stopped clock.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            playing: false,
            rate: 1.0,
            loops: false,
            anchor_frame: 0,
            anchor_time: 0.0,
            fps: 24.0,
            last_preview: 0.0,
            frames: 0,
            sample_time: 0.0,
            measured_fps: 0.0,
            ticks: 0,
        }
    }

    /// Starts the clock from a frame, at a rate.
    pub(crate) fn play(&mut self, frame: i64, now: f64, fps: f64) {
        self.anchor_frame = frame;
        self.anchor_time = now;
        self.fps = if fps.is_finite() && fps > 0.0 {
            fps
        } else {
            24.0
        };
        self.playing = true;
        self.last_preview = now - PREVIEW_INTERVAL;
        self.sample_time = now;
        self.frames = 0;
    }

    /// Stops it where it is.
    pub(crate) fn stop(&mut self) {
        self.playing = false;
        self.rate = 1.0;
        self.measured_fps = 0.0;
    }

    /// Steps the rate up or down, which is what `J` and `L` do. Starting from a stopped clock
    /// starts it, which is what makes one press of `L` play forwards.
    pub(crate) fn shuttle(&mut self, direction: i64, frame: i64, now: f64, fps: f64) {
        let current = RATES
            .iter()
            .position(|rate| (rate - self.rate.abs()).abs() < f64::EPSILON)
            .unwrap_or(2);
        let next = if direction > 0 {
            (current + 1).min(RATES.len() - 1)
        } else {
            current.saturating_sub(1)
        };
        let rate = RATES[next] * if direction < 0 { -1.0 } else { 1.0 };
        if !self.playing {
            self.play(frame, now, fps);
        }
        self.rate = rate;
        // A change of rate re-anchors the clock, so the playhead does not jump by the whole
        // difference between the old rate and the new one.
        self.anchor_frame = frame;
        self.anchor_time = now;
    }

    /// The frame the clock is at, or `None` while it is stopped.
    ///
    /// A clock that runs past either end stops at it, unless it is looping.
    pub(crate) fn frame(&mut self, now: f64, end: i64) -> Option<i64> {
        if !self.playing {
            return None;
        }
        self.ticks += 1;
        let elapsed = now - self.anchor_time;
        let travelled = (elapsed * self.rate * self.fps).round() as i64;
        let target = self.anchor_frame.saturating_add(travelled);
        if target < 0 || target > end {
            if self.loops {
                let span = end.max(1);
                let wrapped = target.rem_euclid(span);
                self.anchor_frame = wrapped;
                self.anchor_time = now;
                return Some(wrapped);
            }
            self.playing = false;
            return Some(if target < 0 { 0 } else { end });
        }
        Some(target)
    }

    /// Whether the preview should be re-rendered now, and records the frame if so.
    pub(crate) fn take_preview(&mut self, now: f64) -> bool {
        if now - self.last_preview < PREVIEW_INTERVAL {
            return false;
        }
        self.last_preview = now;
        self.frames += 1;
        if now - self.sample_time >= SAMPLE_SECONDS {
            self.measured_fps = f64::from(self.frames) / (now - self.sample_time);
            self.frames = 0;
            self.sample_time = now;
        }
        true
    }

    /// How the transport is doing, in the shell's own words being added around it.
    #[must_use]
    pub(crate) fn report(&self) -> String {
        format!(
            "{}x · {:.1} fps",
            self.rate,
            if self.playing { self.measured_fps } else { 0.0 }
        )
    }
}
