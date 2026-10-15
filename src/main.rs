// No console window next to the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod i18n;
mod player;
mod theme;

use eframe::egui;
use egui_phosphor::fill::{
    CHECK_CIRCLE, DOWNLOAD_SIMPLE, STAR, FOLDER_OPEN, GEAR, MAGNIFYING_GLASS, MONITOR_PLAY, PLAY, TRASH,
    WARNING, X,
};
use i18n::{Lang, Strings};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const API: &str = "https://mediathekviewweb.de/api/query";

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Entry {
    channel: String,
    topic: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    timestamp: i64,
    #[serde(default)]
    duration: Option<serde_json::Value>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    url_video: String,
    #[serde(default)]
    url_video_low: String,
    #[serde(default)]
    url_video_hd: String,
}

impl Entry {
    fn url(&self, q: Quality) -> &str {
        let pick = match q {
            Quality::Hd => &self.url_video_hd,
            Quality::Normal => &self.url_video,
            Quality::Low => &self.url_video_low,
        };
        if pick.is_empty() { &self.url_video } else { pick }
    }

    fn duration_secs(&self) -> u64 {
        match &self.duration {
            Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0),
            Some(serde_json::Value::String(s)) => s.parse().unwrap_or(0),
            _ => 0,
        }
    }

    fn file_name(&self) -> String {
        let date = chrono::DateTime::from_timestamp(self.timestamp, 0)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let raw = format!("{} - {} - {} ({})", self.channel, self.topic, self.title, date);
        let clean: String = raw
            .chars()
            .map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '_' } else { c })
            .collect();
        format!("{}.mp4", clean.chars().take(200).collect::<String>().trim())
    }
}

#[derive(Deserialize)]
struct ApiResponse {
    result: Option<ApiResult>,
    err: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ApiResult {
    results: Vec<Entry>,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
enum Quality {
    Hd,
    Normal,
    Low,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct Settings {
    lang: Lang,
    quality: Quality,
}

impl Settings {
    fn path() -> Option<PathBuf> {
        config_file("settings.json")
    }

    fn load() -> Self {
        if let Some(config) = dirs::config_dir() {
            migrate(&config.join("localthek"), &config.join("mymediathek"));
        }
        Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Settings { lang: Lang::detect(), quality: Quality::Hd })
    }

    fn save(&self) {
        if let Some(p) = Self::path() {
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            let _ = std::fs::write(p, serde_json::to_vec_pretty(self).unwrap());
        }
    }
}

fn config_file(name: &str) -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("mymediathek").join(name))
}

/// Remembered videos, newest first, kept as full entries so they work without searching again.
fn load_bookmarks() -> Vec<Entry> {
    config_file("bookmarks.json")
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_bookmarks(bookmarks: &[Entry]) {
    if let Some(p) = config_file("bookmarks.json") {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(p, serde_json::to_vec_pretty(bookmarks).unwrap());
    }
}

#[derive(PartialEq)]
enum Tab {
    Search,
    Favourites,
    Library,
    Player,
    Settings,
}

enum DlState {
    Running { done: u64, total: Option<u64> },
    Finished,
    Failed(String),
}

struct Download {
    title: String,
    target: PathBuf,
    state: DlState,
}

type Shared<T> = Arc<Mutex<T>>;

struct App {
    tab: Tab,
    settings: Settings,
    query: String,
    results: Shared<Option<Result<Vec<Entry>, String>>>,
    searching: Shared<bool>,
    downloads: Shared<Vec<Download>>,
    dir: PathBuf,
    library: Vec<PathBuf>,
    bookmarks: Vec<Entry>,
    /// Deleting takes two clicks: this holds the file awaiting confirmation.
    confirm_delete: Option<PathBuf>,
    player: Option<player::Player>,
    fullscreen: bool,
    /// Finished downloads at the last library refresh, to spot newly finished ones.
    finished_seen: usize,
}

impl App {
    fn new() -> Self {
        let dir = dirs::video_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("MyMediathek");
        migrate(&dir.with_file_name("localthek"), &dir);
        let _ = std::fs::create_dir_all(&dir);
        let mut app = Self {
            tab: Tab::Search,
            settings: Settings::load(),
            query: String::new(),
            results: Default::default(),
            searching: Default::default(),
            downloads: Default::default(),
            dir,
            library: vec![],
            bookmarks: load_bookmarks(),
            confirm_delete: None,
            player: None,
            fullscreen: false,
            finished_seen: 0,
        };
        app.refresh_library();
        app
    }

    fn t(&self) -> &'static Strings {
        self.settings.lang.t()
    }

    fn refresh_library(&mut self) {
        self.library = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "mp4"))
            .collect();
        self.library.sort();
    }

    fn open_player(&mut self, ctx: &egui::Context, source: String, title: String) {
        self.player = Some(player::Player::new(ctx, source, title));
        self.tab = Tab::Player;
    }

    fn search(&self, ctx: &egui::Context) {
        let query = build_query(&self.query);
        let results = self.results.clone();
        let searching = self.searching.clone();
        let ctx = ctx.clone();
        *searching.lock().unwrap() = true;
        std::thread::spawn(move || {
            let res = run_query(&query);
            if let Err(e) = &res {
                eprintln!("search failed: {e}");
            }
            *results.lock().unwrap() = Some(res);
            *searching.lock().unwrap() = false;
            ctx.request_repaint();
        });
    }

    fn download(&self, ctx: &egui::Context, entry: &Entry) {
        let url = entry.url(self.settings.quality).to_string();
        let target = self.dir.join(entry.file_name());
        let downloads = self.downloads.clone();
        let ctx = ctx.clone();
        let idx = {
            let mut d = downloads.lock().unwrap();
            let state = DlState::Running { done: 0, total: entry.size };
            d.push(Download { title: entry.title.clone(), target: target.clone(), state });
            d.len() - 1
        };
        std::thread::spawn(move || {
            let set = |s: DlState| {
                downloads.lock().unwrap()[idx].state = s;
                ctx.request_repaint();
            };
            let progress = |done, total| set(DlState::Running { done, total });
            let res = if url.contains(".m3u8") {
                fetch_ffmpeg(&url, &target)
            } else {
                fetch_http(&url, &target, progress)
            };
            set(match res {
                Ok(()) => DlState::Finished,
                Err(e) => DlState::Failed(e),
            });
        });
    }

    fn is_bookmarked(&self, e: &Entry) -> bool {
        self.bookmarks.iter().any(|b| b.url_video == e.url_video)
    }

    fn toggle_bookmark(&mut self, e: &Entry) {
        if self.is_bookmarked(e) {
            self.bookmarks.retain(|b| b.url_video != e.url_video);
        } else {
            self.bookmarks.insert(0, e.clone());
        }
        save_bookmarks(&self.bookmarks);
    }

    /// Progress (0..1) if `target` is currently being downloaded.
    fn download_progress(&self, target: &Path) -> Option<f32> {
        self.downloads.lock().unwrap().iter().find_map(|d| match d.state {
            DlState::Running { done, total } if d.target == target => {
                Some(total.map(|t| done as f32 / t.max(1) as f32).unwrap_or(0.0))
            }
            _ => None,
        })
    }
}

/// Supports mediathekviewweb-ish syntax: `!channel #topic +title rest`.
fn build_query(input: &str) -> serde_json::Value {
    let mut queries = vec![];
    let mut free = vec![];
    for tok in input.split_whitespace() {
        let (field, rest) = match tok.chars().next() {
            Some('!') => (vec!["channel"], &tok[1..]),
            Some('#') => (vec!["topic"], &tok[1..]),
            Some('+') => (vec!["title"], &tok[1..]),
            _ => {
                free.push(tok);
                continue;
            }
        };
        if !rest.is_empty() {
            queries.push(serde_json::json!({"fields": field, "query": rest.replace(',', " ")}));
        }
    }
    if !free.is_empty() {
        queries.push(serde_json::json!({"fields": ["title", "topic"], "query": free.join(" ")}));
    }
    serde_json::json!({
        "queries": queries,
        "sortBy": "timestamp",
        "sortOrder": "desc",
        "future": false,
        "offset": 0,
        "size": 100,
    })
}

fn run_query(query: &serde_json::Value) -> Result<Vec<Entry>, String> {
    let resp: ApiResponse = ureq::post(API)
        .header("Content-Type", "text/plain")
        .header("User-Agent", player::USER_AGENT)
        .send(query.to_string())
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
    if let Some(err) = resp.err.filter(|e| !e.is_null()) {
        return Err(err.to_string());
    }
    Ok(resp.result.map(|r| r.results).unwrap_or_default())
}

fn fetch_http(url: &str, target: &Path, progress: impl Fn(u64, Option<u64>)) -> Result<(), String> {
    let mut resp = ureq::get(url).header("User-Agent", player::USER_AGENT).call().map_err(|e| e.to_string())?;
    let total = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let mut reader = resp.body_mut().with_config().limit(u64::MAX).reader();
    let part = target.with_extension("mp4.part");
    let res = copy_with_progress(&mut reader, &part, total, progress).and_then(|()| std::fs::rename(&part, target).map_err(|e| e.to_string()));
    if res.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    res
}

fn copy_with_progress(reader: &mut impl Read, part: &Path, total: Option<u64>, progress: impl Fn(u64, Option<u64>)) -> Result<(), String> {
    let mut file = std::fs::File::create(part).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    let mut last = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if done - last > 1 << 20 {
            last = done;
            progress(done, total);
        }
    }
    Ok(())
}

/// HLS streams are muxed into an mp4 by ffmpeg.
fn fetch_ffmpeg(url: &str, target: &Path) -> Result<(), String> {
    let part = target.with_extension("part.mp4");
    let out = player::tool("ffmpeg")
        .args(player::net_args(url))
        .args(["-y", "-loglevel", "error", "-i", url, "-c", "copy"])
        .arg(&part)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&part);
        // The last line is ffmpeg's summary of what went wrong.
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(stderr.lines().last().unwrap_or("ffmpeg failed").to_string());
    }
    std::fs::rename(&part, target).map_err(|e| e.to_string())
}

/// Moves data left under the app's former name ("localthek") to its new place.
fn migrate(old: &Path, new: &Path) {
    if old.is_dir() && !new.exists() {
        let _ = std::fs::rename(old, new);
    }
}

fn human_bytes(b: u64) -> String {
    format!("{:.0} MB", b as f64 / 1_048_576.0)
}

/// The first `n` sentences of `text`, and whether anything was cut off.
fn first_sentences(text: &str, n: usize) -> (&str, bool) {
    let mut count = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        // A terminator only ends a sentence when followed by whitespace, so "15.10.2026" stays whole.
        if matches!(c, '.' | '!' | '?') && chars.peek().is_none_or(|(_, next)| next.is_whitespace()) {
            count += 1;
            if count == n {
                let end = i + c.len_utf8();
                return (&text[..end], end < text.trim_end().len());
            }
        }
    }
    (text, false)
}

fn margin(left: i8, right: i8, top: i8, bottom: i8) -> egui::Margin {
    egui::Margin { left, right, top, bottom }
}

fn medium(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).family(egui::FontFamily::Name("plex-medium".into()))
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::GROUND.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.painter().rect_filled(ui.ctx().content_rect(), 0.0, theme::GROUND);
        let finished = self.downloads.lock().unwrap().iter().filter(|d| matches!(d.state, DlState::Finished)).count();
        if finished != self.finished_seen {
            self.finished_seen = finished;
            self.refresh_library();
        }
        let player_shown = self.tab == Tab::Player && self.player.is_some();
        if self.fullscreen && (!player_shown || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
            self.set_fullscreen(ui.ctx(), false);
        }

        if !self.fullscreen {
            egui::Panel::top("top")
                .frame(theme::card().outer_margin(margin(16, 16, 16, 10)))
                .show(ui, |ui| self.top_ui(ui));
            if !self.downloads.lock().unwrap().is_empty() {
                egui::Panel::bottom("downloads")
                    .frame(theme::card().outer_margin(margin(16, 16, 6, 16)).inner_margin(egui::Margin::same(8)))
                    .show(ui, |ui| self.downloads_ui(ui));
            }
        }

        let frame = if self.fullscreen {
            egui::Frame::new().fill(egui::Color32::BLACK)
        } else if self.tab == Tab::Search {
            theme::card().outer_margin(margin(16, 16, 6, 16)).inner_margin(egui::Margin::same(8))
        } else {
            theme::card().outer_margin(margin(16, 16, 6, 16)).inner_margin(egui::Margin::same(20))
        };
        let t = self.t();
        egui::CentralPanel::default().frame(frame).show(ui, |ui| match self.tab {
            Tab::Search => self.results_ui(ui),
            Tab::Favourites => self.favourites_ui(ui),
            Tab::Library => self.library_ui(ui),
            Tab::Settings => self.settings_ui(ui),
            Tab::Player => {
                if let Some(p) = &mut self.player
                    && p.ui(ui, t, self.fullscreen)
                {
                    let fs = !self.fullscreen;
                    self.set_fullscreen(ui.ctx(), fs);
                }
            }
        });
    }
}

impl App {
    fn set_fullscreen(&mut self, ctx: &egui::Context, on: bool) {
        self.fullscreen = on;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
    }

    /// Tabs, plus the big search bar while searching.
    fn top_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Search, format!("{MAGNIFYING_GLASS} {}", t.tab_search));
            ui.selectable_value(&mut self.tab, Tab::Favourites, format!("{STAR} {}", t.tab_favourites));
            if ui.selectable_value(&mut self.tab, Tab::Library, format!("{DOWNLOAD_SIMPLE} {}", t.tab_library)).clicked() {
                self.refresh_library();
            }
            if let Some(p) = &self.player {
                let mut title: String = p.title.chars().take(36).collect();
                if title.len() < p.title.len() {
                    title.push('…');
                }
                ui.selectable_value(&mut self.tab, Tab::Player, format!("{MONITOR_PLAY} {title}"));
                if ui.button(X).on_hover_text(t.close).clicked() {
                    self.player = None;
                    if self.tab == Tab::Player {
                        self.tab = Tab::Search;
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.selectable_value(&mut self.tab, Tab::Settings, GEAR).on_hover_text(t.tab_settings);
            });
        });
        if self.tab != Tab::Search {
            return;
        }
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text(t.search_hint)
                    .font(egui::FontId::proportional(20.0))
                    .margin(egui::vec2(14.0, 10.0))
                    .desired_width((ui.available_width() - 160.0).max(200.0)),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let button = theme::primary_button(format!("{MAGNIFYING_GLASS} {}", t.search)).min_size(egui::vec2(130.0, 46.0));
            if (ui.add(button).clicked() || enter) && !self.query.trim().is_empty() {
                self.search(&ctx);
            }
            if *self.searching.lock().unwrap() {
                ui.spinner();
            }
        });
    }

    fn results_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        let results = self.results.lock().unwrap().clone();
        let hint = |ui: &mut egui::Ui, text: &str| {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| ui.label(egui::RichText::new(text).size(17.0).weak()));
        };
        match results {
            None => hint(ui, t.start_hint),
            Some(Err(_)) => hint(ui, &format!("{WARNING} {}", t.search_failed)),
            Some(Ok(entries)) if entries.is_empty() => hint(ui, t.nothing_found),
            Some(Ok(entries)) => {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for e in &entries {
                        self.entry_row(ui, &ctx, e);
                    }
                });
            }
        }
    }

    /// One line per result: channel, title – first sentence, length, actions.
    /// Hovering the row shows everything in a bubble.
    fn entry_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, e: &Entry) {
        let t = self.t();
        let local = self.dir.join(e.file_name());
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), egui::Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }
        if resp.hovered() {
            ui.painter().rect_filled(rect, 10.0, theme::FIELD);
        }

        // Actions and length, right-aligned; the text gets whatever is left.
        let inner = rect.shrink2(egui::vec2(8.0, 0.0));
        let actions = ui.scope_builder(
            egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.spacing_mut().button_padding = egui::vec2(10.0, 4.0);
                let small = egui::vec2(38.0, 32.0);
                if local.exists() {
                    ui.add_sized(small, egui::Label::new(egui::RichText::new(CHECK_CIRCLE).color(theme::GREEN)))
                        .on_hover_text(t.saved);
                } else if let Some(p) = self.download_progress(&local) {
                    ui.add_sized(small, theme::progress_bar(p).desired_height(8.0)).on_hover_text(t.saving);
                } else if ui.add_sized(small, egui::Button::new(DOWNLOAD_SIMPLE)).on_hover_text(t.save).clicked() {
                    self.download(ctx, e);
                }
                // Purple only when it's on disk and plays without internet.
                let watch = if local.exists() { theme::primary_button(PLAY) } else { egui::Button::new(PLAY) };
                if ui.add_sized(small, watch).on_hover_text(t.watch).clicked() {
                    // Play the saved copy when there is one, so it works offline.
                    let source = if local.exists() {
                        local.to_string_lossy().into_owned()
                    } else {
                        e.url(self.settings.quality).to_string()
                    };
                    self.open_player(ctx, source, e.title.clone());
                }
                let marked = self.is_bookmarked(e);
                let star = egui::RichText::new(STAR).color(if marked { theme::GOLD } else { theme::TRACK_OFF });
                let star = ui.add_sized(small, egui::Button::new(star.size(18.0)).frame(false));
                if star.on_hover_text(if marked { t.unbookmark } else { t.bookmark }).clicked() {
                    self.toggle_bookmark(e);
                }
                ui.add_space(6.0);
                ui.weak(format!("{} {}", e.duration_secs() / 60, t.minutes));
                ui.min_rect().left()
            },
        );
        let text_right = actions.inner - 14.0;

        let painter = ui.painter().with_clip_rect(egui::Rect::from_min_max(inner.min, egui::pos2(text_right, inner.max.y)));
        let y = rect.center().y;
        let body = egui::TextStyle::Body.resolve(ui.style());
        let bold = egui::FontId::new(body.size, egui::FontFamily::Name("plex-medium".into()));
        painter.text(egui::pos2(inner.left(), y), egui::Align2::LEFT_CENTER, &e.channel, egui::TextStyle::Small.resolve(ui.style()), theme::ACCENT);
        let title = painter.text(egui::pos2(inner.left() + CHANNEL_W, y), egui::Align2::LEFT_CENTER, &e.title, bold, theme::TEXT);
        let (summary, _) = first_sentences(e.description.trim(), 1);
        let mut text_end = title.right();
        if !summary.is_empty() {
            let pos = egui::pos2(title.right() + 8.0, y);
            text_end = painter.text(pos, egui::Align2::LEFT_CENTER, format!("– {summary}"), body, ui.visuals().weak_text_color()).right();
        }
        // Fade instead of a hard cut where the text runs into the actions.
        let fade_w = 40.0;
        if text_end <= text_right {
            return show_entry_bubble(resp, e, t);
        }
        let bg = if resp.hovered() { theme::FIELD } else { theme::CARD };
        let fade = egui::Rect::from_min_max(egui::pos2(text_right - fade_w, rect.top()), egui::pos2(text_right, rect.bottom()));
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(fade.left_top(), bg.gamma_multiply(0.0));
        mesh.colored_vertex(fade.right_top(), bg);
        mesh.colored_vertex(fade.right_bottom(), bg);
        mesh.colored_vertex(fade.left_bottom(), bg.gamma_multiply(0.0));
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        ui.painter().add(mesh);
        show_entry_bubble(resp, e, t);
    }

    fn empty_hint(ui: &mut egui::Ui, text: &str) {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| ui.label(egui::RichText::new(text).size(17.0).weak()));
    }

    /// Starred videos, newest first, with the same rows as search.
    fn favourites_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        ui.heading(t.tab_favourites);
        if self.bookmarks.is_empty() {
            return Self::empty_hint(ui, t.no_favourites);
        }
        let ctx = ui.ctx().clone();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for e in self.bookmarks.clone() {
                self.entry_row(ui, &ctx, &e);
            }
        });
    }

    fn library_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        ui.heading(t.tab_library);
        if self.library.is_empty() {
            return Self::empty_hint(ui, t.no_downloads);
        }
        let mut delete = None;
        let mut play = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for path in &self.library {
                ui.horizontal(|ui| {
                    if ui.add(theme::primary_button(PLAY).min_size(egui::vec2(34.0, 30.0))).on_hover_text(t.watch).clicked() {
                        play = Some(path.clone());
                    }
                    if self.confirm_delete.as_ref() == Some(path) {
                        let really = egui::Button::new(egui::RichText::new(t.really_delete).color(egui::Color32::WHITE)).fill(theme::RED);
                        if ui.add(really).clicked() {
                            delete = Some(path.clone());
                        }
                        if ui.button(X).clicked() {
                            self.confirm_delete = None;
                        }
                    } else if ui.button(TRASH).on_hover_text(t.delete).clicked() {
                        self.confirm_delete = Some(path.clone());
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(medium(path.file_stem().unwrap_or_default().to_string_lossy()));
                        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                        ui.label(egui::RichText::new(human_bytes(size)).small().weak());
                    });
                });
            }
        });
        if let Some(p) = play {
            let title = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            self.open_player(ui.ctx(), p.to_string_lossy().into_owned(), title);
        }
        if let Some(p) = delete {
            if self.player.as_ref().is_some_and(|pl| Path::new(pl.source()) == p) {
                self.player = None;
            }
            let _ = std::fs::remove_file(p);
            self.confirm_delete = None;
            self.refresh_library();
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let before = self.settings.clone();
        ui.heading(t.tab_settings);
        ui.add_space(6.0);
        egui::Grid::new("settings").num_columns(2).spacing([24.0, 18.0]).show(ui, |ui| {
            ui.label(medium(t.language));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.settings.lang, Lang::De, "Deutsch");
                ui.selectable_value(&mut self.settings.lang, Lang::En, "English");
            });
            ui.end_row();

            ui.label(medium(t.quality));
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.settings.quality, Quality::Hd, t.quality_high);
                    ui.selectable_value(&mut self.settings.quality, Quality::Normal, t.quality_medium);
                    ui.selectable_value(&mut self.settings.quality, Quality::Low, t.quality_low);
                });
                ui.label(egui::RichText::new(t.quality_note).small().weak());
            });
            ui.end_row();

            ui.label(medium(t.save_location));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(self.dir.display().to_string()).monospace());
                if ui.button(format!("{FOLDER_OPEN} {}", t.open_folder)).clicked() {
                    let _ = open::that_detached(&self.dir);
                }
            });
            ui.end_row();
        });
        ui.add_space(20.0);
        ui.label(egui::RichText::new(t.data_source).small().weak());
        if self.settings != before {
            self.settings.save();
        }
    }

    /// One compact chip per download, wrapping side by side; progress is the chip's fill.
    /// Finished chips play the video when clicked.
    fn downloads_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let mut play = None;
        let mut downloads = self.downloads.lock().unwrap();
        // Two rows stay visible; more scroll.
        egui::ScrollArea::vertical().max_height(CHIP_H * 2.0 + CHIP_GAP).show(ui, |ui| {
            // Set before horizontal_wrapped, which fixes its row height on entry.
            ui.spacing_mut().item_spacing = egui::vec2(CHIP_GAP, CHIP_GAP);
            ui.spacing_mut().interact_size.y = CHIP_H;
            ui.horizontal_wrapped(|ui| {
                // Equal-width chips filling the row, leaving room for the clear button.
                let avail = ui.available_width() - CHIP_H - CHIP_GAP;
                let per_row = (avail / CHIP_MIN_W).floor().max(1.0);
                let size = egui::vec2((avail - (per_row - 1.0) * CHIP_GAP) / per_row, CHIP_H);
                for d in downloads.iter() {
                    if let Some(target) = download_chip(ui, t, d, size) {
                        play = Some((target, d.title.clone()));
                    }
                }
                if downloads.iter().any(|d| !matches!(d.state, DlState::Running { .. })) && clear_chip(ui, t) {
                    downloads.retain(|d| matches!(d.state, DlState::Running { .. }));
                }
            });
        });
        drop(downloads);
        if let Some((target, title)) = play {
            self.open_player(ui.ctx(), target.to_string_lossy().into_owned(), title);
        }
    }
}

/// Square chip that removes finished and failed downloads.
fn clear_chip(ui: &mut egui::Ui, t: &Strings) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(CHIP_H, CHIP_H), egui::Sense::click());
    let bg = if resp.hovered() { theme::ACCENT_SOFT } else { theme::FIELD };
    ui.painter().rect_filled(rect, 10.0, bg);
    let font = egui::TextStyle::Small.resolve(ui.style());
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, X, font, theme::TEXT);
    resp.on_hover_text(t.clear_finished).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Everything about a result, shown when hovering its row.
fn show_entry_bubble(resp: egui::Response, e: &Entry, t: &Strings) {
    resp.on_hover_ui(|ui| {
        ui.set_max_width(460.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        let date = chrono::DateTime::from_timestamp(e.timestamp, 0)
            .map(|d| d.with_timezone(&chrono::Local).format("%d.%m.%Y %H:%M").to_string())
            .unwrap_or_default();
        ui.label(medium(&e.title).size(17.0));
        ui.horizontal_wrapped(|ui| {
            ui.label(medium(&e.channel).color(theme::ACCENT));
            ui.weak(format!("{} · {date} · {} {}", e.topic, e.duration_secs() / 60, t.minutes));
        });
        if !e.description.is_empty() {
            ui.label(&e.description);
        }
    });
}

const ROW_H: f32 = 40.0;
/// Column for the channel name in result rows.
const CHANNEL_W: f32 = 64.0;
const CHIP_H: f32 = 28.0;
const CHIP_MIN_W: f32 = 230.0;
const CHIP_GAP: f32 = 6.0;

/// Paints one download; returns its file if a finished download was clicked.
fn download_chip(ui: &mut egui::Ui, t: &Strings, d: &Download, size: egui::Vec2) -> Option<PathBuf> {
    let finished = matches!(d.state, DlState::Finished);
    let sense = if finished { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let (icon, color, status) = match &d.state {
        DlState::Running { done, total } => {
            let frac = total.map(|t| (*done as f32 / t.max(1) as f32).min(1.0));
            let fill = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * frac.unwrap_or(0.0), rect.height()));
            ui.painter().rect_filled(rect, 10.0, theme::FIELD);
            ui.painter().rect_filled(fill, 10.0, theme::ACCENT_SOFT);
            let status = frac.map(|f| format!("{:.0}%", f * 100.0)).unwrap_or_else(|| human_bytes(*done));
            (DOWNLOAD_SIMPLE, theme::ACCENT, status)
        }
        DlState::Finished => {
            let bg = if resp.hovered() { theme::GREEN.gamma_multiply(0.25) } else { theme::GREEN.gamma_multiply(0.12) };
            ui.painter().rect_filled(rect, 10.0, bg);
            (CHECK_CIRCLE, theme::GREEN, String::new())
        }
        DlState::Failed(_) => {
            ui.painter().rect_filled(rect, 10.0, theme::RED.gamma_multiply(0.12));
            (WARNING, theme::RED, t.failed.to_string())
        }
    };
    let font = egui::TextStyle::Small.resolve(ui.style());
    let painter = ui.painter();
    let left = rect.left() + 9.0;
    painter.text(egui::pos2(left, rect.center().y), egui::Align2::LEFT_CENTER, icon, font.clone(), color);
    let status_rect = painter.text(egui::pos2(rect.right() - 9.0, rect.center().y), egui::Align2::RIGHT_CENTER, &status, font.clone(), color);
    // Title is clipped to the space between icon and status.
    let clip = egui::Rect::from_min_max(egui::pos2(left + 20.0, rect.top()), egui::pos2(status_rect.left() - 6.0, rect.bottom()));
    painter.with_clip_rect(clip).text(egui::pos2(clip.left(), rect.center().y), egui::Align2::LEFT_CENTER, &d.title, font, theme::TEXT);

    let resp = match &d.state {
        DlState::Failed(e) => resp.on_hover_text(format!("{}\n{e}", d.title)),
        DlState::Finished => resp.on_hover_text(format!("{PLAY} {}", d.title)).on_hover_cursor(egui::CursorIcon::PointingHand),
        DlState::Running { .. } => resp.on_hover_text(&d.title),
    };
    (finished && resp.clicked()).then(|| d.target.clone())
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 720.0])
            .with_app_id("mymediathek")
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")).unwrap_or_default()),
        ..Default::default()
    };
    eframe::run_native(
        "MyMediathek",
        options,
        Box::new(|cc| {
            theme::setup(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(channel: &str, topic: &str, title: &str, description: &str, minutes: u64) -> Entry {
        Entry {
            channel: channel.into(),
            topic: topic.into(),
            title: title.into(),
            description: description.into(),
            timestamp: 1_792_015_500,
            duration: Some(serde_json::json!(minutes * 60)),
            size: None,
            url_video: format!("https://example.org/{title}.mp4"),
            url_video_low: String::new(),
            url_video_hd: String::new(),
        }
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mymediathek-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn hls_download_remuxes_to_mp4() {
        let dir = scratch_dir("hls");
        let playlist = dir.join("stream.m3u8");
        assert!(std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=25:duration=4"])
            .args(["-f", "lavfi", "-i", "sine=duration=4", "-shortest", "-c:v", "libx264", "-c:a", "aac"])
            .args(["-f", "hls", "-hls_time", "1", "-hls_list_size", "0"])
            .arg(&playlist)
            .status()
            .unwrap()
            .success());
        let target = dir.join("Kanal - Thema - Titel (2026-10-15).mp4");
        fetch_ffmpeg(&playlist.to_string_lossy(), &target).unwrap();
        let info = player::probe_for_test(&target.to_string_lossy());
        assert!((info - 4.0).abs() < 0.5, "duration {info}");
        // Only the finished file is left behind.
        assert_eq!(std::fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "mp4")).count(), 1);
    }

    #[test]
    fn failed_hls_download_reports_reason_and_cleans_up() {
        let dir = scratch_dir("hls-fail");
        let target = dir.join("x.mp4");
        let err = fetch_ffmpeg(&dir.join("missing.m3u8").to_string_lossy(), &target).unwrap_err();
        assert!(err.contains("No such file"), "{err}");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn bookmarks_survive_a_round_trip() {
        let e = entry("KiKA", "Löwenzahn", "Fritz", "Beschreibung.", 25);
        let json = serde_json::to_vec(&vec![e.clone()]).unwrap();
        let back: Vec<Entry> = serde_json::from_slice(&json).unwrap();
        assert_eq!(back[0].title, e.title);
        assert_eq!(back[0].duration_secs(), 25 * 60);
        assert_eq!(back[0].file_name(), e.file_name());
    }

    #[test]
    fn file_names_are_safe() {
        let e = entry("ARD", "a/b: c", "Was? \"Ja\" <oder> nein|", "", 1);
        let name = e.file_name();
        assert!(!name.contains(['/', ':', '?', '"', '<', '>', '|', '\\']), "{name}");
        assert!(name.ends_with(".mp4"));
    }

    /// Opens the newest videos of every big channel the way the app would, so a CDN
    /// that starts blocking us (like ORF did with ffmpeg's user agent) shows up here.
    #[test]
    #[ignore = "needs network; run with --ignored"]
    fn every_channel_is_reachable() {
        let channels = [
            "ARD", "ZDF", "ORF", "SRF", "3Sat", "ARTE.DE", "KiKA", "BR", "NDR", "WDR", "SWR", "MDR", "HR", "RBB",
            "PHOENIX", "DW", "Funk.net", "ZDF-tivi", "SR",
        ];
        let failures: Vec<String> = std::thread::scope(|s| {
            let handles: Vec<_> = channels
                .iter()
                .map(|ch| s.spawn(move || check_channel(ch)))
                .collect();
            handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
        });
        assert!(failures.is_empty(), "unreachable:\n{}", failures.join("\n"));
    }

    fn check_channel(channel: &str) -> Vec<String> {
        let query = serde_json::json!({
            "queries": [{"fields": ["channel"], "query": channel.to_lowercase()}],
            "sortBy": "timestamp", "sortOrder": "desc", "future": false, "offset": 0, "size": 10,
        });
        let entries = match run_query(&query) {
            Ok(e) => e,
            Err(e) => return vec![format!("{channel}: search failed: {e}")],
        };
        let entries: Vec<_> = entries.into_iter().filter(|e| e.channel.eq_ignore_ascii_case(channel)).take(2).collect();
        if entries.is_empty() {
            return vec![format!("{channel}: no entries")];
        }
        let mut failures = vec![];
        for e in &entries {
            let mut urls: Vec<&str> = [Quality::Hd, Quality::Normal, Quality::Low].iter().map(|q| e.url(*q)).collect();
            urls.dedup();
            for url in urls {
                if let Err(err) = check_url(url) {
                    failures.push(format!("{channel} «{}» {url}: {err}", e.title));
                }
            }
        }
        failures
    }

    fn check_url(url: &str) -> Result<(), String> {
        if url.contains(".m3u8") {
            let out = std::process::Command::new("ffprobe")
                .args(player::net_args(url))
                .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", url])
                .output()
                .map_err(|e| e.to_string())?;
            return if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) };
        }
        let mut resp = ureq::get(url)
            .header("User-Agent", player::USER_AGENT)
            .header("Range", "bytes=0-65535")
            .call()
            .map_err(|e| e.to_string())?;
        let mut buf = [0u8; 1024];
        let n = resp.body_mut().as_reader().read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 { Err("empty body".into()) } else { Ok(()) }
    }

    /// Renders the given tab off-screen to tests/snapshots/<name>.png.
    fn snapshot(name: &str, lang: Lang, tab: Tab) {
        snapshot_with_pointer(name, lang, tab, None);
    }

    fn snapshot_with_pointer(name: &str, lang: Lang, tab: Tab, pointer: Option<egui::Pos2>) {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1000.0, 720.0))
            .with_theme(egui::Theme::Light)
            .build_eframe(|cc| {
                theme::setup(&cc.egui_ctx);
                let mut app = App::new();
                app.dir = std::env::temp_dir().join(format!("mymediathek-snapshot-{name}"));
                let _ = std::fs::remove_dir_all(&app.dir);
                std::fs::create_dir_all(&app.dir).unwrap();
                app.settings = Settings { lang, quality: Quality::Hd };
                app.tab = tab;
                app.query = "Sendung mit der Maus".into();
                *app.results.lock().unwrap() = Some(Ok(vec![
                    entry("ARD", "Die Sendung mit der Maus", "Wie kommt die Milch in die Tüte?",
                        "Christoph besucht eine Molkerei. Dort sieht er, wie die Milch abgefüllt wird. Außerdem gibt es eine Lachgeschichte. Und natürlich den Elefanten.", 30),
                    entry("KiKA", "Löwenzahn", "Fritz Fuchs und die Bienen",
                        "Fritz entdeckt ein Bienenvolk in seinem Garten. Was machen Bienen eigentlich den ganzen Tag?", 25),
                    entry("ZDF", "logo!", "logo! am Abend", "Nachrichten für Kinder.", 10),
                    entry("BR", "Checker Tobi", "Der Vulkan-Check", "Tobi reist nach Island und klettert auf einen Vulkan. Wie heiß ist Lava?", 25),
                    entry("KiKA", "Wissen macht Ah!", "Warum ist der Himmel blau?", "Shary und Ralph erklären das Wetter.", 24),
                    entry("ARD", "Die Sendung mit der Maus", "Lach- und Sachgeschichten vom 12.10.2026", "Heute mit einer Reise zum Mond und einem Besuch beim Bäcker.", 30),
                    entry("ZDF-tivi", "Löwenzahn", "Fritz und der Regenwurm", "Ein Regenwurm im Garten bringt Fritz auf eine Idee.", 25),
                    entry("KiKA", "pur+", "Wie funktioniert ein Flugzeug?", "Eric darf ins Cockpit. Er fragt die Pilotin alles, was er wissen will.", 25),
                ]));
                let entries = app.results.lock().unwrap().clone().unwrap().unwrap();
                // One downloaded (purple play button), two bookmarked (gold stars).
                std::fs::write(app.dir.join(entries[2].file_name()), b"").unwrap();
                app.refresh_library();
                app.bookmarks = vec![entries[3].clone(), entries[0].clone()];
                let downloads = [
                    ("Fritz Fuchs und die Bienen", DlState::Running { done: 120 << 20, total: Some(300 << 20) }),
                    ("Wie kommt die Milch in die Tüte?", DlState::Running { done: 20 << 20, total: Some(400 << 20) }),
                    ("logo! am Abend", DlState::Finished),
                    ("Checker Tobi – Der Vulkan-Check", DlState::Running { done: 50 << 20, total: None }),
                    ("Die Sendung mit dem Elefanten", DlState::Failed("403 Forbidden".into())),
                    ("Löwenzahn – Fritz und der Regenwurm", DlState::Running { done: 290 << 20, total: Some(300 << 20) }),
                ];
                app.downloads.lock().unwrap().extend(downloads.into_iter().map(|(title, state)| Download {
                    title: title.into(),
                    target: app.dir.join("x.mp4"),
                    state,
                }));
                app
            });
        harness.run_steps(4);
        if let Some(pos) = pointer {
            harness.event(egui::Event::PointerMoved(pos));
            // Tooltips wait for the pointer to rest.
            for _ in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                harness.run_steps(1);
            }
        }
        harness.snapshot(name);
    }

    #[test]
    #[ignore = "needs a GPU; run with --ignored to render UI snapshots"]
    fn snapshot_search_hover() {
        snapshot_with_pointer("search-hover-de", Lang::De, Tab::Search, Some(egui::pos2(300.0, 184.0)));
    }

    #[test]
    #[ignore = "needs a GPU; run with --ignored to render UI snapshots"]
    fn snapshot_search() {
        snapshot("search-de", Lang::De, Tab::Search);
    }

    #[test]
    #[ignore = "needs a GPU; run with --ignored to render UI snapshots"]
    fn snapshot_library() {
        snapshot("library-en", Lang::En, Tab::Library);
    }

    #[test]
    #[ignore = "needs a GPU and ffmpeg; run with --ignored to render UI snapshots"]
    fn snapshot_player() {
        player_snapshot("player-de", false);
    }

    #[test]
    #[ignore = "needs a GPU and ffmpeg; run with --ignored to render UI snapshots"]
    fn snapshot_player_fullscreen() {
        player_snapshot("player-fullscreen-de", true);
    }

    fn player_snapshot(name: &str, fullscreen: bool) {
        let clip = std::env::temp_dir().join(format!("{name}.mp4"));
        assert!(std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=25:duration=5"])
            .args(["-f", "lavfi", "-i", "sine=duration=5", "-shortest"])
            .arg(&clip)
            .status()
            .unwrap()
            .success());
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1000.0, 720.0))
            .with_theme(egui::Theme::Light)
            .build_eframe(|cc| {
                theme::setup(&cc.egui_ctx);
                let mut app = App::new();
                app.settings.lang = Lang::De;
                app.open_player(&cc.egui_ctx, clip.to_string_lossy().into_owned(), "Testbild".into());
                app.fullscreen = fullscreen;
                app
            });
        // Decoding happens on other threads; give it real time to produce frames.
        for _ in 0..20 {
            harness.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        harness.snapshot(name);
    }

    #[test]
    #[ignore = "needs a GPU; run with --ignored to render UI snapshots"]
    fn snapshot_favourites() {
        snapshot("favourites-de", Lang::De, Tab::Favourites);
    }

    #[test]
    #[ignore = "needs a GPU; run with --ignored to render UI snapshots"]
    fn snapshot_settings() {
        snapshot("settings-en", Lang::En, Tab::Settings);
    }

    #[test]
    fn sentences() {
        assert_eq!(first_sentences("A b. C am 15.10.2026 d! E? F.", 3), ("A b. C am 15.10.2026 d! E?", true));
        assert_eq!(first_sentences("Only one.", 3), ("Only one.", false));
        assert_eq!(first_sentences("No terminator", 3), ("No terminator", false));
    }
}
