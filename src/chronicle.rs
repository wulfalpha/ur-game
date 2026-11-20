//! Sharing the Chronicle: a plain-text record of the game, written to a
//! file and handed to the clipboard.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where shared chronicles are written: a `chronicles` folder beside the
/// saved game.
pub fn default_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("ur-game").join("chronicles"))
}

/// Seconds since the Unix epoch, now.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `(year, month, day, hour, minute, second)` in UTC.
fn civil(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's days-to-civil algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    (year, month, day, h as u32, m as u32, s as u32)
}

/// "2026-11-20 15:32 UTC"
pub fn timestamp(secs: u64) -> String {
    let (y, mo, d, h, mi, _) = civil(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02} UTC")
}

/// "ur-game-2026-11-20-153201.txt"
pub fn file_name(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(secs);
    format!("ur-game-{y:04}-{mo:02}-{d:02}-{h:02}{mi:02}{s:02}.txt")
}

/// Write `text` as a new file in `dir`; returns its path.
pub fn write(dir: &Path, secs: u64, text: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(file_name(secs));
    fs::write(&path, text)?;
    Ok(path)
}

/// The OSC 52 escape sequence that asks a terminal to put `text` on the
/// clipboard. It works over SSH, in terminals that allow it.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(timestamp(0), "1970-01-01 00:00 UTC");
        // 2024-02-29 13:05:09 UTC, a leap day.
        assert_eq!(timestamp(1_709_211_909), "2024-02-29 13:05 UTC");
        assert_eq!(file_name(1_709_211_909), "ur-game-2024-02-29-130509.txt");
    }

    #[test]
    fn base64_matches_rfc4648() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded);
        }
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
    }

    #[test]
    fn writes_a_file() {
        let dir = std::env::temp_dir().join(format!("ur-chronicle-{}", std::process::id()));
        let path = write(&dir, 0, "hello").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
        fs::remove_dir_all(dir).unwrap();
    }
}
