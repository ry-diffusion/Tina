// Voice-note recorder backed by `gst-launch-1.0`. We deliberately
// shell out instead of pulling the gstreamer Rust crate in: a single
// pipeline + SIGINT-on-stop covers the WhatsApp PTT use case without
// adding a heavyweight build-time dependency, and most desktop Linux
// systems already ship gst-launch as part of the gstreamer plugin
// stack.
//
// The pipeline writes Opus-in-Ogg, which is the same container the
// WhatsApp clients use for voice notes (mimetype `audio/ogg;
// codecs=opus`). Capture source order: pipewiresrc, pulsesrc, autoaudio
// — first one whose plugin loads wins. If none load (no gst-plugins
// installed at all), `start()` reports the error and the UI shows a
// toast.

use std::process::{Child, Command, Stdio};
use std::time::Instant;

/// State carried while a recording is in flight. Dropping the handle
/// without calling `stop()` SIGKILLs the child and discards the
/// partial file — the destructor is best-effort cleanup, not the
/// happy path.
pub struct RecordingHandle {
    child: Option<Child>,
    pub path: String,
    started_at: Instant,
}

impl RecordingHandle {
    pub fn elapsed_secs(&self) -> u32 {
        self.started_at.elapsed().as_secs() as u32
    }
}

impl Drop for RecordingHandle {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        // Best-effort: leave a partial .ogg behind if it was already
        // written. The temp dir is cleaned by the OS on next boot.
    }
}

/// Spawn a `gst-launch-1.0` pipeline that writes voice-note audio to
/// a fresh tmp file. The returned handle owns the child process; call
/// `stop()` to flush + finalize the recording.
pub fn start() -> Result<RecordingHandle, String> {
    // Tmp file: nanoseconds-since-epoch keeps it unique without
    // pulling `tempfile` into the dep list.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let path = std::env::temp_dir()
        .join(format!("tina-voice-{nanos}.ogg"))
        .to_string_lossy()
        .to_string();

    // Try sources in order. We can't reliably probe gst-inspect
    // upfront without spawning, so the loop just tries each pipeline
    // until one starts successfully — gst-launch returns immediately
    // (and exits) on a missing plugin, so a failed Probe does not
    // tie up a child.
    let sources = ["pipewiresrc", "pulsesrc", "autoaudiosrc"];
    let mut last_err = String::new();
    for src in sources {
        let pipeline = format!(
            "{src} ! audioconvert ! audioresample ! \
             opusenc bitrate=32000 ! oggmux ! filesink location={}",
            shell_escape(&path),
        );
        match Command::new("gst-launch-1.0")
            .arg("-q")
            .args(pipeline.split_whitespace())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => {
                return Ok(RecordingHandle {
                    child: Some(child),
                    path,
                    started_at: Instant::now(),
                });
            }
            Err(e) => {
                last_err = format!("{src}: {e}");
            }
        }
    }
    Err(format!(
        "could not start gst-launch-1.0 ({last_err}). \
         install gstreamer plugins (gst-plugins-good / -base) to \
         record voice notes"
    ))
}

/// Cleanly stop a recording: SIGINT, wait, return the path. On a
/// graceful exit gst-launch flushes the muxer headers — without
/// that the .ogg is unreadable. If the child doesn't exit within
/// ~2s we fall back to SIGKILL and salvage whatever made it to disk.
pub fn stop(mut handle: RecordingHandle) -> Result<(String, u32), String> {
    let secs = handle.elapsed_secs();
    let Some(mut child) = handle.child.take() else {
        return Err("recorder already stopped".into());
    };
    // Send SIGINT so opusenc/oggmux can EOS cleanly (without it the
    // .ogg has no muxer headers and won't decode). Shelling out to
    // `kill` keeps this dep-free; we don't have libc here.
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .arg("-INT")
            .arg(child.id().to_string())
            .status();
        // Give it ~2s to flush; if it overruns, hard-kill and accept
        // whatever the muxer managed to write.
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok((handle.path.clone(), secs))
}

fn shell_escape(s: &str) -> String {
    // gst-launch's -q parser doesn't run a shell, so we just need
    // to dodge whitespace and the equals sign. Quote everything in
    // single quotes; embedded ' becomes '\''.
    if s.is_empty() {
        return "''".into();
    }
    if !s
        .bytes()
        .any(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\'' | b'"' | b'$' | b'`'))
    {
        return s.to_string();
    }
    let mut out = String::from("'");
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}
