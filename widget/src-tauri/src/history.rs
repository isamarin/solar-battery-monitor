//! Rolling 24h history of BMS samples, persisted as JSON lines in the app data dir.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

const KEEP_MS: u64 = 24 * 3600 * 1000;
const EVERY_MS: u64 = 5000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Sample {
    /// unix ms
    pub t: u64,
    pub soc: u8,
    pub v: f64,
    pub i: f64,
    pub cells: Vec<f64>,
    pub temps: Vec<f64>,
}

#[derive(Default)]
pub struct History {
    samples: VecDeque<Sample>,
    file: Option<PathBuf>,
}

impl History {
    /// Loads the last 24h from `file` and compacts it.
    pub fn load(file: PathBuf, now: u64) -> Self {
        let samples: VecDeque<Sample> = fs::read_to_string(&file)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<Sample>(l).ok())
            .filter(|s| s.t + KEEP_MS >= now)
            .collect();
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let body: String = samples.iter().filter_map(|s| serde_json::to_string(s).ok()).map(|l| l + "\n").collect();
        let _ = fs::write(&file, body);
        History { samples, file: Some(file) }
    }

    /// Records `s` if at least EVERY_MS passed since the last sample; returns whether it was kept.
    pub fn push(&mut self, s: Sample) -> bool {
        if self.samples.back().is_some_and(|last| s.t < last.t + EVERY_MS) {
            return false;
        }
        while self.samples.front().is_some_and(|f| f.t + KEEP_MS < s.t) {
            self.samples.pop_front();
        }
        if let (Some(path), Ok(line)) = (&self.file, serde_json::to_string(&s)) {
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(f, "{line}");
            }
        }
        self.samples.push_back(s);
        true
    }

    pub fn all(&self) -> Vec<Sample> {
        self.samples.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: u64) -> Sample {
        Sample { t, soc: 50, v: 13.2, i: 1.0, cells: vec![3.3; 4], temps: vec![25.0] }
    }

    #[test]
    fn throttles_and_expires() {
        let mut h = History::default();
        assert!(h.push(at(0)));
        assert!(!h.push(at(4999)));
        assert!(h.push(at(5000)));
        assert!(h.push(at(KEEP_MS + 5000)));
        assert_eq!(h.all().iter().map(|s| s.t).collect::<Vec<_>>(), [5000, KEEP_MS + 5000]);
    }

    #[test]
    fn persists_and_reloads_recent_only() {
        let path = std::env::temp_dir().join(format!("jbd-history-test-{}.jsonl", std::process::id()));
        let _ = fs::remove_file(&path);
        let mut h = History::load(path.clone(), 0);
        h.push(at(1_000));
        h.push(at(KEEP_MS));
        let reloaded = History::load(path.clone(), KEEP_MS + 2_000);
        assert_eq!(reloaded.all(), [at(KEEP_MS)]);
        let _ = fs::remove_file(&path);
    }
}
