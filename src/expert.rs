//! Getting the Expert level ready: its solved table is loaded from disk, or
//! worked out once (a few minutes) and saved for next time. Either happens
//! on a background thread, so the game stays responsive.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use ur_game::solver::{self, Table};

/// Where the solved table is kept: beside the saved game. 276 MB.
pub fn default_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("ur-game").join("expert-table.bin"))
}

/// How the Expert table is coming along.
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

pub struct Expert {
    status: Arc<Mutex<Status>>,
    path: Option<PathBuf>,
}

impl Expert {
    /// Prepares the table on request, caching it at `path` if given.
    pub fn new(path: Option<PathBuf>) -> Expert {
        let status = if solver::installed().is_some() {
            Status::Ready
        } else {
            Status::Idle
        };
        Expert {
            status: Arc::new(Mutex::new(status)),
            path,
        }
    }

    /// Never prepares anything.
    pub fn off() -> Expert {
        let status = if solver::installed().is_some() {
            Status::Ready
        } else {
            Status::Off
        };
        Expert {
            status: Arc::new(Mutex::new(status)),
            path: None,
        }
    }

    /// Stuck at `status`, for testing how the game reacts.
    #[cfg(test)]
    pub fn pretend(status: Status) -> Expert {
        Expert {
            status: Arc::new(Mutex::new(status)),
            path: None,
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().map_or(Status::Off, |s| s.clone())
    }

    /// Still getting ready: an Expert move would have to wait.
    pub fn busy(&self) -> bool {
        matches!(self.status(), Status::Loading | Status::Solving(_))
    }

    /// Start loading or solving the table, unless that's already under way
    /// or done.
    pub fn prepare(&self) {
        self.start(true);
    }

    /// Load the table if it was saved earlier, so hints and reviews can use
    /// it, but never start working it out: that waits until Expert is
    /// chosen.
    pub fn load_if_saved(&self) {
        if self.path.as_deref().is_some_and(|p| p.exists()) {
            self.start(false);
        }
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
        let set = move |s: Status| {
            if let Ok(mut status) = shared.lock() {
                *status = s;
            }
        };
        std::thread::spawn(move || {
            if let Some(table) = path.as_deref().and_then(|p| Table::read_from(p).ok()) {
                solver::install(table);
                return set(Status::Ready);
            }
            if !solve {
                return set(Status::Idle);
            }
            set(Status::Solving(0.0));
            let table = solver::solve(|f| set(Status::Solving(f)));
            // If it can't be saved, it's worked out again next time.
            if let Some(p) = path.as_deref() {
                let _ = table.write_to(p);
            }
            solver::install(table);
            set(Status::Ready);
        });
    }

    /// What it's doing, while it's getting ready: "loading its table" or
    /// "working out the game, 42%".
    pub fn note(&self) -> Option<String> {
        match self.status() {
            Status::Loading => Some("loading its table".into()),
            Status::Solving(f) => Some(format!("working out the game, {}%", percent(f))),
            _ => None,
        }
    }

    /// The same, in a word or two for the menu: "loading" or "42% ready".
    pub fn short_note(&self) -> Option<String> {
        match self.status() {
            Status::Loading => Some("loading".into()),
            Status::Solving(f) => Some(format!("{}% ready", percent(f))),
            _ => None,
        }
    }
}

fn percent(fraction: f64) -> u32 {
    (fraction * 100.0).clamp(0.0, 99.0) as u32
}
