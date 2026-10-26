// Sound, notifications and settings.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

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

#[cfg(target_os = "macos")]
fn run(cmd: &str, args: &[&str]) {
    let _ = Command::new(cmd).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

fn chime_file() -> Option<&'static PathBuf> {
    static FILE: OnceLock<Option<PathBuf>> = OnceLock::new();
    FILE.get_or_init(|| render_chime().ok()).as_ref()
}

#[cfg(target_os = "macos")]
pub fn chime() {
    if let Some(file) = chime_file() {
        run("afplay", &[&file.to_string_lossy()]);
    }
}

#[cfg(windows)]
pub fn chime() {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
    if let Some(file) = chime_file() {
        let wide: Vec<u16> = file.as_os_str().encode_wide().chain([0]).collect();
        unsafe { PlaySoundW(wide.as_ptr(), std::ptr::null_mut(), SND_FILENAME | SND_ASYNC | SND_NODEFAULT) };
    }
}

/// JSON's string quoting is also AppleScript's, near enough for these fixed messages.
#[cfg(target_os = "macos")]
pub fn notify(title: &str, body: &str) {
    let script = format!(
        "display notification {} with title {}",
        serde_json::to_string(body).unwrap(),
        serde_json::to_string(title).unwrap()
    );
    run("osascript", &["-e", &script]);
}

/// A toast, shown through Windows PowerShell: a toast needs an app id the Start menu knows, and an
/// unpackaged app has none of its own. The fixed messages need no escaping in the XML or the
/// PowerShell string.
#[cfg(windows)]
pub fn notify(title: &str, body: &str) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const POWERSHELL_ID: &str = r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";
    let script = format!(
        "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
         [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom, ContentType = WindowsRuntime] > $null; \
         $x = New-Object Windows.Data.Xml.Dom.XmlDocument; \
         $x.LoadXml('<toast><visual><binding template=\"ToastGeneric\"><text>{title}</text><text>{body}</text></binding></visual></toast>'); \
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{POWERSHELL_ID}').Show([Windows.UI.Notifications.ToastNotification]::new($x))"
    );
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

// Settings go to a small JSON file of their own. DOODORO_SETTINGS points it elsewhere.
fn settings_file() -> PathBuf {
    if let Ok(path) = std::env::var("DOODORO_SETTINGS") {
        return path.into();
    }
    #[cfg(target_os = "macos")]
    let file = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Application Support/Doodoro/settings.json");
    #[cfg(windows)]
    let file = PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("Doodoro").join("settings.json");
    file
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
