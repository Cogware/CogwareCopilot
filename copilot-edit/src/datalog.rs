// SPDX-License-Identifier: GPL-3.0-only
//! Playing back a log the car actually recorded.
//!
//! The simulated car next door agrees with itself -- revs drop into a gear,
//! coolant climbs and stays -- but everything in it was reasoned out from a
//! speed trace. A real log has what reasoning does not: the sensor that
//! drops out for a tenth of a second, the idle that hunts, the AFR spike on
//! a gear change. A layout read against one of those is read against what it
//! will actually have to show.
//!
//! Anything with a header row and numbers under it: TunerStudio's `.msl` and
//! its tab-separated columns, a Haltech or AEM export, a spreadsheet saved as
//! CSV. The header names the channels, this matches them against the gauge
//! table, and the columns it cannot place are left alone rather than guessed
//! at -- a log is somebody's data and a wrong guess is worse than a gap.
//!
//! # Units are taken as the gauge's own
//!
//! A column called `CLT` is written to `CLNT` as it stands. If the log holds
//! Fahrenheit and the gauge reads Celsius, the number is wrong and looks it;
//! nothing here tries to detect that, because a log that says `190` could be
//! either and guessing turns a visible fault into an invisible one.

use std::collections::BTreeMap;
use std::path::Path;

/// One channel of a log: the gauge it feeds and a sample per row.
#[derive(Debug)]
struct Channel {
    id: u16,
    /// Scaled to the raw integer the bus carries, once at load.
    raw: Vec<i32>,
}

/// A log, ready to be played back.
#[derive(Debug)]
pub struct Log {
    /// What the file was called, for the editor to show.
    pub name: String,
    /// Seconds from the start of the log, one per row.
    times_s: Vec<f32>,
    channels: Vec<Channel>,
    /// Gauge names this log fills, for the editor to show.
    pub filled: Vec<&'static str>,
    /// Header columns that matched no gauge, so somebody can see what was
    /// skipped rather than wonder why a needle never moves.
    pub skipped: Vec<String>,
}

impl Log {
    /// How long the log runs.
    #[must_use]
    pub fn seconds(&self) -> f32 {
        self.times_s.last().copied().unwrap_or(0.0)
    }

    /// Rows in the log.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.times_s.len()
    }

    /// Every channel's reading `at_s` seconds in, looping at the end.
    ///
    /// Held at the sample rather than interpolated between two: a log is what
    /// the car recorded, and a value it never held is not in it. A logger
    /// sampling at ten or twenty a second is already finer than the bus
    /// carries.
    pub fn at(&self, at_s: f32) -> Vec<(u16, i32)> {
        if self.times_s.is_empty() {
            return Vec::new();
        }
        let span = self.seconds();
        let t = if span > 0.0 { at_s % span } else { 0.0 };
        // The last sample at or before `t`.
        let row = match self
            .times_s
            .binary_search_by(|s| s.partial_cmp(&t).unwrap_or(std::cmp::Ordering::Less))
        {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        };
        self.channels
            .iter()
            .filter_map(|c| c.raw.get(row).map(|raw| (c.id, *raw)))
            .collect()
    }

    /// The drive every build has, with no file to go and find.
    ///
    /// The EPA's US06 dynamometer schedule: a car driven hard on a rolling
    /// road, published by the United States government and measured rather
    /// than modelled. It carries road speed and nothing else, which is what
    /// the published schedule is -- so it fills the speed and leaves the rest
    /// to whatever else is driving, instead of inventing channels and calling
    /// them a log.
    ///
    /// One sample a second, which is the rate it was published at. Playing it
    /// faster would not add anything: the trace is the seconds, and between
    /// two of them the car really was somewhere this file does not say.
    #[must_use]
    pub fn us06() -> Log {
        let speed = cogware_can::trace::SPEED_DKMH;
        let vss = cogware_can::ALL_GAUGES
            .iter()
            .find(|g| g.name == "VSS")
            .expect("the gauge table has a road speed");
        Log {
            name: "US06 (built in)".into(),
            times_s: (0..speed.len()).map(|i| i as f32).collect(),
            // Tenths of a km/h at both ends, so the published numbers go
            // across as they were published.
            channels: alloc_channel(vss.id, speed),
            filled: vec![vss.name],
            skipped: Vec::new(),
        }
    }

    /// Whether this is the built-in trace rather than somebody's own log.
    #[must_use]
    pub fn is_built_in(&self) -> bool {
        self.name.starts_with("US06")
    }

    /// Read a log from a file.
    pub fn read(path: &Path) -> Result<Log, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into(),
        );
        Log::parse(&name, &text)
    }

    /// The same, from text already in hand.
    pub fn parse(name: &str, text: &str) -> Result<Log, String> {
        let lines: Vec<&str> = text.lines().collect();
        // Tabs if the file uses them at all: a CSV exported from a
        // spreadsheet can hold a comma inside a quoted title line, and a
        // tab-separated log never holds a tab inside a field.
        let sep = if text.contains('\t') { '\t' } else { ',' };
        let split = |line: &str| -> Vec<String> {
            line.split(sep)
                .map(|f| f.trim().trim_matches('"').to_string())
                .collect()
        };

        // The header is the first line whose fields are mostly names rather
        // than numbers. TunerStudio puts a title and a blank line above it.
        let header_at = lines
            .iter()
            .position(|line| {
                let f = split(line);
                f.len() >= 2 && f.iter().filter(|s| !s.is_empty() && !is_number(s)).count() >= 2
            })
            .ok_or("no header row: expected a line naming the channels")?;
        let header = split(lines[header_at]);

        // A units row under the header, if there is one: more names, no
        // numbers. Skipped rather than read, since units are not converted.
        let mut first_row = header_at + 1;
        if let Some(line) = lines.get(first_row) {
            let f = split(line);
            if !f.is_empty() && f.iter().all(|s| s.is_empty() || !is_number(s)) {
                first_row += 1;
            }
        }

        // Which column is the clock, and which columns are gauges.
        let time_col = header.iter().position(|h| {
            let k = key(h);
            k == "time" || k == "times" || k == "seconds" || k == "secs" || k == "timestamp"
        });
        let mut wanted: Vec<(usize, u16, f32)> = Vec::new();
        let mut filled = Vec::new();
        let mut skipped = Vec::new();
        for (col, head) in header.iter().enumerate() {
            if Some(col) == time_col || head.is_empty() {
                continue;
            }
            match gauge_for(head) {
                Some(g) => {
                    wanted.push((col, g.id, g.unit.scale as f32));
                    filled.push(g.name);
                }
                None => skipped.push(head.clone()),
            }
        }
        if wanted.is_empty() {
            return Err(format!(
                "no column in {name} matches a gauge; saw {}",
                header.join(", ")
            ));
        }

        let mut times_s = Vec::new();
        let mut cols: BTreeMap<usize, Vec<i32>> =
            wanted.iter().map(|(c, _, _)| (*c, Vec::new())).collect();
        for (n, line) in lines.iter().enumerate().skip(first_row) {
            let f = split(line);
            // A short or non-numeric line is the end of the data as often as
            // it is a fault -- loggers write a footer -- so it stops the read
            // rather than failing it.
            if f.len() < header.len() || !f.iter().take(2).any(|s| is_number(s)) {
                continue;
            }
            let t = match time_col
                .and_then(|c| f.get(c))
                .and_then(|s| s.parse::<f32>().ok())
            {
                Some(t) => t,
                // No clock in the file, so the rows are the clock. Ten a
                // second is what a stock logger writes and is close enough
                // that a log with no time reads at about the right speed.
                None => (n - first_row) as f32 / 10.0,
            };
            times_s.push(t);
            for (col, _, scale) in &wanted {
                let raw = f
                    .get(*col)
                    .and_then(|s| s.parse::<f32>().ok())
                    .map_or(0, |v| (v * scale).round() as i32);
                cols.entry(*col).or_default().push(raw);
            }
        }
        if times_s.len() < 2 {
            return Err(format!("{name} has a header but no rows under it"));
        }
        // A log whose clock runs backwards or restarts is not one this can
        // seek in, and a silent sort would put the samples in an order the
        // car was never in.
        if times_s.windows(2).any(|w| w[1] < w[0]) {
            return Err(format!("{name}: its clock goes backwards"));
        }
        // From zero, so the first row plays at once.
        let start = times_s[0];
        for t in &mut times_s {
            *t -= start;
        }

        let channels = wanted
            .iter()
            .filter_map(|(col, id, _)| cols.remove(col).map(|raw| Channel { id: *id, raw }))
            .collect();
        Ok(Log {
            name: name.to_string(),
            times_s,
            channels,
            filled,
            skipped,
        })
    }
}

/// One channel's worth of already-scaled samples.
fn alloc_channel(id: u16, raw: &[u16]) -> Vec<Channel> {
    vec![Channel {
        id,
        raw: raw.iter().map(|v| i32::from(*v)).collect(),
    }]
}

/// Whether a field parses as a number.
fn is_number(s: &str) -> bool {
    !s.is_empty() && s.parse::<f32>().is_ok()
}

/// A header name reduced to letters and digits, lowercase, so `Coolant Temp`,
/// `coolant_temp` and `CoolantTemp` are one name.
fn key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The gauge a log column feeds, if this build knows one.
///
/// The gauge table's own names first, so a log exported from this system
/// round-trips, then the names the common loggers use. Anything else is left
/// for the caller to report as skipped.
fn gauge_for(head: &str) -> Option<&'static cogware_can::GaugeData> {
    let k = key(head);
    if let Some(g) = cogware_can::ALL_GAUGES.iter().find(|g| key(g.name) == k) {
        return Some(g);
    }
    let name = match k.as_str() {
        "rpm" | "engspeed" | "enginespeed" => "RPM",
        "tps" | "tp" | "throttle" | "throttleposition" => "TPS",
        "map" | "manifoldpressure" | "mappressure" => "MAP",
        "afr" | "afr1" | "o2" | "lambda" | "lambda1" | "wideband" => "AFR_PRI",
        "afrtarget" | "targetafr" | "lambdatarget" | "targetlambda" => "AFR_TARGET",
        "clt" | "ect" | "coolant" | "coolanttemp" | "watertemp" | "engtemp" => "CLNT",
        "iat" | "mat" | "intake" | "intaketemp" | "airtemp" | "chargetemp" => "IAT",
        "batt" | "battv" | "battery" | "batteryvoltage" | "vbat" | "voltage" => "BAT_VOL",
        "vss" | "speed" | "vehiclespeed" | "roadspeed" | "kph" | "kmh" | "mph" => "VSS",
        "gear" | "currentgear" => "GEAR",
        "oilpressure" | "oilpres" | "oilp" => "OIL_PRES",
        "oiltemp" | "oiltemperature" => "OIL_TEMP",
        "fuel" | "fuellevel" => "FUEL_LEVEL",
        "boost" | "boosttarget" | "targetboost" => "BOOST_TARGET",
        _ => return None,
    };
    cogware_can::ALL_GAUGES
        .iter()
        .find(|g| g.name == name)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tab-separated log shaped like TunerStudio's: a title, a header, a
    /// units row, then data.
    const MSL: &str = "\"MS2Extra log\"\n\
Time\tRPM\tTPS\tCLT\tAFR\tWidgetCount\n\
s\trpm\t%\tC\tAFR\t-\n\
0.000\t850\t2.1\t78\t14.7\t3\n\
0.100\t1200\t18.0\t79\t13.2\t3\n\
0.200\t2400\t55.5\t80\t12.6\t3\n";

    #[test]
    fn a_tab_separated_log_reads_its_channels_and_says_what_it_skipped() {
        let log = Log::parse("run.msl", MSL).expect("parses");
        assert_eq!(log.rows(), 3);
        assert!((log.seconds() - 0.2).abs() < 1e-6, "clock runs from zero");
        assert!(log.filled.contains(&"RPM"), "{:?}", log.filled);
        assert!(log.filled.contains(&"CLNT"), "CLT is the coolant");
        assert!(log.filled.contains(&"AFR_PRI"), "AFR is the primary one");
        // A column nothing matches is reported rather than guessed at.
        assert_eq!(log.skipped, vec!["WidgetCount".to_string()]);
    }

    #[test]
    fn a_reading_is_the_sample_at_or_before_the_moment_asked_for() {
        let log = Log::parse("run.msl", MSL).expect("parses");
        let rpm = |t: f32| {
            log.at(t)
                .into_iter()
                .find(|(id, _)| cogware_can::gauge_by_id(*id).is_some_and(|g| g.name == "RPM"))
                .map(|(_, raw)| raw)
        };
        assert_eq!(rpm(0.0), Some(850), "the first row plays at once");
        // Held, not interpolated: the car never read 1025.
        assert_eq!(rpm(0.05), Some(850));
        assert_eq!(rpm(0.1), Some(1200));
        // And it loops rather than stopping on the last row.
        assert_eq!(rpm(0.2 + 0.05), Some(850), "past the end it starts again");
    }

    #[test]
    fn a_comma_separated_log_with_no_clock_is_read_at_ten_rows_a_second() {
        let csv = "rpm,coolant temp\n900,80\n950,80\n1000,81\n";
        let log = Log::parse("plain.csv", csv).expect("parses");
        assert_eq!(log.rows(), 3);
        assert!((log.seconds() - 0.2).abs() < 1e-6);
        assert_eq!(log.filled, vec!["RPM", "CLNT"]);
    }

    #[test]
    fn a_file_with_nothing_this_build_knows_says_so_rather_than_playing_silence() {
        let err = Log::parse("odd.csv", "alpha,beta\n1,2\n3,4\n").unwrap_err();
        assert!(err.contains("matches a gauge"), "{err}");
    }

    #[test]
    fn a_log_whose_clock_goes_backwards_is_refused() {
        let csv = "Time,RPM\n0.0,900\n0.2,950\n0.1,1000\n";
        let err = Log::parse("bad.csv", csv).unwrap_err();
        assert!(err.contains("backwards"), "{err}");
    }

    #[test]
    fn the_built_in_drive_is_the_published_trace_second_by_second() {
        let log = Log::us06();
        assert!(log.is_built_in());
        assert_eq!(log.rows(), cogware_can::trace::SPEED_DKMH.len());
        assert_eq!(
            log.filled,
            vec!["VSS"],
            "the schedule is a speed and nothing else"
        );
        assert!(log.skipped.is_empty());

        let vss = cogware_can::ALL_GAUGES
            .iter()
            .find(|g| g.name == "VSS")
            .unwrap();
        let at = |t: f32| {
            log.at(t)
                .into_iter()
                .find(|(id, _)| *id == vss.id)
                .map(|(_, r)| r)
        };
        // Straight through, in the tenths of a km/h both ends count in.
        for second in [0usize, 10, 60, 200] {
            assert_eq!(
                at(second as f32),
                Some(i32::from(cogware_can::trace::SPEED_DKMH[second])),
                "second {second} does not match the published trace"
            );
        }
        // It stands still at the start, which is how the schedule opens.
        assert_eq!(at(0.0), Some(0));
        // And somewhere in the middle it is moving properly.
        assert!(at(60.0).unwrap() > 400, "a minute in the car is going");
    }
}
