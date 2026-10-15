//! Minimal embedded player: ffmpeg decodes video to raw RGBA and audio to f32 PCM
//! via two child processes. Audio is the master clock; video frames are shown
//! once their timestamp has been reached.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::i18n::Strings;
use crate::theme;
use eframe::egui;
use egui_phosphor::fill::{CORNERS_IN, CORNERS_OUT, PAUSE, PLAY, SPEAKER_HIGH};
use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// `ffmpeg` or `ffprobe`. A copy shipped with the app wins: release builds bundle them on Windows and
/// macOS, and an app started from Finder doesn't see Homebrew's PATH either.
pub fn tool(name: &str) -> Command {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let mut dirs = vec![];
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        dirs.push(dir.join("../Helpers")); // inside MyMediathek.app
        dirs.push(dir);
    }
    if cfg!(target_os = "macos") {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    }
    let path = dirs.into_iter().map(|d| d.join(&file)).find(|p| p.is_file()).unwrap_or_else(|| file.into());
    #[allow(unused_mut)]
    let mut cmd = Command::new(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console popping up per decoder
    }
    cmd
}

/// Some CDNs (e.g. ORF's) answer ffmpeg's default `Lavf/…` user agent with 403.
pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0";

/// `-user_agent …` for network sources; ffmpeg rejects it for local files.
pub fn net_args(source: &str) -> Vec<&'static str> {
    if source.starts_with("http") { vec!["-user_agent", USER_AGENT] } else { vec![] }
}

/// Fullscreen controls disappear after the mouse rests this long.
const CONTROLS_HIDE_SECS: f32 = 2.5;

/// Videos wider than this are scaled down before being piped to us.
const MAX_WIDTH: u32 = 1280;

#[derive(Clone)]
struct MediaInfo {
    width: u32,
    height: u32,
    fps: f64,
    duration: f64,
    has_audio: bool,
}

#[cfg(test)]
pub fn probe_for_test(source: &str) -> f64 {
    probe(source).unwrap().duration
}

fn probe(source: &str) -> Result<MediaInfo, String> {
    let out = tool("ffprobe")
        .args(net_args(source))
        .args(["-v", "error", "-show_streams", "-show_format", "-of", "json", source])
        .output()
        .map_err(|e| format!("ffprobe: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let streams = json["streams"].as_array().cloned().unwrap_or_default();
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .ok_or("no video stream")?;
    let fps = ["avg_frame_rate", "r_frame_rate"]
        .iter()
        .filter_map(|k| {
            let (n, d) = video[k].as_str()?.split_once('/')?;
            let fps = n.parse::<f64>().ok()? / d.parse::<f64>().ok()?;
            (fps.is_finite() && fps > 0.0 && fps < 200.0).then_some(fps)
        })
        .next()
        .unwrap_or(25.0);
    let (w, h) = (video["width"].as_u64().unwrap_or(0) as u32, video["height"].as_u64().unwrap_or(0) as u32);
    if w == 0 || h == 0 {
        return Err("unknown video size".into());
    }
    let width = w.min(MAX_WIDTH) & !1;
    let height = ((h as f64 * width as f64 / w as f64).round() as u32) & !1;
    Ok(MediaInfo {
        width,
        height,
        fps,
        duration: json["format"]["duration"].as_str().and_then(|d| d.parse().ok()).unwrap_or(0.0),
        has_audio: streams.iter().any(|s| s["codec_type"] == "audio"),
    })
}

struct Frame {
    pts: f64,
    rgba: Vec<u8>,
}

/// State shared between the audio reader thread and the cpal callback.
#[derive(Default)]
struct AudioShared {
    buf: Mutex<VecDeque<f32>>,
    /// Interleaved samples actually played.
    played: AtomicU64,
    paused: AtomicBool,
    eof: AtomicBool,
    volume: AtomicU32,
}

struct Audio {
    _stream: cpal::Stream,
    shared: Arc<AudioShared>,
    samples_per_sec: f64,
}

fn start_audio(source: &str, start: f64, volume: f32, children: &mut Vec<Child>) -> Result<Audio, String> {
    let device = cpal::default_host().default_output_device().ok_or("no audio device")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    let (rate, channels) = (config.sample_rate, config.channels);

    let mut child = tool("ffmpeg")
        .args(net_args(source))
        .args(["-v", "error", "-ss", &start.to_string(), "-i", source, "-vn"])
        .args(["-f", "f32le", "-ac", &channels.to_string(), "-ar", &rate.to_string(), "-"])
        .stdout(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let mut stdout = child.stdout.take().unwrap();
    children.push(child);

    let shared = Arc::new(AudioShared::default());
    shared.volume.store(volume.to_bits(), Ordering::Relaxed);
    let max_buffered = (rate as usize) * (channels as usize); // ~1s
    {
        let shared = shared.clone();
        std::thread::spawn(move || {
            let mut bytes = vec![0u8; 16 * 1024];
            let mut carry = Vec::new();
            loop {
                if shared.buf.lock().unwrap().len() > max_buffered {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                let n = match stdout.read(&mut bytes) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                carry.extend_from_slice(&bytes[..n]);
                let whole = carry.len() / 4 * 4;
                let mut buf = shared.buf.lock().unwrap();
                buf.extend(carry[..whole].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])));
                drop(buf);
                carry.drain(..whole);
            }
            shared.eof.store(true, Ordering::Relaxed);
        });
    }

    let cb_shared = shared.clone();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_stream::<f32>(&device, config, cb_shared),
        cpal::SampleFormat::I16 => build_stream::<i16>(&device, config, cb_shared),
        cpal::SampleFormat::U16 => build_stream::<u16>(&device, config, cb_shared),
        cpal::SampleFormat::I32 => build_stream::<i32>(&device, config, cb_shared),
        f => return Err(format!("unsupported sample format {f}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Audio { _stream: stream, shared, samples_per_sec: rate as f64 * channels as f64 })
}

fn build_stream<T>(device: &cpal::Device, config: cpal::StreamConfig, shared: Arc<AudioShared>) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    device
        .build_output_stream::<T, _, _>(
            config,
            move |out: &mut [T], _| {
                let silence = T::from_sample(0.0f32);
                if shared.paused.load(Ordering::Relaxed) {
                    out.fill(silence);
                    return;
                }
                let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
                let mut buf = shared.buf.lock().unwrap();
                let n = out.len().min(buf.len());
                for (o, s) in out.iter_mut().zip(buf.drain(..n)) {
                    *o = T::from_sample(s * volume);
                }
                out[n..].fill(silence);
                shared.played.fetch_add(n as u64, Ordering::Relaxed);
            },
            |e| eprintln!("audio error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

fn start_video(source: &str, start: f64, info: &MediaInfo, children: &mut Vec<Child>) -> Result<Receiver<Frame>, String> {
    let mut child = tool("ffmpeg")
        .args(net_args(source))
        .args(["-v", "error", "-ss", &start.to_string(), "-i", source, "-an", "-sn"])
        .args(["-vf", &format!("scale={}:{}", info.width, info.height)])
        .args(["-r", &info.fps.to_string(), "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdout(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let mut stdout = child.stdout.take().unwrap();
    children.push(child);

    let (tx, rx): (SyncSender<Frame>, _) = std::sync::mpsc::sync_channel(8);
    let frame_len = (info.width * info.height * 4) as usize;
    let fps = info.fps;
    std::thread::spawn(move || {
        for i in 0u64.. {
            let mut rgba = vec![0u8; frame_len];
            if stdout.read_exact(&mut rgba).is_err() {
                break;
            }
            if tx.send(Frame { pts: start + i as f64 / fps, rgba }).is_err() {
                break;
            }
        }
    });
    Ok(rx)
}

/// One run of the decoder processes, starting at `start`. Seeking replaces it.
struct Session {
    start: f64,
    frames: Receiver<Frame>,
    pending: Option<Frame>,
    shown_first: bool,
    video_eof: bool,
    paused: bool,
    audio: Option<Audio>,
    /// Fallback clock for videos without audio: (resumed at, position at that moment).
    wall: Option<(Instant, f64)>,
    children: Vec<Child>,
}

impl Session {
    fn new(source: &str, info: &MediaInfo, start: f64, volume: f32) -> Result<Self, String> {
        let mut children = vec![];
        let frames = start_video(source, start, info, &mut children)?;
        let audio = if info.has_audio {
            match start_audio(source, start, volume, &mut children) {
                Ok(a) => Some(a),
                Err(e) => {
                    eprintln!("audio disabled: {e}");
                    None
                }
            }
        } else {
            None
        };
        Ok(Self { start, frames, pending: None, shown_first: false, video_eof: false, paused: false, audio, wall: None, children })
    }

    fn clock(&self, paused: bool) -> f64 {
        match &self.audio {
            Some(a) => self.start + a.shared.played.load(Ordering::Relaxed) as f64 / a.samples_per_sec,
            None => match self.wall {
                Some((t, pos)) if !paused => pos + t.elapsed().as_secs_f64(),
                Some((_, pos)) => pos,
                None => self.start,
            },
        }
    }

    fn set_paused(&mut self, paused: bool) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        if let Some(a) = &self.audio {
            a.shared.paused.store(paused, Ordering::Relaxed);
        } else if let Some((t, pos)) = self.wall {
            self.wall = Some(if paused { (t, pos + t.elapsed().as_secs_f64()) } else { (Instant::now(), pos) });
        }
    }

    fn finished(&self) -> bool {
        self.video_eof
            && self.pending.is_none()
            && self.audio.as_ref().is_none_or(|a| a.shared.eof.load(Ordering::Relaxed) && a.shared.buf.lock().unwrap().is_empty())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        for c in &mut self.children {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

pub struct Player {
    pub title: String,
    source: String,
    info: Arc<Mutex<Option<Result<MediaInfo, String>>>>,
    session: Option<Session>,
    error: Option<String>,
    texture: Option<egui::TextureHandle>,
    paused: bool,
    volume: f32,
    position: f64,
    /// Slider value while the user is dragging it.
    scrub: Option<f64>,
    last_activity: Instant,
}

impl Player {
    pub fn new(ctx: &egui::Context, source: String, title: String) -> Self {
        let info: Arc<Mutex<Option<Result<MediaInfo, String>>>> = Default::default();
        {
            let (info, source, ctx) = (info.clone(), source.clone(), ctx.clone());
            std::thread::spawn(move || {
                *info.lock().unwrap() = Some(probe(&source));
                ctx.request_repaint();
            });
        }
        Self {
            title,
            source,
            info,
            session: None,
            error: None,
            texture: None,
            paused: false,
            volume: 1.0,
            position: 0.0,
            scrub: None,
            last_activity: Instant::now(),
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    fn seek(&mut self, info: &MediaInfo, to: f64) {
        let to = to.clamp(0.0, (info.duration - 1.0).max(0.0));
        self.session = None; // kill old decoders first
        match Session::new(&self.source, info, to, self.volume) {
            Ok(mut s) => {
                s.set_paused(self.paused);
                self.session = Some(s);
                self.position = to;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if let Some(s) = &mut self.session {
            s.set_paused(self.paused);
        }
    }

    /// Pulls due frames from the decoder and uploads the newest one.
    fn advance(&mut self, ctx: &egui::Context, info: &MediaInfo) {
        let Some(s) = &mut self.session else { return };
        let clock = s.clock(self.paused);
        let mut newest = None;
        loop {
            if s.pending.is_none() {
                match s.frames.try_recv() {
                    Ok(f) => s.pending = Some(f),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        s.video_eof = true;
                        break;
                    }
                }
            }
            // Always show the first frame immediately so seeking while paused updates the picture.
            if s.pending.as_ref().is_some_and(|f| f.pts <= clock || !s.shown_first) {
                newest = s.pending.take();
                if !s.shown_first {
                    s.shown_first = true;
                    break;
                }
            } else {
                break;
            }
        }
        if let Some(frame) = newest {
            // The wall clock starts with the first frame so startup latency doesn't cause skipping.
            if s.audio.is_none() && s.wall.is_none() {
                s.wall = Some((Instant::now(), frame.pts));
            }
            self.position = frame.pts;
            let image = egui::ColorImage::from_rgba_unmultiplied([info.width as usize, info.height as usize], &frame.rgba);
            match &mut self.texture {
                Some(t) => t.set(image, egui::TextureOptions::LINEAR),
                None => self.texture = Some(ctx.load_texture("video", image, egui::TextureOptions::LINEAR)),
            }
        }
        if s.audio.is_some() {
            self.position = s.clock(self.paused).max(self.position);
        }
        if s.finished() {
            self.paused = true;
        }
    }

    /// Shown when we can't play something ourselves.
    fn failed_ui(&self, ui: &mut egui::Ui, t: &Strings, detail: &str) {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(t.cannot_play).size(18.0));
            ui.label(egui::RichText::new(detail).small().weak());
            ui.add_space(10.0);
            if ui.add(theme::primary_button(t.open_other_app)).clicked() {
                play_external(&self.source);
            }
        });
    }

    /// Returns true if the user asked to toggle fullscreen.
    pub fn ui(&mut self, ui: &mut egui::Ui, t: &Strings, is_fullscreen: bool) -> bool {
        let info = match self.info.lock().unwrap().clone() {
            None => {
                ui.centered_and_justified(|ui| ui.spinner());
                return false;
            }
            Some(Err(e)) => {
                self.failed_ui(ui, t, &e);
                return false;
            }
            Some(Ok(info)) => info,
        };
        if self.session.is_none() && self.error.is_none() {
            self.seek(&info, 0.0);
        }
        if let Some(e) = &self.error {
            self.failed_ui(ui, t, e);
            return false;
        }
        let ctx = ui.ctx().clone();
        self.advance(&ctx, &info);
        if !self.paused {
            ctx.request_repaint();
        }

        let mut fullscreen = false;
        let mut seek_to = None;
        let mut toggle = false;
        ui.input(|i| {
            toggle = i.key_pressed(egui::Key::Space);
            if i.key_pressed(egui::Key::ArrowRight) {
                seek_to = Some(self.position + 10.0);
            }
            if i.key_pressed(egui::Key::ArrowLeft) {
                seek_to = Some(self.position - 10.0);
            }
            fullscreen |= i.key_pressed(egui::Key::F);
        });
        if toggle {
            self.toggle_pause();
        }

        // Windowed: the control bar sits below the video. Fullscreen: the video fills the
        // screen and the bar floats over it, hiding with the cursor when the mouse rests.
        let controls_h = 48.0;
        let avail = ui.available_rect_before_wrap();
        let (area, bar) = if is_fullscreen {
            let bar = egui::Rect::from_min_max(
                egui::pos2(avail.min.x + 24.0, avail.max.y - controls_h - 24.0),
                egui::pos2(avail.max.x - 24.0, avail.max.y - 24.0),
            );
            (avail, bar)
        } else {
            let split = avail.max.y - controls_h;
            (
                egui::Rect::from_min_max(avail.min, egui::pos2(avail.max.x, split)),
                egui::Rect::from_min_max(egui::pos2(avail.min.x, split), avail.max),
            )
        };
        let active = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO || i.pointer.any_down())
            || toggle
            || seek_to.is_some()
            || ui.rect_contains_pointer(bar);
        if active {
            self.last_activity = Instant::now();
        }
        let show_controls = !is_fullscreen || self.paused || self.last_activity.elapsed().as_secs_f32() < CONTROLS_HIDE_SECS;
        if !show_controls {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        }

        let resp = ui.allocate_rect(area, egui::Sense::click());
        ui.painter().rect_filled(area, 0.0, egui::Color32::BLACK);
        if let Some(tex) = &self.texture {
            let aspect = info.width as f32 / info.height as f32;
            let size = if area.width() / area.height() > aspect {
                egui::vec2(area.height() * aspect, area.height())
            } else {
                egui::vec2(area.width(), area.width() / aspect)
            };
            let rect = egui::Rect::from_center_size(area.center(), size);
            ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        } else {
            ui.put(area, egui::Spinner::new());
        }
        if resp.double_clicked() {
            fullscreen = true;
        } else if resp.clicked() {
            self.toggle_pause();
        }

        if show_controls {
            if is_fullscreen {
                ui.painter().rect_filled(bar, 16.0, theme::CARD);
            }
            let inner = bar.shrink2(egui::vec2(if is_fullscreen { 12.0 } else { 0.0 }, 0.0));
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(layout), |ui| {
                if ui.add(theme::primary_button(if self.paused { PLAY } else { PAUSE })).clicked() {
                    self.toggle_pause();
                }
                let shown = self.scrub.unwrap_or(self.position);
                ui.label(format!("{} / {}", fmt_time(shown), fmt_time(info.duration)));
                let right_w = 200.0;
                let mut value = shown;
                ui.spacing_mut().slider_width = (ui.available_width() - right_w).max(50.0);
                let slider = ui.add(egui::Slider::new(&mut value, 0.0..=info.duration.max(1.0)).show_value(false));
                if slider.dragged() || slider.changed() {
                    self.scrub = Some(value);
                }
                if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                    seek_to = Some(value);
                    self.scrub = None;
                }
                ui.label(SPEAKER_HIGH);
                ui.spacing_mut().slider_width = 80.0;
                if ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.5).show_value(false)).changed()
                    && let Some(a) = self.session.as_ref().and_then(|s| s.audio.as_ref())
                {
                    a.shared.volume.store(self.volume.to_bits(), Ordering::Relaxed);
                }
                let icon = if is_fullscreen { CORNERS_IN } else { CORNERS_OUT };
                if ui.button(icon).on_hover_text(t.fullscreen).clicked() {
                    fullscreen = true;
                }
            });
        }

        if let Some(t) = seek_to {
            self.seek(&info, t);
        }
        fullscreen
    }
}

fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Hands the video to mpv, or failing that to the system's default player.
pub fn play_external(target: &str) {
    if Command::new("mpv").arg(target).spawn().is_err() {
        let _ = open::that_detached(target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 3s 1920x1080 test pattern with a sine tone, generated by ffmpeg.
    fn sample() -> String {
        let path = std::env::temp_dir().join("mymediathek-test.mp4");
        if !path.exists() {
            let ok = Command::new("ffmpeg")
                .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=1920x1080:rate=25:duration=3"])
                .args(["-f", "lavfi", "-i", "sine=duration=3", "-shortest"])
                .arg(&path)
                .status()
                .unwrap()
                .success();
            assert!(ok);
        }
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn probe_scales_down() {
        let info = probe(&sample()).unwrap();
        assert_eq!((info.width, info.height), (1280, 720));
        assert_eq!(info.fps, 25.0);
        assert!(info.has_audio);
        assert!((info.duration - 3.0).abs() < 0.2);
    }

    #[test]
    fn decodes_frames_from_seek_point() {
        let src = sample();
        let info = probe(&src).unwrap();
        let mut children = vec![];
        let rx = start_video(&src, 1.0, &info, &mut children).unwrap();
        let frames: Vec<Frame> = rx.iter().collect();
        assert!((48..=52).contains(&frames.len()), "{} frames", frames.len());
        assert_eq!(frames[0].pts, 1.0);
        assert_eq!(frames[0].rgba.len(), 1280 * 720 * 4);
    }
}
