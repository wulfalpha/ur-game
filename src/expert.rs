//! Getting the Expert level ready: its solved table for each rule set is
//! loaded from disk, or worked out once (minutes) and saved for next time.
//! Either happens on a background thread, so the game stays responsive.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use ur_game::game::Rules;
use ur_game::solver::{self, Table};

/// Where the solved tables are kept: beside the saved game.
pub fn default_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("ur-game"))
}

/// The file for `rules`' table in `dir`: 276 MB for Finkel, 1 GB for
/// Masters.
pub fn table_path(dir: &Path, rules: Rules) -> PathBuf {
    dir.join(match rules {
        Rules::Finkel => "expert-finkel.bin",
        Rules::Masters => "expert-masters.bin",
    })
}

/// How an Expert table is coming along.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Nothing asked for it yet.
    Idle,
    /// Reading the saved table.
    Loading,
    /// Working the game out; the fraction done.
    Solving(f64),
    Ready,
    /// Never prepared (tests, mostly): Expert plays like Hard.
    Off,
}

struct Slot {
    rules: Rules,
    status: Arc<Mutex<Status>>,
    path: Option<PathBuf>,
    /// Looked for a saved table already.
    checked: AtomicBool,
}

impl Slot {
    fn new(rules: Rules, path: Option<PathBuf>, idle: Status) -> Slot {
        let status = if solver::installed(rules).is_some() {
            Status::Ready
        } else {
            idle
        };
        Slot {
            rules,
            status: Arc::new(Mutex::new(status)),
            path,
            checked: AtomicBool::new(false),
        }
    }

    fn status(&self) -> Status {
        self.status.lock().map_or(Status::Off, |s| s.clone())
    }

    fn start(&self, solve: bool) {
        let mut status = match self.status.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        if *status != Status::Idle {
            return;
        }
        *status = Status::Loading;
        drop(status);
        let shared = Arc::clone(&self.status);
        let path = self.path.clone();
        let rules = self.rules;
        let set = move |s: Status| {
            if let Ok(mut status) = shared.lock() {
                *status = s;
            }
        };
        std::thread::spawn(move || {
            if let Some(table) = path
                .as_deref()
                .and_then(|p| Table::read_from(p, rules).ok())
            {
                solver::install(table);
                return set(Status::Ready);
            }
            if !solve {
                return set(Status::Idle);
            }
            set(Status::Solving(0.0));
            let table = solver::solve(rules, |f| set(Status::Solving(f)));
            // If it can't be saved, it's worked out again next time.
            if let Some(p) = path.as_deref() {
                let _ = table.write_to(p);
            }
            solver::install(table);
            set(Status::Ready);
        });
    }
}

pub struct Expert {
    slots: [Slot; 2],
}

impl Expert {
    /// Prepares tables on request, caching them in `dir` if given.
    pub fn new(dir: Option<PathBuf>) -> Expert {
        // Earlier versions kept a single Finkel table, in a format these
        // don't read; it would only take up space.
        if let Some(old) = dir.as_deref().map(|d| d.join("expert-table.bin")) {
            let _ = std::fs::remove_file(old);
        }
        let slot = |rules| {
            let path = dir.as_deref().map(|d| table_path(d, rules));
            Slot::new(rules, path, Status::Idle)
        };
        Expert {
            slots: Rules::ALL.map(slot),
        }
    }

    /// Never prepares anything.
    pub fn off() -> Expert {
        Expert {
            slots: Rules::ALL.map(|rules| Slot::new(rules, None, Status::Off)),
        }
    }

    /// Stuck at `status` for every rule set, for testing how the game
    /// reacts.
    #[cfg(test)]
    pub fn pretend(status: Status) -> Expert {
        let slot = |rules| Slot {
            rules,
            status: Arc::new(Mutex::new(status.clone())),
            path: None,
            checked: AtomicBool::new(true),
        };
        Expert {
            slots: Rules::ALL.map(slot),
        }
    }

    fn slot(&self, rules: Rules) -> &Slot {
        &self.slots[rules as usize]
    }

    pub fn status(&self, rules: Rules) -> Status {
        self.slot(rules).status()
    }

    /// Still getting ready: an Expert move would have to wait.
    pub fn busy(&self, rules: Rules) -> bool {
        matches!(self.status(rules), Status::Loading | Status::Solving(_))
    }

    /// Start loading or solving the table for `rules`, unless that's
    /// already under way or done.
    pub fn prepare(&self, rules: Rules) {
        self.slot(rules).start(true);
    }

    /// Load the table for `rules` if it was saved earlier, so hints and
    /// reviews can use it, but never start working it out: that waits
    /// until Expert is chosen. Looks for the file only once.
    pub fn load_if_saved(&self, rules: Rules) {
        let slot = self.slot(rules);
        if slot.checked.swap(true, Ordering::Relaxed) {
            return;
        }
        if slot.path.as_deref().is_some_and(|p| p.exists()) {
            slot.start(false);
        }
    }

    /// What it's doing, while it's getting ready: "loading its table" or
    /// "working out the game, 42%".
    pub fn note(&self, rules: Rules) -> Option<String> {
        match self.status(rules) {
            Status::Loading => Some("loading its table".into()),
            Status::Solving(f) => Some(format!("working out the game, {}%", percent(f))),
            _ => None,
        }
    }

    /// The same, in a word or two for the menu: "loading" or "42% ready".
    pub fn short_note(&self, rules: Rules) -> Option<String> {
        match self.status(rules) {
            Status::Loading => Some("loading".into()),
            Status::Solving(f) => Some(format!("{}% ready", percent(f))),
            _ => None,
        }
    }
}

fn percent(fraction: f64) -> u32 {
    (fraction * 100.0).clamp(0.0, 99.0) as u32
}
