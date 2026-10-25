// Sound, notifications and settings.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/**
 * The prototype's chime: triangle waves at E5, G5 and C6, 180 ms apart, each rising to 0.16 in
 * 20 ms and decaying exponentially to 0.001 over a second. Rendered once to a WAV file.
 */
fn render_chime() -> std::io::Result<PathBuf> {
    let rate = 44100u32;
    let notes = [659.25, 783.99, 1046.5];
    let length = (rate as f64 * (0.18 * 2.0 + 1.1)).ceil() as usize;
    let mut samples = vec![0f32; length];
    for (i, f) in notes.iter().enumerate() {
        let start = (rate as f64 * 0.18 * i as f64).floor() as usize;
        let mut j = 0;
        while (j as f64) < rate as f64 * 1.1 && start + j < length {
            let t = j as f64 / rate as f64;
            let gain = if t < 0.02 { 0.16 * t / 0.02 } else { 0.16 * (0.001f64 / 0.16).powf((t - 0.02) / 0.98) };
            let phase = (t * f) % 1.0;
            let tri = 4.0 * (phase - 0.5).abs() - 1.0;
            samples[start + j] += (tri * if t > 1.0 { 0.0 } else { gain }) as f32;
            j += 1;
        }
    }
    let mut buf = Vec::with_capacity(44 + length * 2);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + length as u32 * 2).to_le_bytes());
    buf.extend_from_slice(b"WAVEfmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&rate.to_le_bytes());
    buf.extend_from_slice(&(rate * 2).to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(length as u32 * 2).to_le_bytes());
    for s in samples {
        buf.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    let file = std::env::temp_dir().join("doodoro-chime.wav");
    std::fs::write(&file, buf)?;
    Ok(file)
}

fn run(cmd: &str, args: &[&str]) {
    let _ = Command::new(cmd).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

pub fn chime() {
    static FILE: OnceLock<Option<PathBuf>> = OnceLock::new();
    if let Some(file) = FILE.get_or_init(|| render_chime().ok()) {
        run("afplay", &[&file.to_string_lossy()]);
    }
}

/// JSON's string quoting is also AppleScript's, near enough for these fixed messages.
pub fn notify(title: &str, body: &str) {
    let script = format!(
        "display notification {} with title {}",
        serde_json::to_string(body).unwrap(),
        serde_json::to_string(title).unwrap()
    );
    run("osascript", &["-e", &script]);
}

// Settings go to a small JSON file of their own. DOODORO_SETTINGS points it elsewhere.
fn settings_file() -> PathBuf {
    if let Ok(path) = std::env::var("DOODORO_SETTINGS") {
        return path.into();
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/Doodoro/settings.json")
}

/// Session lengths in minutes as last set (focus, short, long), or None on first run.
pub fn load_durations() -> Option<[u32; 3]> {
    let text = std::fs::read_to_string(settings_file()).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let saved = &json["durations"];
    let get = |k: &str| saved[k].as_u64().filter(|n| (1..=90).contains(n)).map(|n| n as u32);
    Some([get("focus")?, get("short")?, get("long")?])
}

pub fn save_durations(d: [u32; 3]) {
    let file = settings_file();
    let json = serde_json::json!({ "durations": { "focus": d[0], "short": d[1], "long": d[2] } });
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, serde_json::to_string_pretty(&json).unwrap() + "\n");
}
