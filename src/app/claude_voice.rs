//! Dictation in the chat: a recorder command writes the voice to a file while the button is
//! pressed; then a transcription command (whisper.cpp or another, set in the settings) turns it
//! into text, which goes at the end of the field. Nothing goes over the network from Florm.

use super::*;
use std::path::PathBuf;
use std::process::Child;
use std::time::Instant;

/// A recording in progress: the recorder process and the file it writes.
pub(super) struct Recording {
    child: Child,
    wav: PathBuf,
    pub(super) started: Instant,
}

impl Recording {
    /// Stops the recorder (so that the file is complete), then transcribes it. The file is removed.
    pub(super) fn finish(&mut self, transcribe: &str) -> Result<String, String> {
        stop(&mut self.child);
        let text = transcribe_file(transcribe, &self.wav);
        let _ = std::fs::remove_file(&self.wav);
        text
    }
}

impl Drop for Recording {
    /// A conversation dropped while recording: its recorder does not run on.
    fn drop(&mut self) {
        stop(&mut self.child);
        let _ = std::fs::remove_file(&self.wav);
    }
}

/// The program and its arguments of a setting, with `{wav}` replaced by `wav`.
fn command_args(template: &str, wav: &str) -> Option<(String, Vec<String>)> {
    let mut parts = template.split_whitespace().map(|p| p.replace("{wav}", wav));
    let program = parts.next()?;
    Some((program, parts.collect()))
}

/// Asks the recorder to stop (SIGINT, so that it closes the file), and waits for it.
fn stop(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = crate::os::command("kill").args(["-INT", &child.id().to_string()]).status();
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    let _ = child.wait();
}

/// Starts the recorder command writing into `wav`.
pub(super) fn start(record: &str, wav: &std::path::Path) -> Result<Child, String> {
    let (program, args) = command_args(record, &wav.display().to_string()).ok_or_else(|| t!("Commande d'enregistrement vide", "Empty recording command").to_string())?;
    crate::os::command(&program)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| tf!("« {program} » ne se lance pas : {e}", "« {program} » does not start: {e}"))
}

/// The text the transcription command writes for `wav`.
fn transcribe_file(template: &str, wav: &std::path::Path) -> Result<String, String> {
    let (program, args) = command_args(template, &wav.display().to_string()).ok_or_else(|| t!("Commande de transcription vide", "Empty transcription command").to_string())?;
    let out = crate::os::command(&program).args(&args).output().map_err(|e| tf!("« {program} » ne se lance pas : {e}", "« {program} » does not start: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err(t!("rien n'a été entendu", "nothing was heard").into());
    }
    Ok(text)
}

impl App {
    /// The dictation button: starts recording, or stops it and transcribes in the background.
    pub(super) fn claude_voice_toggle(&mut self) {
        let id = self.claude.id;
        if let Some(mut rec) = self.claude.voice.take() {
            let transcribe = self.settings.voice_transcribe.clone();
            self.set_status(t!("Transcription en cours…", "Transcribing…"));
            self.jobs.spawn(move || JobResult::ClaudeVoice(id, rec.finish(&transcribe)));
            return;
        }
        if self.settings.voice_transcribe.trim().is_empty() {
            self.set_status(t!("Dictée désactivée : indique voice_transcribe dans les réglages (settings.toml)", "Dictation off: set voice_transcribe in the settings (settings.toml)"));
            return;
        }
        let wav = std::env::temp_dir().join(format!("florm-voix-{id}.wav"));
        match start(&self.settings.voice_record, &wav) {
            Ok(child) => {
                self.claude.voice = Some(Recording { child, wav, started: Instant::now() });
                self.set_status(t!("Enregistrement : clique sur ■ Fin quand tu as fini", "Recording: click ■ End when you are done"));
            }
            Err(e) => self.set_error(e),
        }
    }

    /// The text of a dictation of conversation `id`: at the end of its field.
    pub(super) fn claude_voice_loaded(&mut self, id: u64, r: Result<String, String>) {
        let text = match r {
            Ok(text) => text,
            Err(e) => {
                self.set_error(tf!("Dictée : {e}", "Dictation: {e}"));
                return;
            }
        };
        let target = if self.claude.id == id { Some(&mut self.claude) } else { self.claude_others.iter_mut().find(|c| c.id == id) };
        let Some(c) = target else { return };
        if !c.input.is_empty() && !c.input.ends_with(char::is_whitespace) {
            c.input.push(' ');
        }
        c.input.push_str(&text);
        c.focus = true;
    }
}

#[cfg(test)]
mod tests {
    use super::command_args;

    #[test]
    fn the_file_name_goes_into_the_arguments() {
        assert_eq!(command_args("arecord -q -c 1 {wav}", "/tmp/v.wav"), Some(("arecord".into(), vec!["-q".into(), "-c".into(), "1".into(), "/tmp/v.wav".into()])));
        assert_eq!(command_args("   ", "x"), None);
    }
}
