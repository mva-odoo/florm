//! Short sounds (Claude is done): made here as WAV files, played by the player of the system
//! (pw-play / paplay / aplay on Linux, afplay on macOS, PowerShell on Windows), without an
//! audio library.

use std::path::PathBuf;

/// The sounds: (key in the settings, French name, English name).
pub const SOUNDS: [(&str, &str, &str); 5] = [
    ("chime", "Carillon", "Chime"),
    ("bell", "Clochette", "Bell"),
    ("drop", "Goutte", "Drop"),
    ("beep", "Bip", "Beep"),
    ("chord", "Accord", "Chord"),
];

const RATE: u32 = 44_100;

/// A tone: frequency (Hz) at its start and at its end, start and length (seconds), how fast it
/// fades (per second), loudness.
struct Tone {
    from: f32,
    to: f32,
    at: f32,
    len: f32,
    fade: f32,
    gain: f32,
}

fn tones(name: &str) -> Vec<Tone> {
    let t = |f: f32, at: f32, len: f32, fade: f32, gain: f32| Tone { from: f, to: f, at, len, fade, gain };
    match name {
        // two notes going up
        "chime" => vec![t(784.0, 0.0, 0.45, 6.0, 0.5), t(1046.5, 0.14, 0.7, 5.0, 0.5)],
        // a bell: its note and an inharmonic partial
        "bell" => vec![t(1318.5, 0.0, 1.2, 3.5, 0.45), t(1318.5 * 2.76, 0.0, 0.5, 9.0, 0.15)],
        // a drop of water: a quick fall in pitch
        "drop" => vec![Tone { from: 1400.0, to: 450.0, at: 0.0, len: 0.18, fade: 14.0, gain: 0.6 }],
        // two short beeps
        "beep" => vec![t(880.0, 0.0, 0.12, 2.0, 0.35), t(880.0, 0.18, 0.12, 2.0, 0.35)],
        // a major chord, note after note
        _ => vec![t(523.3, 0.0, 0.8, 4.0, 0.3), t(659.3, 0.09, 0.75, 4.0, 0.3), t(784.0, 0.18, 0.7, 4.0, 0.3)],
    }
}

/// The sound `name` as a WAV file (16 bits, mono).
pub fn wav(name: &str) -> Vec<u8> {
    let tones = tones(name);
    let end = tones.iter().map(|t| t.at + t.len).fold(0.0, f32::max) + 0.05;
    let n = (end * RATE as f32) as usize;
    let mut samples = vec![0.0f32; n];
    for t in &tones {
        let (start, len) = ((t.at * RATE as f32) as usize, (t.len * RATE as f32) as usize);
        let mut phase = 0.0f32;
        for i in 0..len.min(n.saturating_sub(start)) {
            let x = i as f32 / RATE as f32;
            let f = t.from + (t.to - t.from) * (i as f32 / len as f32);
            phase += std::f32::consts::TAU * f / RATE as f32;
            // a short attack and end, no click
            let edge = (x / 0.004).min(1.0) * ((t.len - x) / 0.01).clamp(0.0, 1.0);
            samples[start + i] += phase.sin() * t.gain * (-t.fade * x).exp() * edge;
        }
    }
    let data: Vec<u8> = samples.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 0.8 * i16::MAX as f32) as i16).to_le_bytes()).collect();
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(1u16.to_le_bytes()); // mono
    out.extend(RATE.to_le_bytes());
    out.extend((RATE * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

/// The file of sound `name`, written the first time.
fn file(name: &str) -> Option<PathBuf> {
    let p = crate::config::data_dir().join("sounds").join(format!("{name}.wav"));
    if !p.exists() {
        std::fs::create_dir_all(p.parent()?).ok()?;
        std::fs::write(&p, wav(name)).ok()?;
    }
    Some(p)
}

/// Plays sound `name` (one of `SOUNDS`) without waiting; nothing when no player is found.
pub fn play(name: &str) {
    if !SOUNDS.iter().any(|s| s.0 == name) {
        return;
    }
    let name = name.to_string();
    std::thread::spawn(move || {
        let Some(p) = file(&name) else { return };
        let path = p.to_string_lossy().into_owned();
        let players: Vec<Vec<String>> = if cfg!(windows) {
            vec![vec!["powershell".into(), "-NoProfile".into(), "-Command".into(), format!("(New-Object Media.SoundPlayer '{}').PlaySync()", path.replace('\'', "''"))]]
        } else if cfg!(target_os = "macos") {
            vec![vec!["afplay".into(), path]]
        } else {
            vec![vec!["pw-play".into(), path.clone()], vec!["paplay".into(), path.clone()], vec!["aplay".into(), "-q".into(), path]]
        };
        for argv in players {
            let status = crate::os::command(&argv[0]).args(&argv[1..]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
            if status.is_ok_and(|s| s.success()) {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_is_a_short_wav() {
        for (name, ..) in SOUNDS {
            let w = wav(name);
            assert_eq!(&w[..4], b"RIFF");
            assert_eq!(&w[8..16], b"WAVEfmt ");
            let len = u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize;
            assert_eq!(len + 44, w.len());
            // between 0.1 and 1.5 seconds, not silent
            assert!((RATE as usize * 2 / 10..RATE as usize * 3).contains(&len), "{name}: {len}");
            assert!(w[44..].chunks(2).any(|s| i16::from_le_bytes([s[0], s[1]]).abs() > 8000), "{name}");
        }
    }
}
