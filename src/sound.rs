//! Embedded original effects, shared by the window and terminal front ends.
use crate::app::App;
use quad_snd::{AudioContext, PlaySoundParams, Sound};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Dice,
    Move,
    Capture,
    Rosette,
    Victory,
    Hop,
    Home,
    Menu,
}

pub struct Audio {
    context: Option<AudioContext>,
    sounds: Vec<Sound>,
    failed: bool,
}

impl Audio {
    pub fn new() -> Self {
        Self {
            context: None,
            sounds: Vec::new(),
            failed: false,
        }
    }

    /// Initialize lazily: muted games never open an audio device.
    pub fn update(&mut self, app: &mut App) {
        let cues = std::mem::take(&mut app.sound_cues);
        if !app.sound_enabled {
            if let Some(ctx) = &self.context {
                for sound in &self.sounds {
                    sound.stop(ctx);
                }
            }
            return;
        }
        if cues.is_empty() || self.failed {
            return;
        }
        if self.context.is_none() {
            let Ok(ctx) = std::panic::catch_unwind(AudioContext::new) else {
                self.failed = true;
                return;
            };
            self.sounds = [
                &include_bytes!("../assets/sounds/dice.wav")[..],
                &include_bytes!("../assets/sounds/move.wav")[..],
                &include_bytes!("../assets/sounds/capture.wav")[..],
                &include_bytes!("../assets/sounds/rosette.wav")[..],
                &include_bytes!("../assets/sounds/victory.wav")[..],
                &include_bytes!("../assets/sounds/hop.wav")[..],
                &include_bytes!("../assets/sounds/home.wav")[..],
                &include_bytes!("../assets/sounds/menu.wav")[..],
            ]
            .into_iter()
            .map(|bytes| Sound::load(&ctx, bytes))
            .collect();
            self.context = Some(ctx);
        }
        let ctx = self.context.as_ref().unwrap();
        for sound in &self.sounds {
            sound.set_volume(ctx, app.sound_volume);
        }
        for cue in cues {
            let index = match cue {
                Cue::Dice => 0,
                Cue::Move => 1,
                Cue::Capture => 2,
                Cue::Rosette => 3,
                Cue::Victory => 4,
                Cue::Hop => 5,
                Cue::Home => 6,
                Cue::Menu => 7,
            };
            self.sounds[index].play(
                ctx,
                PlaySoundParams {
                    looped: false,
                    volume: app.sound_volume,
                },
            );
        }
    }
}
