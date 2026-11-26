//! Interface sounds: short tones synthesized here and played through raylib's
//! audio device, so the launcher ships no sound files.
//!
//! The device is opened for the first sound and closed again after
//! [`IDLE_SECONDS`] of quiet, so an idle launcher runs no mixing thread and
//! leaves the codec and its amplifier off. On Gizlet OS the sounds play through
//! ALSA's default device at the system volume.

use raylib_sys as ffi;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sfx {
    /// The focus moved, or a choice or slider changed.
    Move,
    /// A page, menu or keyboard opened, or something was chosen or switched.
    Confirm,
    /// A page, menu or keyboard closed.
    Back,
    /// The volume was set: played at the new level.
    Volume,
}

impl Sfx {
    pub const ALL: [Sfx; 4] = [Sfx::Move, Sfx::Confirm, Sfx::Back, Sfx::Volume];

    /// The notes played in turn: frequency in Hz, seconds and loudness (0–1).
    fn notes(self) -> &'static [(f32, f32, f32)] {
        match self {
            Sfx::Move => &[(1568.0, 0.03, 0.2)],
            Sfx::Confirm => &[(1047.0, 0.04, 0.24), (1568.0, 0.07, 0.24)],
            Sfx::Back => &[(1319.0, 0.04, 0.22), (880.0, 0.07, 0.22)],
            Sfx::Volume => &[(988.0, 0.09, 0.5)],
        }
    }
}

pub const RATE: u32 = 44_100;
/// Seconds without a sound before the device is closed.
const IDLE_SECONDS: f64 = 8.0;
/// Seconds before opening a device that failed to open is tried again.
const RETRY_SECONDS: f64 = 30.0;

/// The sound as mono 16-bit samples at [`RATE`]: each note a sine with a soft
/// second harmonic, a quick attack and an exponential decay that ends at zero.
pub fn samples(sfx: Sfx) -> Vec<i16> {
    let mut out = Vec::new();
    for &(hz, seconds, gain) in sfx.notes() {
        let n = (seconds * RATE as f32) as usize;
        let attack = RATE as f32 * 0.002;
        let release = RATE as f32 * 0.004;
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            let phase = std::f32::consts::TAU * hz * t;
            let tone = phase.sin() + 0.2 * (2.0 * phase).sin();
            let left = (n - i) as f32;
            let envelope = (i as f32 / attack).min(1.0) * (-5.0 * t / seconds).exp() * (left / release).min(1.0);
            out.push((tone / 1.2 * envelope * gain * i16::MAX as f32) as i16);
        }
    }
    out
}

/// Plays [`Sfx`] on raylib's audio device, which it opens and closes as needed.
#[derive(Default)]
pub struct Player {
    /// One loaded sound per [`Sfx::ALL`] while the device is open.
    sounds: Vec<ffi::Sound>,
    last_play: f64,
    /// When opening may be tried again after a failure.
    retry_at: Option<f64>,
}

impl Player {
    /// Plays `sfx` at `gain` (0–1); `now` is the time in seconds.
    pub fn play(&mut self, sfx: Sfx, gain: f32, now: f64) {
        if self.sounds.is_empty() && !self.open(now) {
            return;
        }
        self.last_play = now;
        let i = Sfx::ALL.iter().position(|&s| s == sfx).unwrap_or(0);
        let sound = self.sounds[i];
        unsafe {
            ffi::SetSoundVolume(sound, gain.clamp(0.0, 1.0));
            ffi::PlaySound(sound);
        }
    }

    /// Closes the device once it has been quiet for a while.
    pub fn tick(&mut self, now: f64) {
        if !self.sounds.is_empty() && now - self.last_play >= IDLE_SECONDS {
            self.close();
        }
    }

    fn open(&mut self, now: f64) -> bool {
        if self.retry_at.is_some_and(|at| now < at) {
            return false;
        }
        unsafe {
            ffi::InitAudioDevice();
            if !ffi::IsAudioDeviceReady() {
                self.retry_at = Some(now + RETRY_SECONDS);
                return false;
            }
        }
        self.retry_at = None;
        for sfx in Sfx::ALL {
            let mut data = samples(sfx);
            let wave = ffi::Wave {
                frameCount: data.len() as u32,
                sampleRate: RATE,
                sampleSize: 16,
                channels: 1,
                data: data.as_mut_ptr().cast(),
            };
            // raylib copies the samples into its own buffer.
            self.sounds.push(unsafe { ffi::LoadSoundFromWave(wave) });
        }
        true
    }

    fn close(&mut self) {
        if self.sounds.is_empty() {
            return;
        }
        unsafe {
            for sound in self.sounds.drain(..) {
                ffi::StopSound(sound);
                ffi::UnloadSound(sound);
            }
            ffi::CloseAudioDevice();
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_are_short_and_end_silent() {
        for sfx in Sfx::ALL {
            let s = samples(sfx);
            let seconds = s.len() as f32 / RATE as f32;
            assert!((0.02..0.15).contains(&seconds), "{sfx:?} lasts {seconds} s");
            assert!(s.iter().any(|&v| v.unsigned_abs() > 2000), "{sfx:?} is audible");
            assert!(s.first().unwrap().unsigned_abs() < 200 && s.last().unwrap().unsigned_abs() < 200, "{sfx:?} starts and ends without a click");
        }
        let peak = |sfx| samples(sfx).iter().map(|v| v.unsigned_abs()).max().unwrap();
        assert!(peak(Sfx::Volume) > peak(Sfx::Move), "the volume sound is the loudest");
    }
}
