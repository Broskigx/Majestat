use core::{fmt, str::FromStr};

use crate::{TimeError, Timebase};

/// Which timecode convention to use when converting frames.
///
/// Drop-frame exists because `30000/1001` fps is not 30 fps: without dropping frame
/// *numbers* (never frame *content*), timecode would drift from wall-clock time at
/// about 3.6 seconds per hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimecodeMode {
    /// Sequential frame numbers with no gaps. Uses the nominal frame rate as the
    /// divisor.
    NonDrop,
    /// Skips frame numbers at the start of each minute, except every tenth minute.
    Drop,
}

impl TimecodeMode {
    /// The convention a rate is normally read with: drop-frame for the two NTSC
    /// rates that define it, non-drop otherwise.
    #[must_use]
    pub const fn preferred_for(timebase: Timebase) -> Self {
        if timebase.drop_frame_frames().is_some() {
            Self::Drop
        } else {
            Self::NonDrop
        }
    }
}

/// A SMPTE-style timecode position, `HH:MM:SS:FF` or `HH:MM:SS;FF`.
///
/// A timecode carries no rate of its own. Converting to and from frames always
/// requires a [`Timebase`], which is what makes a stored timecode unambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Timecode {
    hours: u32,
    minutes: u32,
    seconds: u32,
    frames: u32,
    drop_frame: bool,
}

/// Splits an adjusted frame count into hours, minutes, seconds and frames.
fn decompose(frames: i64, nominal: i64, drop_frame: bool) -> Result<Timecode, TimeError> {
    let frames_field = frames % nominal;
    let total_seconds = frames / nominal;
    let seconds = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    let minutes = total_minutes % 60;
    let hours = total_minutes / 60;
    Ok(Timecode {
        hours: u32::try_from(hours).map_err(|_| TimeError::Overflow)?,
        minutes: u32::try_from(minutes).map_err(|_| TimeError::Overflow)?,
        seconds: u32::try_from(seconds).map_err(|_| TimeError::Overflow)?,
        frames: u32::try_from(frames_field).map_err(|_| TimeError::Overflow)?,
        drop_frame,
    })
}

/// Converts a real drop-frame frame count into the number it is labelled with.
///
/// Drop-frame timecode omits the frame numbers `:00` and `:01` at the start of every
/// minute except every tenth minute. The number printed is therefore higher than the
/// index, by exactly the count of omitted numbers so far.
fn drop_frame_label(frames: i64, nominal: i64, drop: i64) -> Result<i64, TimeError> {
    let per_ten_minutes = nominal * 600 - drop * 9;
    let per_minute = nominal * 60 - drop;
    let tens = frames / per_ten_minutes;
    let remainder = frames % per_ten_minutes;
    let extra = drop * (remainder.saturating_sub(drop) / per_minute);
    frames
        .checked_add(drop * 9 * tens)
        .and_then(|v| v.checked_add(extra))
        .ok_or(TimeError::Overflow)
}

/// Parses one numeric field, reporting the whole input on failure.
fn parse_field(input: &str, text: &str) -> Result<u32, TimeError> {
    text.trim()
        .parse::<u32>()
        .map_err(|_| TimeError::TimecodeParse {
            input: input.to_owned(),
        })
}

impl Timecode {
    /// The `HH:MM:SS:FF` separator for non-drop timecode.
    pub const NON_DROP_SEPARATOR: char = ':';
    /// The `HH:MM:SS;FF` separator for drop-frame timecode.
    pub const DROP_SEPARATOR: char = ';';

    /// Converts a frame index into timecode under `timebase`.
    ///
    /// Returns [`TimeError::NegativeFrames`] for a negative index,
    /// [`TimeError::DropFrameUnsupported`] when `mode` is [`TimecodeMode::Drop`] but
    /// the rate defines no drop rule, and [`TimeError::Overflow`] for absurd inputs.
    ///
    /// ```
    /// use maj_time::{Timecode, TimecodeMode, Timebase};
    /// let tb = Timebase::FPS_29_97;
    /// let tc = Timecode::from_frames(1_798, tb, TimecodeMode::Drop).unwrap();
    /// assert_eq!(tc.to_string(), "00:00:59;28");
    /// assert_eq!(tc.to_frames(tb).unwrap(), 1_798);
    /// ```
    pub fn from_frames(
        frames: i64,
        timebase: Timebase,
        mode: TimecodeMode,
    ) -> Result<Self, TimeError> {
        if frames < 0 {
            return Err(TimeError::NegativeFrames { frames });
        }
        let nominal = i64::from(timebase.nominal_fps());
        if nominal <= 0 {
            return Err(TimeError::NonPositiveTimebase);
        }
        let drop = match mode {
            TimecodeMode::NonDrop => 0,
            TimecodeMode::Drop => {
                let drop = timebase
                    .drop_frame_frames()
                    .ok_or(TimeError::DropFrameUnsupported {
                        num: timebase.numerator(),
                        den: timebase.denominator(),
                    })?;
                i64::from(drop)
            }
        };
        let labelled = if drop == 0 {
            frames
        } else {
            drop_frame_label(frames, nominal, drop)?
        };
        decompose(labelled, nominal, drop > 0)
    }

    /// Converts a frame index using the rate's conventional mode.
    ///
    /// Drop-frame for `30000/1001` and `60000/1001`, non-drop otherwise.
    pub fn from_frames_preferred(frames: i64, timebase: Timebase) -> Result<Self, TimeError> {
        Self::from_frames(frames, timebase, TimecodeMode::preferred_for(timebase))
    }

    /// Builds a timecode from its four fields.
    ///
    /// Structural bounds are checked here (`minutes < 60`, `seconds < 60`). The
    /// `frames` field is checked against the rate in [`Timecode::to_frames`],
    /// because a frame number is only meaningful relative to a rate.
    pub fn from_hmsf(
        hours: u32,
        minutes: u32,
        seconds: u32,
        frames: u32,
        drop_frame: bool,
    ) -> Result<Self, TimeError> {
        if minutes >= 60 {
            return Err(TimeError::InvalidTimecodeField {
                field: "minutes",
                value: i64::from(minutes),
            });
        }
        if seconds >= 60 {
            return Err(TimeError::InvalidTimecodeField {
                field: "seconds",
                value: i64::from(seconds),
            });
        }
        Ok(Self {
            hours,
            minutes,
            seconds,
            frames,
            drop_frame,
        })
    }

    /// Converts the timecode back into a frame index under `timebase`.
    ///
    /// This is the exact inverse of [`Timecode::from_frames`] for the same mode.
    pub fn to_frames(self, timebase: Timebase) -> Result<i64, TimeError> {
        let nominal = i64::from(timebase.nominal_fps());
        if nominal <= 0 {
            return Err(TimeError::NonPositiveTimebase);
        }
        if self.minutes >= 60 {
            return Err(TimeError::InvalidTimecodeField {
                field: "minutes",
                value: i64::from(self.minutes),
            });
        }
        if self.seconds >= 60 {
            return Err(TimeError::InvalidTimecodeField {
                field: "seconds",
                value: i64::from(self.seconds),
            });
        }
        if i64::from(self.frames) >= nominal {
            return Err(TimeError::InvalidTimecodeField {
                field: "frames",
                value: i64::from(self.frames),
            });
        }
        let drop = if self.drop_frame {
            let drop = timebase
                .drop_frame_frames()
                .ok_or(TimeError::DropFrameUnsupported {
                    num: timebase.numerator(),
                    den: timebase.denominator(),
                })?;
            i64::from(drop)
        } else {
            0
        };

        let total_seconds =
            i64::from(self.hours) * 3_600 + i64::from(self.minutes) * 60 + i64::from(self.seconds);
        let labelled = total_seconds
            .checked_mul(nominal)
            .and_then(|v| v.checked_add(i64::from(self.frames)))
            .ok_or(TimeError::Overflow)?;

        if drop == 0 {
            return Ok(labelled);
        }
        let total_minutes = i64::from(self.hours) * 60 + i64::from(self.minutes);
        let omitted = drop * (total_minutes - total_minutes / 10);
        labelled.checked_sub(omitted).ok_or(TimeError::Overflow)
    }

    /// Hours field.
    #[must_use]
    pub const fn hours(self) -> u32 {
        self.hours
    }

    /// Minutes field.
    #[must_use]
    pub const fn minutes(self) -> u32 {
        self.minutes
    }

    /// Seconds field.
    #[must_use]
    pub const fn seconds(self) -> u32 {
        self.seconds
    }

    /// Frames field.
    #[must_use]
    pub const fn frames(self) -> u32 {
        self.frames
    }

    /// Reports whether this timecode uses the drop-frame convention.
    #[must_use]
    pub const fn is_drop_frame(self) -> bool {
        self.drop_frame
    }
}

impl fmt::Display for Timecode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let separator = if self.drop_frame {
            Self::DROP_SEPARATOR
        } else {
            Self::NON_DROP_SEPARATOR
        };
        write!(
            f,
            "{:02}:{:02}:{:02}{}{:02}",
            self.hours, self.minutes, self.seconds, separator, self.frames
        )
    }
}

impl FromStr for Timecode {
    type Err = TimeError;

    /// Parses `HH:MM:SS:FF` (non-drop) or `HH:MM:SS;FF` (drop-frame).
    ///
    /// ```
    /// use maj_time::Timecode;
    /// let tc: Timecode = "01:00:00;00".parse().unwrap();
    /// assert!(tc.is_drop_frame());
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        let drop_frame = trimmed.contains(Self::DROP_SEPARATOR);
        let normalised = trimmed.replace(Self::DROP_SEPARATOR, ":");
        let parts: Vec<&str> = normalised.split(':').collect();
        if parts.len() != 4 {
            return Err(TimeError::TimecodeParse {
                input: trimmed.to_owned(),
            });
        }
        Self::from_hmsf(
            parse_field(trimmed, parts[0])?,
            parse_field(trimmed, parts[1])?,
            parse_field(trimmed, parts[2])?,
            parse_field(trimmed, parts[3])?,
            drop_frame,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_drop_conversion_is_positional() {
        let tb = Timebase::FPS_25;
        let tc = Timecode::from_frames(90_061, tb, TimecodeMode::NonDrop).unwrap();
        assert_eq!(tc.to_string(), "01:00:02:11");
        assert_eq!(tc.to_frames(tb).unwrap(), 90_061);
    }

    #[test]
    fn drop_frame_skips_the_expected_frame_numbers() {
        let tb = Timebase::FPS_29_97;
        // Frame 1800 is the first frame of minute one, so it is labelled ;02.
        let tc = Timecode::from_frames(1_800, tb, TimecodeMode::Drop).unwrap();
        assert_eq!(tc.to_string(), "00:01:00;02");
        // Frame 1798 is the last frame before the minute boundary.
        let last = Timecode::from_frames(1_798, tb, TimecodeMode::Drop).unwrap();
        assert_eq!(last.to_string(), "00:00:59;28");
    }

    #[test]
    fn drop_frame_does_not_skip_on_the_tenth_minute() {
        let tb = Timebase::FPS_29_97;
        let tc = Timecode::from_frames(17_982, tb, TimecodeMode::Drop).unwrap();
        assert_eq!(tc.to_string(), "00:10:00;00");
        assert_eq!(tc.to_frames(tb).unwrap(), 17_982);
    }

    #[test]
    fn drop_frame_round_trips_across_a_full_hour() {
        let tb = Timebase::FPS_29_97;
        // 29.97 drop-frame timecode is what makes an hour of wall clock exactly
        // 107892 frames, not 108000.
        for frames in [0, 1, 17, 1_798, 1_800, 17_982, 107_891] {
            let tc = Timecode::from_frames(frames, tb, TimecodeMode::Drop).unwrap();
            assert_eq!(
                tc.to_frames(tb).unwrap(),
                frames,
                "drop-frame round trip failed at {frames} ({tc})"
            );
        }
    }

    #[test]
    fn drop_frame_is_two_frames_per_minute_at_59_94() {
        let tb = Timebase::FPS_59_94;
        let tc = Timecode::from_frames(3_600, tb, TimecodeMode::Drop).unwrap();
        assert_eq!(tc.to_string(), "00:01:00;04");
        assert_eq!(tc.to_frames(tb).unwrap(), 3_600);
    }

    #[test]
    fn drop_frame_is_rejected_for_rates_without_a_rule() {
        for tb in [
            Timebase::FPS_24,
            Timebase::FPS_25,
            Timebase::FPS_30,
            Timebase::FPS_50,
        ] {
            let err = Timecode::from_frames(1_000, tb, TimecodeMode::Drop).unwrap_err();
            assert!(matches!(err, TimeError::DropFrameUnsupported { .. }));
        }
    }

    #[test]
    fn negative_frames_have_no_timecode() {
        let err = Timecode::from_frames(-1, Timebase::FPS_25, TimecodeMode::NonDrop).unwrap_err();
        assert_eq!(err, TimeError::NegativeFrames { frames: -1 });
    }

    #[test]
    fn out_of_range_frame_fields_are_rejected() {
        let tb = Timebase::FPS_25;
        let tc = Timecode::from_hmsf(0, 0, 0, 25, false).unwrap();
        assert_eq!(
            tc.to_frames(tb),
            Err(TimeError::InvalidTimecodeField {
                field: "frames",
                value: 25
            })
        );
    }

    #[test]
    fn structural_fields_are_validated() {
        assert!(Timecode::from_hmsf(0, 60, 0, 0, false).is_err());
        assert!(Timecode::from_hmsf(0, 0, 60, 0, false).is_err());
        assert!(Timecode::from_hmsf(1, 59, 59, 24, false).is_ok());
    }

    #[test]
    fn parsing_accepts_both_separators() {
        let nf: Timecode = "01:02:03:04".parse().unwrap();
        assert!(!nf.is_drop_frame());
        assert_eq!(
            (nf.hours(), nf.minutes(), nf.seconds(), nf.frames()),
            (1, 2, 3, 4)
        );

        let df: Timecode = "01:02:03;04".parse().unwrap();
        assert!(df.is_drop_frame());
        assert_eq!(df.to_string(), "01:02:03;04");

        assert!("not a timecode".parse::<Timecode>().is_err());
        assert!("01:02:03".parse::<Timecode>().is_err());
        assert!("01:02:03:04:05".parse::<Timecode>().is_err());
    }

    #[test]
    fn display_zero_pads_every_field() {
        let tc = Timecode::from_hmsf(1, 2, 3, 4, false).unwrap();
        assert_eq!(tc.to_string(), "01:02:03:04");
        let zero = Timecode::from_hmsf(0, 0, 0, 0, false).unwrap();
        assert_eq!(zero.to_string(), "00:00:00:00");
    }

    #[test]
    fn preferred_mode_matches_the_rate() {
        assert_eq!(
            TimecodeMode::preferred_for(Timebase::FPS_29_97),
            TimecodeMode::Drop
        );
        assert_eq!(
            TimecodeMode::preferred_for(Timebase::FPS_25),
            TimecodeMode::NonDrop
        );
        let tc = Timecode::from_frames_preferred(1_798, Timebase::FPS_29_97).unwrap();
        assert!(tc.is_drop_frame());
    }
}
