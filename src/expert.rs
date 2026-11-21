//! Getting the Expert level ready: its solved table for each rule set is
//! loaded from disk, or worked out once (minutes) and saved for next time.
//! Either happens on a background thread, so the game stays responsive.

use std::io;
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
    /// Waiting for another table operation to finish.
    Queued,
    /// Reading the saved table.
    Loading,
    /// Working the game out; the fraction done.
    Solving(f64),
    Ready,
    /// Playable from memory, but not cached for the next launch.
    ReadyUnsaved(String),
    /// Writing the cache; the installed table remains playable.
    Saving,
    /// Never prepared (tests, mostly): Expert plays like Hard.
    Off,
}

struct Slot {
    rules: Rules,
    status: Arc<Mutex<Status>>,
    path: Option<PathBuf>,
    /// Looked for a saved table already.
    checked: AtomicBool,
    notices: Arc<Mutex<Vec<String>>>,
    cache_warned: Arc<AtomicBool>,
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
            notices: Arc::new(Mutex::new(Vec::new())),
            cache_warned: Arc::new(AtomicBool::new(false)),
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
        *status = Status::Queued;
        drop(status);
        let shared = Arc::clone(&self.status);
        let path = self.path.clone();
        let rules = self.rules;
        let notices = Arc::clone(&self.notices);
        let cache_warned = Arc::clone(&self.cache_warned);
        let set = move |s: Status| {
            if let Ok(mut status) = shared.lock() {
                *status = s;
            }
        };
        std::thread::spawn(move || {
            let _permit = TABLE_WORK.lock().unwrap_or_else(|e| e.into_inner());
            set(Status::Loading);
            if let Some(p) = path.as_deref() {
                match Table::read_from(p, rules) {
                    Ok(table) => {
                        solver::install(table);
                        return set(Status::Ready);
                    }
                    Err(e) if e.kind() != io::ErrorKind::NotFound => {
                        if !cache_warned.swap(true, Ordering::Relaxed)
                            && let Ok(mut notices) = notices.lock()
                        {
                            notices.push(format!("{rules} Expert cache could not be verified: {e}. Older caches have no checksum. Choose Expert to rebuild it once; hints and reviews use Hard until ready."));
                        }
                    }
                    Err(_) => {}
                }
            }
            if !solve {
                return set(Status::Idle);
            }
            set(Status::Solving(0.0));
            let table = solver::solve(rules, |f| set(Status::Solving(f)));
            // Install before saving: a cache failure must never discard a solve.
            solver::install(table);
            set(Status::Saving);
            let result = save_table(path.as_deref(), |p| {
                solver::installed(rules)
                    .expect("just installed")
                    .write_to(p)
            });
            set(cache_status(rules, result, &notices));
        });
    }
}

// One table operation at a time, including loads and retries, to bound peak memory.
static TABLE_WORK: Mutex<()> = Mutex::new(());

fn save_table(path: Option<&Path>, write: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
    let path = path.ok_or_else(|| io::Error::other("no data directory available"))?;
    write(path)
}

fn cache_status(rules: Rules, result: io::Result<()>, notices: &Mutex<Vec<String>>) -> Status {
    let (status, message) = match result {
        Ok(()) => (
            Status::Ready,
            format!("{rules} Expert table saved for next time."),
        ),
        Err(e) => {
            let error = e.to_string();
            let message = format!(
                "{rules} Expert is ready, but could not save its table: {error}. It will need to be prepared again next launch. Press T to retry saving."
            );
            (Status::ReadyUnsaved(error), message)
        }
    };
    if let Ok(mut notices) = notices.lock() {
        notices.push(message);
    }
    status
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
            notices: Arc::new(Mutex::new(Vec::new())),
            cache_warned: Arc::new(AtomicBool::new(false)),
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
        matches!(
            self.status(rules),
            Status::Queued | Status::Loading | Status::Solving(_)
        )
    }

    /// Start loading or solving the table for `rules`, unless that's
    /// already under way or done.
    pub fn prepare(&self, rules: Rules) {
        self.slot(rules).start(true);
    }

    /// Retry failed cache writes without recomputing either table.
    pub fn retry_saving(&self) {
        for slot in &self.slots {
            let mut status = slot.status.lock().unwrap_or_else(|e| e.into_inner());
            if !matches!(*status, Status::ReadyUnsaved(_)) {
                continue;
            }
            *status = Status::Saving;
            let shared = Arc::clone(&slot.status);
            let notices = Arc::clone(&slot.notices);
            let path = slot.path.clone();
            let rules = slot.rules;
            std::thread::spawn(move || {
                let _permit = TABLE_WORK.lock().unwrap_or_else(|e| e.into_inner());
                let result = save_table(path.as_deref(), |p| {
                    solver::installed(rules)
                        .ok_or_else(|| io::Error::other("table is not in memory"))?
                        .write_to(p)
                });
                let next = cache_status(rules, result, &notices);
                *shared.lock().unwrap_or_else(|e| e.into_inner()) = next;
            });
        }
    }

    pub fn take_notices(&self) -> Vec<String> {
        self.slots
            .iter()
            .flat_map(|slot| {
                std::mem::take(&mut *slot.notices.lock().unwrap_or_else(|e| e.into_inner()))
            })
            .collect()
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
            Status::Queued => Some("waiting for the other Expert table".into()),
            Status::Loading => Some("loading its table".into()),
            Status::Solving(f) => Some(format!("working out the game, {}%", percent(f))),
            _ => None,
        }
    }

    /// The same, in a word or two for the menu: "loading" or "42% ready".
    pub fn short_note(&self, rules: Rules) -> Option<String> {
        match self.status(rules) {
            Status::Queued => Some("queued".into()),
            Status::ReadyUnsaved(_) => Some("not saved · T retry".into()),
            Status::Saving => Some("saving".into()),
            Status::Loading => Some("loading".into()),
            Status::Solving(f) => Some(format!("{}% ready", percent(f))),
            _ => None,
        }
    }
}

fn percent(fraction: f64) -> u32 {
    (fraction * 100.0).clamp(0.0, 99.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition() {
            assert!(Instant::now() < deadline, "background operation timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn table_operations_wait_without_blocking_the_caller() {
        let permit = TABLE_WORK.lock().unwrap_or_else(|e| e.into_inner());
        let expert = Expert {
            slots: Rules::ALL.map(|rules| Slot::new(rules, None, Status::Idle)),
        };
        // Loading nonexistent tables exercises scheduling without a full solve.
        for rules in Rules::ALL {
            expert.slot(rules).start(false);
            assert_eq!(expert.status(rules), Status::Queued);
            assert!(expert.busy(rules));
            assert!(expert.note(rules).unwrap().contains("waiting"));
            // Repeated requests do not spawn another job or overwrite the queue state.
            expert.prepare(rules);
            assert_eq!(expert.status(rules), Status::Queued);
        }
        drop(permit);
        wait_until(|| Rules::ALL.iter().all(|&r| expert.status(r) == Status::Idle));
    }

    #[test]
    fn invalid_cache_is_reported_once_and_never_installed() {
        let dir = std::env::temp_dir().join(format!("ur-invalid-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = table_path(&dir, Rules::Masters);
        std::fs::write(&path, b"ur-game expert table 2 masters\n").unwrap();
        let expert = Expert::new(Some(dir.clone()));
        expert.load_if_saved(Rules::Masters);
        wait_until(|| expert.status(Rules::Masters) == Status::Idle);
        assert!(solver::installed(Rules::Masters).is_none());
        let notices = expert.take_notices();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains("could not be verified"));
        assert!(notices[0].contains("Choose Expert to rebuild"));
        // A later attempt must not flood the Chronicle with the same warning.
        expert.slot(Rules::Masters).start(false);
        wait_until(|| expert.status(Rules::Masters) == Status::Idle);
        assert!(expert.take_notices().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cache_failure_is_visible_and_success_clears_it() {
        let notices = Mutex::new(Vec::new());
        let result = save_table(Some(Path::new("unused")), |_| {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "read only"))
        });
        let status = cache_status(Rules::Masters, result, &notices);
        assert_eq!(status, Status::ReadyUnsaved("read only".into()));
        let expert = Expert::pretend(status);
        assert!(!expert.busy(Rules::Masters));
        assert!(
            expert
                .short_note(Rules::Masters)
                .unwrap()
                .contains("T retry")
        );
        let message = notices.lock().unwrap().pop().unwrap();
        assert!(message.contains("Masters"));
        assert!(message.contains("read only"));
        assert!(message.contains("next launch"));
        let result = save_table(Some(Path::new("unused")), |_| Ok(()));
        assert_eq!(
            cache_status(Rules::Masters, result, &notices),
            Status::Ready
        );
        assert!(save_table(None, |_| panic!("no destination")).is_err());
    }

    #[test]
    fn failed_retry_reports_the_error_without_starting_a_solve() {
        let expert = Expert::pretend(Status::ReadyUnsaved("previous failure".into()));
        expert.retry_saving();
        wait_until(|| {
            Rules::ALL
                .iter()
                .all(|&r| matches!(expert.status(r), Status::ReadyUnsaved(_)))
        });
        let notices = expert.take_notices();
        assert_eq!(notices.len(), 2);
        assert!(notices.iter().all(|n| n.contains("no data directory")));
        assert!(expert.take_notices().is_empty());
    }
}
