//! Native launcher + remote session windows. Japanese-first, English-ready (see `crate::i18n`).
//! The root window is a launcher (saved connections + profile editor + settings). Each
//! connection opens its own (deferred) viewport with the remote desktop filling the window and
//! an auto-hiding floating control bar — no permanent side panel while connected.
//! Connection windows draw the most recent framebuffer; frames are relayed by the root view,
//! which also owns clipboard sync (RFB ClientCutText/ServerCutText, text only, opt-out).
use eframe::egui;
use glintvnc_core::{
    configuration::{self as conf, KeyPreset, Profile, ScaleMode, SecurityPolicy},
    input, protocol, reconnect, secrets,
};
use std::{
    collections::HashMap,
    io,
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc as async_mpsc;

use crate::i18n::{self, Lang};

const SCROLL_NOTCH_PT: f32 = 28.0;
const MAX_SCROLL_PULSES_PER_FRAME: u32 = 6;
/// TigerVNC and RealVNC default to ~256 KiB caps for cut-text payloads.
const MAX_CUTTEXT_SEND: usize = 262_144;

pub fn launch(
    store: conf::Store,
    path: PathBuf,
    selected: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let opt = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([880.0, 580.0]),
        ..Default::default()
    };
    eframe::run_native(
        "GlintVNC",
        opt,
        Box::new(move |cc| {
            install_japanese_fonts(&cc.egui_ctx);
            let mut app = GlintApp::new(cc, store.clone(), path.clone());
            if let Some(sel) = selected.as_ref() {
                // `glintvnc connect <id>` opens the connection directly in its own window.
                let ctx = cc.egui_ctx.clone();
                app.open_connection(sel, &ctx);
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}

/// macOS ships Hiragino Sans; egui's default fonts have no CJK glyphs.
/// W3 for body, W6 for strong text; `y_offset_factor` nudges CJK glyphs down so they
/// look vertically centered next to Latin glyphs (Japanese fonts sit high in the line box).
fn install_japanese_fonts(ctx: &egui::Context) {
    let candidates: &[(&str, &str)] = &[
        (
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
            "Hiragino-W3",
        ),
        (
            "/System/Library/Fonts/ヒラギノ角ゴシック W6.ttc",
            "Hiragino-W6",
        ),
    ];
    let mut fonts = egui::FontDefinitions::default();
    let mut loaded = 0usize;
    for (path, name) in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let tweak = egui::FontTweak {
                y_offset_factor: 0.08,
                ..Default::default()
            };
            fonts.font_data.insert(
                (*name).to_string(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes).tweak(tweak)),
            );
            loaded += 1;
        }
    }
    // Phosphor icons (regular) as a proportional fallback: icons can be mixed into
    // ordinary labels like "GEAR Settings" without a second font call.
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // Menlo for host:port and log-ish values.
    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/Menlo.ttc") {
        fonts.font_data.insert(
            "Menlo".to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            list.insert(0, "Menlo".to_string());
        }
    }
    if loaded == 0 {
        // Still install phosphor even if Hiragino is missing.
        ctx.set_fonts(fonts);
        return;
    }
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(list) = fonts.families.get_mut(&family) {
            for (_, name) in candidates {
                if fonts.font_data.contains_key(*name) {
                    list.push((*name).to_string());
                }
            }
        }
    }
    ctx.set_fonts(fonts);
}

fn rgb(r: u8, g: u8, b: u8) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

/// Backgrounds near-black / pure white; ALL default text strictly ffffff / 000000.
fn build_visuals(dark: bool) -> egui::Visuals {
    // Design tokens (DESIGN_ROUND4_INPUT_UI.md):
    // a quiet instrument-panel dark, one signal blue, hairline dividers, 4 type sizes.
    let fg = if dark {
        rgb(0xe8, 0xea, 0xf0)
    } else {
        rgb(0x16, 0x18, 0x1d)
    };
    let dim = if dark {
        rgb(0x9a, 0xa3, 0xb2)
    } else {
        rgb(0x5b, 0x64, 0x70)
    };
    let (panel, window, extreme, faint, card, card_hover, divider) = if dark {
        (
            rgb(0x13, 0x15, 0x1a), // chrome panels
            rgb(0x0e, 0x10, 0x14), // window background
            rgb(0x05, 0x06, 0x08),
            rgb(0x1a, 0x1d, 0x24),
            rgb(0x1a, 0x1d, 0x24), // raised card / input
            rgb(0x21, 0x25, 0x2e), // hover
            rgb(0x26, 0x2b, 0x36), // hairline
        )
    } else {
        (
            rgb(0xf3, 0xf3, 0xf6),
            rgb(0xfa, 0xfa, 0xfc),
            rgb(0xff, 0xff, 0xff),
            rgb(0xff, 0xff, 0xff),
            rgb(0xff, 0xff, 0xff),
            rgb(0xec, 0xec, 0xf0),
            rgb(0xe0, 0xe0, 0xe6),
        )
    };
    let accent = if dark {
        rgb(0x4c, 0x8d, 0xff)
    } else {
        rgb(0x0b, 0x69, 0xda)
    };
    let danger = if dark {
        rgb(0xf0, 0x55, 0x4d)
    } else {
        rgb(0xcf, 0x22, 0x2e)
    };

    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = panel;
    v.window_fill = window;
    v.extreme_bg_color = extreme;
    v.faint_bg_color = faint;
    v.weak_text_color = Some(dim);
    v.window_stroke = egui::Stroke::new(1.0, divider);
    v.window_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 18,
        spread: 0,
        color: if dark {
            egui::Color32::from_black_alpha(110)
        } else {
            egui::Color32::from_black_alpha(28)
        },
    };

    let stroke = egui::Stroke::new(1.0, fg);
    v.widgets.noninteractive.fg_stroke = stroke;
    v.widgets.noninteractive.bg_fill = card;
    v.widgets.noninteractive.weak_bg_fill = card;
    v.widgets.inactive.fg_stroke = stroke;
    v.widgets.inactive.bg_fill = card;
    v.widgets.inactive.weak_bg_fill = card;
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, divider);
    v.widgets.hovered.fg_stroke = stroke;
    v.widgets.hovered.bg_fill = card_hover;
    v.widgets.hovered.weak_bg_fill = card_hover;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, accent);
    v.widgets.active.fg_stroke = stroke;
    v.widgets.active.bg_fill = card_hover;
    v.widgets.active.weak_bg_fill = card_hover;
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, accent);
    v.widgets.open.fg_stroke = stroke;
    v.widgets.open.bg_fill = card_hover;
    v.widgets.open.bg_stroke = egui::Stroke::new(1.0, divider);

    v.selection.bg_fill = accent.gamma_multiply(0.45);
    v.selection.stroke = egui::Stroke::new(1.0, accent);
    v.hyperlink_color = accent;
    v.warn_fg_color = if dark {
        rgb(0xe5, 0xc0, 0x7b)
    } else {
        rgb(0x9a, 0x67, 0x00)
    };
    v.error_fg_color = danger;
    v
}

fn apply_theme(ctx: &egui::Context, theme_setting: &str) {
    ctx.set_visuals_of(egui::Theme::Dark, build_visuals(true));
    ctx.set_visuals_of(egui::Theme::Light, build_visuals(false));
    let pref = match theme_setting {
        "light" => egui::ThemePreference::Light,
        "system" => egui::ThemePreference::System,
        _ => egui::ThemePreference::Dark,
    };
    ctx.set_theme(pref);
}

pub struct GlintApp {
    store: conf::Store,
    config: PathBuf,
    selected: Option<String>,
    draft: Profile,
    secret_draft: String,
    status: String,
    show_settings: bool,
    confirm_delete: bool,
    started: Instant,
    screenshot_requested: bool,
    session_counter: u64,
    sessions: HashMap<egui::ViewportId, SessionSlot>,
    clipboard: Option<arboard::Clipboard>,
    clip_poll_due: Instant,
    /// Results of off-UI-thread Keychain operations: (success status, error prefix, result).
    secret_tx: mpsc::Sender<(String, String, Result<(), String>)>,
    secret_rx: Receiver<(String, String, Result<(), String>)>,
}

struct SessionSlot {
    state: Arc<Mutex<SessionShared>>,
    /// Worker thread handle. Joined on a reaper thread when a session goes away so
    /// stalled connections can never leak a thread + framebuffer indefinitely.
    worker: Option<std::thread::JoinHandle<()>>,
}

/// Ask the worker to stop, then join it off the UI thread.
fn reap_worker(slot: &mut SessionSlot) {
    if let Ok(s) = slot.state.try_lock() {
        let _ = s.commands.send(protocol::Command::Stop);
    }
    if let Some(handle) = slot.worker.take() {
        std::thread::spawn(move || {
            let _ = handle.join();
        });
    }
}

#[derive(Clone)]
struct RemoteCursor {
    /// Straight RGBA bytes, `size` = (width, height), hotspot from the server.
    rgba: Arc<[u8]>,
    size: [u16; 2],
    hotspot: [u16; 2],
}

struct SessionShared {
    /// Unbounded on purpose: senders must never silently drop a pointer release,
    /// and the 16ms throttle in `motion` caps the legitimate message rate to ~60/s.
    commands: async_mpsc::UnboundedSender<protocol::Command>,
    events: Receiver<protocol::Event>,
    profile: Profile,
    lang: Lang,
    frame: Option<egui::TextureHandle>,
    /// Persistent CPU-side framebuffer mirror (patched by dirty rects). The GPU texture
    /// receives partial uploads (set_partial); full re-upload only on first frame/resize.
    image: Option<egui::ColorImage>,
    cursor: Option<RemoteCursor>,
    size: [usize; 2],
    scroll_speed: f32,
    wheel_acc_x: f32,
    wheel_acc_y: f32,
    last_pointer: Option<(u16, u16, u8)>,
    /// 16ms motion throttle (latest-wins pending slot). Button-mask changes and
    /// press/release edges bypass it; it exists to stop high-frequency mice from
    /// flooding the wire, not to drop anything important.
    motion: input::MotionThrottle,
    throttle_base: Instant,
    /// RFC 6143 §7.5.5 button-mask: 1=left, 2=middle, 4=right.
    button_mask: u8,
    /// Outer rect of the floating control bar while it is visible. Presses/motion/releases
    /// inside it are handled locally and never forwarded to the remote framebuffer.
    bar_rect: Option<egui::Rect>,
    /// QEMU LED State (-261): bit2 caps / bit1 num / bit0 scroll. None until the server sends it.
    lock_state: Option<u8>,
    last_modifiers: [bool; 4],
    held_keysyms: Vec<u32>,
    had_focus: bool,
    active_fullscreen: bool,
    clipboard_baseline: Option<String>,
    controls_visible_until: Instant,
    closed_message: Option<String>,
    attempt: u32,
    retry_due: Option<Instant>,
    retry_now: bool,
    disconnect_requested: bool,
    settings_requested: bool,
    close_requested: bool,
}

fn throttle_now(s: &SessionShared) -> u64 {
    u64::try_from(s.throttle_base.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Single funnel for all pointer commands: updates `last_pointer` and keeps the
/// motion throttle's view of the wire in sync.
fn send_pointer(shared: &mut SessionShared, x: u16, y: u16, mask: u8) {
    let _ = shared.commands.send(protocol::Command::Pointer {
        x,
        y,
        buttons: mask,
    });
    shared.last_pointer = Some((x, y, mask));
    let now = throttle_now(shared);
    shared.motion.note_sent((x, y, mask), now);
}

fn release_buttons(shared: &mut SessionShared) {
    // Flush any throttled motion first so button order is preserved.
    let now = throttle_now(shared);
    if let Some((x, y, buttons)) = shared.motion.flush_pending(now) {
        send_pointer(shared, x, y, buttons);
    }
    if let Some((x, y, buttons)) = shared.last_pointer {
        if buttons != 0 {
            send_pointer(shared, x, y, 0);
        }
    }
}

fn release_held(shared: &mut SessionShared) {
    for keysym in shared.held_keysyms.drain(..) {
        let _ = shared.commands.send(protocol::Command::Key {
            keysym,
            pressed: false,
        });
    }
    shared.last_modifiers = [false; 4];
    // Never leave a mouse button latched on the remote side either.
    release_buttons(shared);
    shared.button_mask = 0;
}

fn local_clipboard_text(cb: &mut Option<arboard::Clipboard>) -> Option<String> {
    if cb.is_none() {
        *cb = arboard::Clipboard::new().ok();
    }
    cb.as_mut().and_then(|c| c.get_text().ok())
}

impl GlintApp {
    fn lang(&self) -> Lang {
        Lang::from_setting(&self.store.settings.language)
    }
    fn tr(&self, key: &str) -> &'static str {
        i18n::t(self.lang(), key)
    }
    fn new(cc: &eframe::CreationContext<'_>, store: conf::Store, config: PathBuf) -> Self {
        apply_theme(&cc.egui_ctx, &store.settings.theme);
        let (secret_tx, secret_rx) = mpsc::channel();
        Self {
            store,
            config,
            selected: None,
            draft: Profile::default(),
            secret_draft: String::new(),
            status: String::new(),
            show_settings: std::env::var_os("GLINTVNC_OPEN_SETTINGS").is_some(),
            confirm_delete: false,
            started: Instant::now(),
            screenshot_requested: false,
            session_counter: 0,
            sessions: HashMap::new(),
            clipboard: arboard::Clipboard::new().ok(),
            clip_poll_due: Instant::now(),
            secret_tx,
            secret_rx,
        }
    }
    fn persist(&mut self) {
        match conf::save(&self.config, &self.store) {
            Ok(()) => self.status = self.tr("status.saved").into(),
            Err(e) => self.status = format!("{}: {e}", self.tr("status.save_failed")),
        }
    }
    fn set_scroll_speed(&mut self, value: f32) {
        self.store.settings.scroll_speed = value.clamp(0.25, 4.0);
    }
    fn edit(&mut self, id: &str) {
        if let Some(p) = self.store.profiles.iter().find(|p| p.id == id) {
            self.draft = p.clone();
            self.selected = Some(id.into());
            self.secret_draft.clear();
            self.confirm_delete = false;
        }
    }
    fn new_draft(&mut self) {
        self.draft = Profile::default();
        self.draft.fullscreen_on_connect = self.store.settings.default_fullscreen;
        self.selected = None;
        self.secret_draft.clear();
        self.confirm_delete = false;
    }
    // ---------- sessions ----------
    /// Open (or focus) a remote session in its own window.
    /// Applies the profile's saved customization: scale mode, fullscreen-on-connect,
    /// key preset and security policy.
    fn open_connection(&mut self, selector: &str, ctx: &egui::Context) {
        let Some(p) = conf::get_profile(&self.store, selector).cloned() else {
            self.status = self.tr("status.not_found").into();
            return;
        };
        if p.security == SecurityPolicy::LocalhostOnly
            && !["localhost", "127.0.0.1", "::1", "[::1]"].contains(&p.host.as_str())
        {
            self.status = self.tr("status.blocked").into();
            return;
        }
        self.session_counter += 1;
        let vid = egui::ViewportId::from_hash_of(("glintvnc-session", self.session_counter, &p.id));
        let keychain_error = self.tr("status.keychain_unavailable").to_string();
        self.spawn_session(vid, p, keychain_error, ctx);
    }
    /// (Re)create the shared session slot and worker thread for `vid`.
    fn spawn_session(
        &mut self,
        vid: egui::ViewportId,
        profile: Profile,
        keychain_error: String,
        ctx: &egui::Context,
    ) {
        let (event_tx, event_rx) = mpsc::sync_channel(2);
        let (cmd_tx, cmd_rx) = async_mpsc::unbounded_channel();
        let p = profile.clone();
        let ctx2 = ctx.clone();
        let ctx3 = ctx.clone();
        let keychain_id = p.id.clone();
        // The protocol thread pokes the root window on every event so remote
        // frames land in the texture within one frame instead of on a slow poll.
        // `request_repaint()` targets whatever viewport was last active; from a
        // worker thread that can be a session window, leaving the root (which owns
        // event draining) asleep for up to 250 ms. Always wake ROOT explicitly.
        let wake: protocol::Wake =
            Arc::new(move || ctx3.request_repaint_of(egui::ViewportId::ROOT));
        let worker = std::thread::spawn(move || {
            // Keychain access (including interactive approval) happens here, never
            // on the UI thread. A locked Keychain ends this worker with a clear error.
            let password = match secrets::get_password(&keychain_id) {
                Ok(v) => v,
                Err(e) => {
                    let _ = event_tx.send(protocol::Event::Closed {
                        message: format!("{keychain_error}: {e}"),
                        retryable: false,
                    });
                    ctx2.request_repaint_of(egui::ViewportId::ROOT);
                    return;
                }
            };
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            let result = match rt {
                Ok(rt) => rt.block_on(protocol::run_once(
                    p,
                    password,
                    cmd_rx,
                    event_tx.clone(),
                    Some(wake),
                )),
                Err(e) => Err(protocol::Error::Io(e)),
            };
            match result {
                Err(e) => {
                    let _ = event_tx.send(protocol::Event::Closed {
                        message: e.to_string(),
                        retryable: e.retryable(),
                    });
                }
                Ok(()) => {
                    let _ = event_tx.send(protocol::Event::Closed {
                        message: "Disconnected".into(),
                        retryable: false,
                    });
                }
            }
            ctx2.request_repaint();
        });
        // Baseline the local clipboard now so we only push *changes* (never on connect),
        // matching RealVNC's SendInitialClipboard=false behavior.
        let mut cb = self.clipboard.take();
        let baseline = local_clipboard_text(&mut cb);
        self.clipboard = cb;
        let clipboard_baseline = baseline.or(Some(String::new()));
        let shared = SessionShared {
            commands: cmd_tx,
            events: event_rx,
            profile: profile.clone(),
            lang: self.lang(),
            frame: None,
            image: None,
            cursor: None,
            size: [0, 0],
            scroll_speed: self.store.settings.scroll_speed.clamp(0.25, 4.0),
            wheel_acc_x: 0.0,
            wheel_acc_y: 0.0,
            last_pointer: None,
            motion: input::MotionThrottle::new(),
            throttle_base: Instant::now(),
            button_mask: 0,
            bar_rect: None,
            lock_state: None,
            last_modifiers: [false; 4],
            held_keysyms: Vec::new(),
            had_focus: true,
            active_fullscreen: false,
            clipboard_baseline,
            controls_visible_until: Instant::now(),
            closed_message: None,
            attempt: 0,
            retry_due: None,
            retry_now: false,
            disconnect_requested: false,
            settings_requested: false,
            close_requested: false,
        };
        self.sessions.insert(
            vid,
            SessionSlot {
                state: Arc::new(Mutex::new(shared)),
                worker: Some(worker),
            },
        );
        self.status = self.tr("status.connecting").into();
    }
    fn respawn_session(&mut self, vid: egui::ViewportId, ctx: &egui::Context) {
        let Some(mut slot) = self.sessions.remove(&vid) else {
            return;
        };
        reap_worker(&mut slot);
        let id = slot.state.lock().unwrap().profile.id.clone();
        drop(slot);
        let Some(profile) = conf::get_profile(&self.store, &id).cloned() else {
            return;
        };
        let keychain_error = self.tr("status.keychain_unavailable").to_string();
        self.spawn_session(vid, profile, keychain_error, ctx);
    }
    fn open_session_count_for(&self, id: &str) -> usize {
        self.sessions
            .values()
            .filter(|slot| {
                slot.state
                    .try_lock()
                    .map(|s| s.profile.id == id)
                    .unwrap_or(false)
            })
            .count()
    }
    fn poll_sessions(&mut self, ctx: &egui::Context) {
        // ---- Keychain results (never block the UI thread on a prompt) ----
        while let Ok((ok_msg, err_prefix, result)) = self.secret_rx.try_recv() {
            self.status = match result {
                Ok(()) => ok_msg,
                Err(e) => format!("{err_prefix}: {e}"),
            };
        }
        // ---- clipboard: local -> remote (change-only, never on connect) ----
        if self.store.settings.clipboard_sync && Instant::now() >= self.clip_poll_due {
            self.clip_poll_due = Instant::now() + Duration::from_millis(500);
            let local = local_clipboard_text(&mut self.clipboard);
            if let Some(text) = local {
                if !text.is_empty() && text.len() <= MAX_CUTTEXT_SEND {
                    for slot in self.sessions.values() {
                        let mut s = slot.state.lock().unwrap();
                        if s.clipboard_baseline.as_deref() != Some(text.as_str())
                            && s.closed_message.is_none()
                            && s.commands
                                .send(protocol::Command::CutText { text: text.clone() })
                                .is_ok()
                        {
                            s.clipboard_baseline = Some(text.clone());
                        }
                    }
                }
            }
        }
        // ---- drain events per session ----
        let vids: Vec<egui::ViewportId> = self.sessions.keys().copied().collect();
        let mut to_remove: Vec<egui::ViewportId> = Vec::new();
        let mut to_respawn: Vec<egui::ViewportId> = Vec::new();
        for vid in vids {
            let (remove, respawn) = {
                let slot = self.sessions.get(&vid).expect("session vanished");
                let mut s = slot.state.lock().unwrap();
                s.scroll_speed = self.store.settings.scroll_speed.clamp(0.25, 4.0);
                s.lang = self.lang();
                if s.settings_requested {
                    s.settings_requested = false;
                    self.show_settings = true;
                }
                if s.disconnect_requested || s.close_requested {
                    release_held(&mut s);
                    let _ = s.commands.send(protocol::Command::Stop);
                    (true, false)
                } else {
                    let mut incoming = Vec::new();
                    while let Ok(e) = s.events.try_recv() {
                        incoming.push(e);
                        if incoming.len() >= 16 {
                            break;
                        }
                    }
                    let mut frame_updated = false;
                    let mut lock_updated = false;
                    for event in incoming {
                        match event {
                            protocol::Event::Connected {
                                name,
                                width,
                                height,
                            } => {
                                self.status = format!(
                                    "{}: {name} — {width}×{height}",
                                    self.tr("status.connected")
                                );
                                s.attempt = 0;
                                if s.profile.fullscreen_on_connect && !s.active_fullscreen {
                                    ctx.send_viewport_cmd_to(
                                        vid,
                                        egui::ViewportCommand::Fullscreen(true),
                                    );
                                    s.active_fullscreen = true;
                                }
                                ctx.send_viewport_cmd_to(vid, egui::ViewportCommand::Focus);
                            }
                            protocol::Event::Frame {
                                width,
                                height,
                                rects,
                            } => {
                                let size = [usize::from(width), usize::from(height)];
                                if s.size != size || s.image.is_none() {
                                    // Framebuffer (re)configured or first frame: rebuild the persistent
                                    // buffer and re-create the texture once (resize path only).
                                    s.size = size;
                                    let zeros =
                                        vec![0u8; usize::from(width) * usize::from(height) * 4];
                                    s.image = Some(egui::ColorImage::from_rgba_unmultiplied(
                                        size, &zeros,
                                    ));
                                    // Drop the stale texture; the first partial upload re-creates it.
                                    let _ = s.frame.take();
                                }
                                let fb_size = s.image.as_ref().map(|i| (i.width(), i.height()));
                                let mut full_upload = false;
                                if let Some(image) = s.image.as_mut() {
                                    let fb_w = image.width();
                                    for r in &rects {
                                        // Defense in depth: DirtyRect crosses a module
                                        // boundary and is untrusted-derived. Never index
                                        // outside the persistent CPU image.
                                        let rw = usize::from(r.width);
                                        let rh = usize::from(r.height);
                                        let Some(bytes) =
                                            rw.checked_mul(rh).and_then(|n| n.checked_mul(4))
                                        else {
                                            continue;
                                        };
                                        if r.rgba.len() != bytes
                                            || usize::from(r.x).saturating_add(rw) > image.width()
                                            || usize::from(r.y).saturating_add(rh) > image.height()
                                        {
                                            continue;
                                        }
                                        let src_stride = usize::from(r.width) * 4;
                                        for row in 0..usize::from(r.height) {
                                            let src =
                                                &r.rgba[row * src_stride..(row + 1) * src_stride];
                                            let dst = ((usize::from(r.y) + row) * fb_w
                                                + usize::from(r.x))
                                                * 4;
                                            for (d, px) in image.pixels
                                                [(dst / 4)..((dst / 4) + usize::from(r.width))]
                                                .iter_mut()
                                                .zip(src.as_chunks::<4>().0.iter())
                                            {
                                                *d = egui::Color32::from_rgba_unmultiplied(
                                                    px[0], px[1], px[2], px[3],
                                                );
                                            }
                                        }
                                    }
                                }
                                // Partial texture uploads: only the dirty rectangles are sent to the
                                // GPU (TextureHandle::set_partial). Full re-upload happens only when
                                // the texture does not exist yet (first frame / resize).
                                match s.frame.as_mut() {
                                    Some(tex) => {
                                        for r in &rects {
                                            let pos = [usize::from(r.x), usize::from(r.y)];
                                            let sz = [usize::from(r.width), usize::from(r.height)];
                                            // Defensive bound check (TextureManager debug_asserts).
                                            if let Some((fw, fh)) = fb_size {
                                                if pos[0] + sz[0] <= fw && pos[1] + sz[1] <= fh {
                                                    tex.set_partial(
                                                        pos,
                                                        egui::ColorImage::from_rgba_unmultiplied(
                                                            sz, &r.rgba,
                                                        ),
                                                        egui::TextureOptions::LINEAR,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    None => full_upload = true,
                                }
                                if full_upload {
                                    if let Some(image) = s.image.take() {
                                        s.frame = Some(ctx.load_texture(
                                            format!("vnc-frame-{vid:?}"),
                                            image.clone(),
                                            egui::TextureOptions::LINEAR,
                                        ));
                                        s.image = Some(image);
                                    }
                                }
                                frame_updated = true;
                            }
                            protocol::Event::Cursor {
                                width,
                                height,
                                hot_x,
                                hot_y,
                                rgba,
                            } => {
                                if width == 0 || height == 0 {
                                    s.cursor = None; // server unset the cursor
                                } else {
                                    if std::env::var_os("GLINTVNC_DEBUG_CURSOR").is_some() {
                                        // Size/hotspot only, never pixels.
                                        eprintln!("glintvnc: remote cursor {width}x{height} hotspot=({hot_x},{hot_y})");
                                    }
                                    s.cursor = Some(RemoteCursor {
                                        rgba: Arc::from(rgba.into_boxed_slice()),
                                        size: [width, height],
                                        hotspot: [hot_x, hot_y],
                                    });
                                }
                                frame_updated = true;
                            }
                            protocol::Event::LedState(state) => {
                                if std::env::var_os("GLINTVNC_DEBUG_CURSOR").is_some() {
                                    // State byte only, never session contents.
                                    eprintln!("glintvnc: lock state 0x{state:02x}");
                                }
                                s.lock_state = Some(state);
                                lock_updated = true;
                            }
                            protocol::Event::ClipboardText(text) => {
                                // remote -> local; never log the payload.
                                if self.store.settings.clipboard_sync {
                                    if let Ok(mut cb) = arboard::Clipboard::new() {
                                        if cb.set_text(&text).is_ok() {
                                            s.clipboard_baseline = Some(text);
                                        }
                                    }
                                }
                            }
                            protocol::Event::Closed { message, retryable } => {
                                // Release everything latched on the remote, and drop the
                                // last frame. Otherwise the frozen image hides the
                                // reconnect/close overlay until the window is closed.
                                release_held(&mut s);
                                s.held_keysyms.clear();
                                s.last_modifiers = [false; 4];
                                s.frame = None;
                                s.image = None;
                                s.closed_message = Some(message);
                                self.status = self.tr("status.closed").into();
                                let allow = s.profile.auto_reconnect;
                                if retryable
                                    && allow
                                    && (self.store.settings.reconnect_max_attempts == 0
                                        || s.attempt < self.store.settings.reconnect_max_attempts)
                                {
                                    s.attempt += 1;
                                    s.retry_due = Some(
                                        Instant::now()
                                            + reconnect::backoff_decorrelated(
                                                s.attempt,
                                                reconnect::jitter_unit(),
                                            ),
                                    );
                                }
                            }
                        }
                    }
                    if frame_updated || lock_updated {
                        // Wake the session window so it repaints with the new texture/lock state now.
                        ctx.request_repaint_of(vid);
                    }
                    if s.retry_now {
                        s.retry_now = false;
                        s.retry_due = None;
                        s.attempt = 0;
                        (false, true)
                    } else if let Some(when) = s.retry_due {
                        if Instant::now() >= when {
                            s.retry_due = None;
                            (false, true)
                        } else {
                            ctx.request_repaint_after(
                                when.saturating_duration_since(Instant::now()),
                            );
                            (false, false)
                        }
                    } else {
                        (false, false)
                    }
                }
            };
            if remove {
                to_remove.push(vid);
            } else if respawn {
                to_respawn.push(vid);
            }
        }
        for vid in to_remove {
            if let Some(mut slot) = self.sessions.remove(&vid) {
                reap_worker(&mut slot); // egui destroys the viewport next root frame
            }
        }
        for vid in to_respawn {
            self.respawn_session(vid, ctx);
        }
    }
    // ---------- root window: launcher ----------
    fn chrome(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            // Brand mark: a calm rounded square with the G glyph (one bold element; everything else quiet).
            let (mark, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
            ui.painter().rect_filled(mark, 8.0, accent(ui.visuals()));
            ui.painter().text(
                mark.center(),
                egui::Align2::CENTER_CENTER,
                "G",
                egui::FontId::proportional(17.0),
                egui::Color32::WHITE,
            );
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.add_space(2.0);
                ui.label(egui::RichText::new("GlintVNC").strong().size(15.0));
                ui.label(
                    egui::RichText::new(concat!("v", env!("CARGO_PKG_VERSION"), " · VNC"))
                        .small()
                        .color(muted(ui.visuals())),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                // Settings: ghost icon button.
                if ui
                    .add(
                        egui::Button::new(format!(
                            "{}  {}",
                            egui_phosphor::regular::GEAR,
                            self.tr("chrome.settings")
                        ))
                        .frame(false)
                        .min_size(egui::vec2(0.0, 32.0)),
                    )
                    .clicked()
                {
                    self.show_settings = !self.show_settings;
                }
                // New profile: solid secondary with a plus icon.
                if ui
                    .add(
                        egui::Button::new(format!(
                            "{}  {}",
                            egui_phosphor::regular::PLUS,
                            self.tr("chrome.new")
                        ))
                        .fill(ui.visuals().faint_bg_color)
                        .min_size(egui::vec2(0.0, 32.0)),
                    )
                    .clicked()
                {
                    self.new_draft();
                }
            });
        });
        ui.add_space(2.0);
    }
    fn statusbar(&self, ui: &mut egui::Ui) {
        ui.add_space(1.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.small(egui::RichText::new(&self.status).size(12.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(10.0);
                let open = self.sessions.len();
                if open > 0 {
                    // Live sessions pill: green dot + count (info, not decoration).
                    ui.horizontal(|ui| {
                        let (dot, _) =
                            ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter()
                            .circle_filled(dot.center(), 4.0, ok_color(ui.visuals()));
                        ui.monospace(format!("{open} {}", self.tr("home.sessions_open")));
                    });
                } else {
                    ui.small(
                        egui::RichText::new(concat!("GlintVNC ", env!("CARGO_PKG_VERSION")))
                            .color(muted(ui.visuals())),
                    );
                }
            });
        });
        ui.add_space(1.0);
    }
    fn home_left(&mut self, ui: &mut egui::Ui) {
        let visuals = ui.visuals().clone();
        let count = self.store.profiles.len();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(self.tr("home.saved"))
                    .strong()
                    .size(15.0),
            );
            // count chip
            egui::Frame::new()
                .fill(visuals.faint_bg_color)
                .inner_margin(egui::Margin::symmetric(8, 2))
                .corner_radius(999.0)
                .show(ui, |ui| {
                    ui.small(format!("{count}"));
                });
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.store.profiles.is_empty() {
                    ui.add_space(56.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(egui_phosphor::regular::PLUG)
                                .size(44.0)
                                .color(muted(&visuals)),
                        );
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new(self.tr("home.empty_title"))
                                .strong()
                                .size(15.0),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(self.tr("home.empty_body"))
                                .small()
                                .color(muted(&visuals)),
                        );
                    });
                    return;
                }
                let entries: Vec<_> = self
                    .store
                    .profiles
                    .iter()
                    .map(|p| (p.id.clone(), p.name.clone(), p.host.clone(), p.port))
                    .collect();
                for (id, name, host, port) in entries {
                    let open_count = self.open_session_count_for(&id);
                    let selected = self.selected.as_deref() == Some(id.as_str());
                    card_frame(ui, selected, |ui| {
                        ui.horizontal(|ui| {
                            // host avatar: monitor glyph in a soft chip
                            let chip = egui::Frame::new()
                                .fill(ui.visuals().faint_bg_color)
                                .inner_margin(egui::Margin::same(6))
                                .corner_radius(8.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(egui_phosphor::regular::DESKTOP)
                                            .size(18.0)
                                            .color(muted(ui.visuals())),
                                    );
                                });
                            let _ = chip;
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(&name).strong().size(14.0));
                                    if open_count > 0 {
                                        ui.horizontal(|ui| {
                                            let (dot, _) = ui.allocate_exact_size(
                                                egui::vec2(7.0, 7.0),
                                                egui::Sense::hover(),
                                            );
                                            ui.painter().circle_filled(
                                                dot.center(),
                                                3.5,
                                                ok_color(ui.visuals()),
                                            );
                                            ui.small(
                                                egui::RichText::new(format!(
                                                    "{open_count} {}",
                                                    self.tr("home.sessions_open")
                                                ))
                                                .color(ok_color(ui.visuals())),
                                            );
                                        });
                                    }
                                });
                                ui.label(
                                    egui::RichText::new(format!("{host}:{port}"))
                                        .monospace()
                                        .size(11.5)
                                        .color(muted(ui.visuals())),
                                );
                            });
                        });
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(format!(
                                        "{}  {}",
                                        egui_phosphor::regular::PLAY,
                                        self.tr("home.connect")
                                    ))
                                    .fill(accent(ui.visuals()))
                                    .min_size(egui::vec2(104.0, 32.0)),
                                )
                                .clicked()
                            {
                                self.open_connection(&id, ui.ctx());
                            }
                            if ui
                                .add(
                                    egui::Button::new(format!(
                                        "{}  {}",
                                        egui_phosphor::regular::PENCIL,
                                        self.tr("home.edit")
                                    ))
                                    .frame(false)
                                    .min_size(egui::vec2(0.0, 32.0)),
                                )
                                .clicked()
                            {
                                self.edit(&id);
                            }
                        });
                    });
                    ui.add_space(8.0);
                }
            });
    }
    fn home_form(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail = ui.available_width();
                let form_w = 620.0_f32.min((avail - 16.0).max(280.0));
                let margin = ((avail - form_w) / 2.0).max(0.0);
                ui.horizontal(|ui| {
                    ui.add_space(margin);
                    ui.allocate_ui_with_layout(
                        egui::vec2(form_w, ui.available_height()),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            ui.add_space(8.0);
                            let title = if self.draft.id.is_empty() {
                                self.tr("form.new_title")
                            } else {
                                self.tr("form.edit_title")
                            };
                            ui.heading(egui::RichText::new(title).size(20.0));
                            ui.add_space(10.0);

                            // ---- Section 1: Connection
                            self.form_section(ui, "form.section_connect");
                            let label_w = 118.0;
                            let field_w = ui.available_width() - label_w - 16.0;
                            egui::Grid::new("profile-form")
                                .num_columns(2)
                                .spacing([16.0, 10.0])
                                .min_col_width(label_w)
                                .show(ui, |ui| {
                                    self.form_label(ui, "form.name");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.draft.name)
                                            .desired_width(field_w),
                                    );
                                    ui.end_row();

                                    self.form_label(ui, "form.host");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.draft.host)
                                            .desired_width(field_w)
                                            .hint_text("192.0.2.10"),
                                    );
                                    ui.end_row();

                                    self.form_label(ui, "form.port");
                                    ui.add(
                                        egui::DragValue::new(&mut self.draft.port).range(1..=65535),
                                    );
                                    ui.end_row();

                                    self.form_label(ui, "form.username");
                                    let u = self.draft.username.get_or_insert_with(String::new);
                                    ui.add(
                                        egui::TextEdit::singleline(u)
                                            .desired_width(field_w)
                                            .hint_text("user"),
                                    );
                                    ui.end_row();

                                    self.form_label(ui, "form.password");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.secret_draft)
                                            .password(true)
                                            .desired_width(field_w)
                                            .hint_text("••••••••"),
                                    );
                                    ui.end_row();
                                });
                            ui.add_space(4.0);

                            // ---- Section 2: Options
                            self.form_section(ui, "form.section_options");
                            let auto_label = self.tr("form.auto_reconnect");
                            let fullscreen_label = self.tr("form.fullscreen");
                            ui.horizontal(|ui| {
                                ui.add_space(label_w + 16.0);
                                ui.checkbox(&mut self.draft.auto_reconnect, auto_label);
                                ui.add_space(20.0);
                                ui.checkbox(
                                    &mut self.draft.fullscreen_on_connect,
                                    fullscreen_label,
                                );
                            });
                            ui.add_space(6.0);
                            egui::Grid::new("profile-view")
                                .num_columns(2)
                                .spacing([16.0, 10.0])
                                .min_col_width(label_w)
                                .show(ui, |ui| {
                                    self.form_label(ui, "form.scale");
                                    ui.horizontal(|ui| {
                                        for (mode, key) in [
                                            (ScaleMode::Fit, "viewer.scale_fit"),
                                            (ScaleMode::PixelPerfect, "viewer.scale_one"),
                                            (ScaleMode::Stretch, "viewer.scale_stretch"),
                                        ] {
                                            let active = self.draft.scale_mode == mode;
                                            if ui
                                                .add(
                                                    egui::Button::selectable(active, self.tr(key))
                                                        .min_size(egui::vec2(0.0, 30.0)),
                                                )
                                                .clicked()
                                            {
                                                self.draft.scale_mode = mode;
                                            }
                                        }
                                    });
                                    ui.end_row();
                                    self.form_label(ui, "form.keymap");
                                    ui.horizontal(|ui| {
                                        for (preset, key) in [
                                            (KeyPreset::MacToLinux, "form.keymap_linux"),
                                            (KeyPreset::MacToWindows, "form.keymap_windows"),
                                            (KeyPreset::Identity, "form.keymap_identity"),
                                        ] {
                                            let active = self.draft.key_preset == preset;
                                            if ui
                                                .add(
                                                    egui::Button::selectable(active, self.tr(key))
                                                        .min_size(egui::vec2(0.0, 30.0)),
                                                )
                                                .clicked()
                                            {
                                                self.draft.key_preset = preset;
                                            }
                                        }
                                    });
                                    ui.end_row();
                                });
                            ui.add_space(4.0);

                            // ---- Section 3: Security
                            self.form_section(ui, "form.section_security");
                            // segmented control
                            egui::Frame::new()
                                .fill(ui.visuals().faint_bg_color)
                                .inner_margin(egui::Margin::same(3))
                                .corner_radius(8.0)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        for (policy, label) in [
                                            (
                                                SecurityPolicy::LocalhostOnly,
                                                self.tr("form.sec_local"),
                                            ),
                                            (
                                                SecurityPolicy::AllowLegacyUnencrypted,
                                                self.tr("form.sec_legacy"),
                                            ),
                                        ] {
                                            let active = self.draft.security == policy;
                                            let btn = if active {
                                                egui::Button::new(
                                                    egui::RichText::new(label).strong(),
                                                )
                                                .fill(ui.visuals().window_fill)
                                                .stroke(egui::Stroke::new(
                                                    1.0,
                                                    divider(ui.visuals()),
                                                ))
                                            } else {
                                                egui::Button::new(label).frame(false)
                                            };
                                            if ui.add(btn.min_size(egui::vec2(0.0, 30.0))).clicked()
                                            {
                                                self.draft.security = policy;
                                            }
                                        }
                                    });
                                });
                            if self.draft.security == SecurityPolicy::AllowLegacyUnencrypted {
                                ui.add_space(6.0);
                                egui::Frame::new()
                                    .fill(ui.visuals().warn_fg_color.gamma_multiply(0.10))
                                    .stroke(egui::Stroke::new(1.0, ui.visuals().warn_fg_color))
                                    .inner_margin(egui::Margin::same(10))
                                    .corner_radius(8.0)
                                    .show(ui, |ui| {
                                        ui.colored_label(
                                            ui.visuals().warn_fg_color,
                                            self.tr("form.legacy_warn"),
                                        );
                                    });
                            }
                            ui.add_space(16.0);

                            // ---- Actions
                            ui.horizontal(|ui| {
                                if ui
                                    .add(
                                        egui::Button::new(format!(
                                            "{}  {}",
                                            egui_phosphor::regular::CHECK,
                                            self.tr("form.save")
                                        ))
                                        .fill(accent(ui.visuals()))
                                        .min_size(egui::vec2(150.0, 34.0)),
                                    )
                                    .clicked()
                                {
                                    self.save_draft();
                                }
                                if !self.draft.id.is_empty() {
                                    let delete_label = if self.confirm_delete {
                                        self.tr("form.delete_confirm")
                                    } else {
                                        self.tr("form.delete")
                                    };
                                    let mut btn = egui::Button::new(format!(
                                        "{}  {}",
                                        egui_phosphor::regular::TRASH,
                                        delete_label
                                    ))
                                    .frame(false)
                                    .min_size(egui::vec2(150.0, 34.0));
                                    if self.confirm_delete {
                                        btn = btn
                                            .fill(ui.visuals().error_fg_color.gamma_multiply(0.85));
                                    }
                                    let clicked = ui.add(btn).clicked();
                                    if clicked {
                                        if self.confirm_delete {
                                            self.remove_draft();
                                        } else {
                                            self.confirm_delete = true;
                                        }
                                    }
                                }
                            });
                        },
                    );
                });
            });
    }
    fn form_section(&self, ui: &mut egui::Ui, title_key: &str) {
        ui.label(
            egui::RichText::new(self.tr(title_key))
                .small()
                .strong()
                .color(muted(ui.visuals())),
        );
        let y = ui.cursor().min.y + 2.0;
        let rect = ui.available_rect_before_wrap();
        ui.painter().line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0, divider(ui.visuals())),
        );
        ui.add_space(6.0);
    }
    fn form_label(&self, ui: &mut egui::Ui, key: &str) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(self.tr(key)).color(muted(ui.visuals())));
        });
    }
    fn save_draft(&mut self) {
        let mut p = self.draft.clone();
        if p.id.is_empty() {
            p.id = conf::make_id();
        }
        if let Err(e) = p.validate() {
            self.status = self.tr(conf::validation_i18n_key(&e)).into();
            return;
        }
        if !self.secret_draft.is_empty() {
            let tx = self.secret_tx.clone();
            let id = p.id.clone();
            let pw = self.secret_draft.clone();
            let ok = self.tr("status.saved").to_string();
            let err = self.tr("status.keychain_unavailable").to_string();
            std::thread::spawn(move || {
                let result = secrets::set_password(&id, &pw).map_err(|e| e.to_string());
                let _ = tx.send((ok, err, result));
            });
        }
        if let Some(old) = self.store.profiles.iter_mut().find(|x| x.id == p.id) {
            *old = p.clone();
        } else {
            self.store.profiles.push(p.clone());
        }
        self.draft = p.clone();
        self.selected = Some(p.id.clone());
        self.secret_draft.clear();
        self.confirm_delete = false;
        self.persist();
    }
    fn remove_draft(&mut self) {
        let id = self.draft.id.clone();
        self.store.profiles.retain(|p| p.id != id);
        self.persist();
        let tx = self.secret_tx.clone();
        let ok = self.tr("status.delete_done").to_string();
        let err = self.tr("status.delete_keychain_failed").to_string();
        std::thread::spawn(move || {
            let result = secrets::delete_password(&id).map_err(|e| e.to_string());
            let _ = tx.send((ok, err, result));
        });
        self.draft = Profile::default();
        self.selected = None;
        self.secret_draft.clear();
        self.confirm_delete = false;
    }
}

// ---------- session windows (child viewports) ----------
fn panel_bg(visuals: &egui::Visuals) -> egui::Color32 {
    visuals.panel_fill
}
fn session_window_ui(ui: &mut egui::Ui, state: Arc<Mutex<SessionShared>>) {
    let ctx = ui.ctx().clone();
    let vid = ctx.viewport_id();
    let lang = state.lock().unwrap().lang;
    if ctx.input(|i| i.viewport().close_requested()) {
        state.lock().unwrap().close_requested = true;
    }
    // Focus loss: release every key we hold on the remote side.
    {
        let mut s = state.lock().unwrap();
        let focused = ctx.input(|i| i.focused);
        if s.had_focus && !focused {
            release_held(&mut s);
        }
        s.had_focus = focused;
    }
    let mut canvas_rect: Option<egui::Rect> = None;
    let visuals = ui.visuals().clone();
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(panel_bg(&visuals)))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let s = state.lock().unwrap();
            if let Some(tex) = s.frame.as_ref() {
                let tex_id = tex.id();
                let mode = s.profile.scale_mode.clone();
                let fb = egui::vec2(s.size[0].max(1) as f32, s.size[1].max(1) as f32);
                match mode {
                    ScaleMode::Stretch => {
                        let r = ui.image((tex_id, rect.size()));
                        canvas_rect = Some(r.rect);
                        drop(s);
                        remote_input(&ctx, r.rect, &state);
                    }
                    ScaleMode::PixelPerfect => {
                        drop(s);
                        egui::ScrollArea::both()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let r = ui.image((tex_id, fb));
                                canvas_rect = Some(r.rect);
                                remote_input(&ctx, r.rect, &state);
                            });
                    }
                    ScaleMode::Fit => {
                        // Fit preserving aspect ratio; center inside the panel. Compute the
                        // image rect explicitly instead of relying on nested layout alignment
                        // (the rect passed to input mapping must equal the drawn rect).
                        let factor = (rect.width() / fb.x).min(rect.height() / fb.y).max(0.01);
                        let target = fb * factor;
                        let pos = egui::pos2(
                            rect.center().x - target.x / 2.0,
                            rect.center().y - target.y / 2.0,
                        );
                        let image_rect = egui::Rect::from_min_size(pos, target);
                        drop(s);
                        let r = ui.put(image_rect, egui::Image::new((tex_id, target)));
                        canvas_rect = Some(r.rect);
                        remote_input(&ctx, r.rect, &state);
                    }
                }
            } else if let Some(message) = s.closed_message.clone() {
                drop(s);
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.5 - 60.0).max(24.0));
                    ui.heading(i18n::t(lang, "status.closed"));
                    ui.add_space(4.0);
                    ui.label(&message);
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button(i18n::t(lang, "viewer.reconnect")).clicked() {
                            state.lock().unwrap().retry_now = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(i18n::t(lang, "session.close"))
                                    .min_size(egui::vec2(96.0, 32.0)),
                            )
                            .clicked()
                        {
                            state.lock().unwrap().disconnect_requested = true;
                        }
                    });
                });
            } else {
                drop(s);
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.5 - 60.0).max(24.0));
                    ui.spinner();
                    ui.add_space(8.0);
                    ui.label(i18n::t(lang, "viewer.connecting"));
                    ui.add_space(10.0);
                    if ui
                        .add(
                            egui::Button::new(i18n::t(lang, "session.cancel"))
                                .min_size(egui::vec2(96.0, 32.0)),
                        )
                        .clicked()
                    {
                        state.lock().unwrap().disconnect_requested = true;
                    }
                });
            }
        });
    // OS cursor: when the server negotiated the Cursor pseudo-encoding, swap the
    // OS cursor for the server-provided shape while the pointer is over the canvas.
    // (Server stops drawing its own cursor into the framebuffer in that case, so this
    // is the only cursor on screen — it tracks with zero added latency.)
    {
        let hover = ctx.input(|i| i.pointer.hover_pos());
        let in_canvas = hover
            .zip(canvas_rect)
            .map(|(p, r)| r.contains(p))
            .unwrap_or(false);
        if in_canvas {
            let cursor = state.lock().unwrap().cursor.clone();
            ctx.set_cursor_image(cursor.map(|c| egui::CustomCursorImage {
                rgba: c.rgba,
                size: c.size,
                hotspot: c.hotspot,
            }));
        } else {
            ctx.set_cursor_image(None);
        }
    }
    // floating control bar: appears when the pointer is near the top edge.
    // It must never cover the remote framebuffer while a drag is in flight, and any
    // press/release/motion inside the bar's rect is treated as local-only input.
    let bar_allowed = state.lock().unwrap().button_mask == 0;
    const BAR_HOVER_PT: f32 = 42.0;
    let hover_top = ui
        .ctx()
        .input(|i| i.pointer.hover_pos())
        .map(|p| p.y <= BAR_HOVER_PT)
        .unwrap_or(false);
    {
        let mut s = state.lock().unwrap();
        if bar_allowed && hover_top {
            s.controls_visible_until = Instant::now() + Duration::from_secs(2);
        }
    }
    let show_controls =
        bar_allowed && Instant::now() < state.lock().unwrap().controls_visible_until;
    if !show_controls {
        state.lock().unwrap().bar_rect = None;
    }
    if show_controls {
        egui::Area::new(egui::Id::unique(("session-controls", vid)))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 10.0))
            .order(egui::Order::Foreground)
            .show(&ctx, |ui| {
                let dark = ui.visuals().dark_mode;
                let pill = if dark {
                    egui::Color32::from_rgba_unmultiplied(0x14, 0x16, 0x1c, 238)
                } else {
                    egui::Color32::from_rgba_unmultiplied(0xf7, 0xf7, 0xfa, 238)
                };
                let hairline = if dark {
                    rgb(0x2b, 0x30, 0x3c)
                } else {
                    rgb(0xdd, 0xde, 0xe4)
                };
                let cap_on = state.lock().unwrap().lock_state.map(|s| s & 0x04 != 0);
                let frame_resp = egui::Frame::new()
                    .fill(pill)
                    .stroke(egui::Stroke::new(1.0, hairline))
                    .inner_margin(egui::Margin::symmetric(6, 4))
                    .corner_radius(10.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // 接続一覧 (ghost)
                            if ui
                                .add(
                                    egui::Button::new(format!(
                                        "{}  {}",
                                        egui_phosphor::regular::ARROW_LEFT,
                                        i18n::t(lang, "session.back")
                                    ))
                                    .frame(false)
                                    .min_size(egui::vec2(0.0, 32.0)),
                                )
                                .clicked()
                            {
                                ctx.send_viewport_cmd_to(
                                    egui::ViewportId::ROOT,
                                    egui::ViewportCommand::Visible(true),
                                );
                                ctx.send_viewport_cmd_to(
                                    egui::ViewportId::ROOT,
                                    egui::ViewportCommand::Focus,
                                );
                            }
                            // 全画面トグル (ghost, active = filled)
                            let fullscreen_on = state.lock().unwrap().active_fullscreen;
                            let fs_icon = if fullscreen_on {
                                egui_phosphor::regular::ARROWS_IN_SIMPLE
                            } else {
                                egui_phosphor::regular::ARROWS_OUT_SIMPLE
                            };
                            let fs_label = if fullscreen_on {
                                i18n::t(lang, "chrome.fullscreen_exit")
                            } else {
                                i18n::t(lang, "chrome.fullscreen")
                            };
                            if ui
                                .add(
                                    egui::Button::selectable(
                                        fullscreen_on,
                                        format!("{fs_icon}  {fs_label}"),
                                    )
                                    .frame(fullscreen_on)
                                    .min_size(egui::vec2(0.0, 32.0)),
                                )
                                .clicked()
                            {
                                let now_fullscreen = !fullscreen_on;
                                state.lock().unwrap().active_fullscreen = now_fullscreen;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(
                                    now_fullscreen,
                                ));
                            }
                            // CAPS: momentary caps-lock toggle sent to the remote X server.
                            // macOS cannot detect caps lock (winit drops it), so this is the
                            // way to flip remote caps state. Highlighted when the server
                            // reports it engaged (LED State pseudo-encoding, -261).
                            let caps_label = format!(
                                "{}  {}",
                                egui_phosphor::regular::KEYBOARD,
                                i18n::t(lang, "session.caps")
                            );
                            let caps_btn = match cap_on {
                                Some(true) => {
                                    egui::Button::new(egui::RichText::new(&caps_label).strong())
                                        .fill(accent(ui.visuals()))
                                }
                                _ => egui::Button::new(&caps_label).frame(false),
                            };
                            if ui
                                .add(caps_btn.min_size(egui::vec2(0.0, 32.0)))
                                .on_hover_text(i18n::t(lang, "session.caps"))
                                .clicked()
                            {
                                let tx = state.lock().unwrap().commands.clone();
                                let _ = tx.send(protocol::Command::Key {
                                    keysym: 0xffe5,
                                    pressed: true,
                                });
                                let _ = tx.send(protocol::Command::Key {
                                    keysym: 0xffe5,
                                    pressed: false,
                                });
                            }
                            // 再描画 (ghost): force a full framebuffer repaint; clears any
                            // ghost pixels left by the server's damage tracking.
                            if ui
                                .add(
                                    egui::Button::new(format!(
                                        "{}  {}",
                                        egui_phosphor::regular::ARROWS_CLOCKWISE,
                                        i18n::t(lang, "session.refresh")
                                    ))
                                    .frame(false)
                                    .min_size(egui::vec2(0.0, 32.0)),
                                )
                                .on_hover_text(i18n::t(lang, "session.refresh"))
                                .clicked()
                            {
                                let _ = state
                                    .lock()
                                    .unwrap()
                                    .commands
                                    .send(protocol::Command::Refresh);
                            }
                            // 設定 (ghost)
                            if ui
                                .add(
                                    egui::Button::new(format!(
                                        "{}  {}",
                                        egui_phosphor::regular::GEAR,
                                        i18n::t(lang, "session.settings")
                                    ))
                                    .frame(false)
                                    .min_size(egui::vec2(0.0, 32.0)),
                                )
                                .clicked()
                            {
                                state.lock().unwrap().settings_requested = true;
                                ctx.send_viewport_cmd_to(
                                    egui::ViewportId::ROOT,
                                    egui::ViewportCommand::Visible(true),
                                );
                                ctx.send_viewport_cmd_to(
                                    egui::ViewportId::ROOT,
                                    egui::ViewportCommand::Focus,
                                );
                            }
                            // 切断 (danger tint)
                            let (danger_fill, danger_text) = if dark {
                                (rgb(0x4a, 0x1e, 0x1c), rgb(0xff, 0x8a, 0x84))
                            } else {
                                (rgb(0xfd, 0xe7, 0xe7), rgb(0xcf, 0x22, 0x2e))
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(format!(
                                            "{}  {}",
                                            egui_phosphor::regular::POWER,
                                            i18n::t(lang, "session.disconnect")
                                        ))
                                        .color(danger_text),
                                    )
                                    .fill(danger_fill)
                                    .min_size(egui::vec2(0.0, 32.0)),
                                )
                                .clicked()
                            {
                                state.lock().unwrap().disconnect_requested = true;
                            }
                        });
                    });
                // The bar's outer rect (including padding) is the local-input zone; presses
                // inside it never reach the remote framebuffer while the bar is visible.
                state.lock().unwrap().bar_rect = Some(frame_resp.response.rect);
            });
    }
    // Repaint only while the floating bar may still change visibility (timed auto-hide).
    // Remote frames arrive via the protocol wake path and request their own repaint.
    let s = state.lock().unwrap();
    let until = s.controls_visible_until;
    let mask = s.button_mask;
    drop(s);
    if until > Instant::now() && mask == 0 {
        ctx.request_repaint_after(until.saturating_duration_since(Instant::now()));
    }
}
fn remote_input(ctx: &egui::Context, rect: egui::Rect, state: &Arc<Mutex<SessionShared>>) {
    if std::env::var_os("GLINTVNC_DEBUG_INPUT").is_some() {
        let s = state.lock().unwrap();
        eprintln!(
            "glintvnc: canvas rect=({:.1},{:.1}) {:.1}x{:.1} fb={}x{} fullscreen={}",
            rect.min.x,
            rect.min.y,
            rect.width(),
            rect.height(),
            s.size[0],
            s.size[1],
            s.active_fullscreen
        );
    }
    // 16ms motion flush; runs on every frame the window paints, and we wake
    // ourselves every 16ms while a throttled move is waiting.
    {
        let mut s = state.lock().unwrap();
        let now = throttle_now(&s);
        if let Some((x, y, mask)) = s.motion.flush_if_due(now) {
            send_pointer(&mut s, x, y, mask);
        }
    }
    let fb_size = state.lock().unwrap().size;
    if fb_size[0] == 0 || fb_size[1] == 0 {
        return;
    }
    let (key_preset, scroll_speed) = {
        let s = state.lock().unwrap();
        (s.profile.key_preset.clone(), s.scroll_speed)
    };
    let map_point = move |pos: egui::Pos2| -> Option<(u16, u16)> {
        if !rect.contains(pos) || rect.width() <= 0.0 || rect.height() <= 0.0 {
            return None;
        }
        let x = ((pos.x - rect.min.x) / rect.width() * fb_size[0] as f32)
            .round()
            .clamp(0.0, (fb_size[0] - 1) as f32) as u16;
        let y = ((pos.y - rect.min.y) / rect.height() * fb_size[1] as f32)
            .round()
            .clamp(0.0, (fb_size[1] - 1) as f32) as u16;
        Some((x, y))
    };
    // Clamp a pointer position into the canvas, then map it (used while a button is
    // held so drags keep streaming clamped fb coordinates at the edges instead of
    // stopping or leaking a release mid-gesture).
    let clamp_map_point = move |pos: egui::Pos2| -> Option<(u16, u16)> {
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return None;
        }
        let clamped = egui::pos2(
            pos.x.clamp(rect.min.x, rect.max.x - 0.001),
            pos.y.clamp(rect.min.y, rect.max.y - 0.001),
        );
        map_point(clamped)
    };
    // RFB button-mask bits (RFC 6143 §7.5.5): 1=left, 2=middle, 4=right.
    // macOS convention (Finder/Cocoa): Ctrl+click acts as a right-click; for MacTo*
    // presets we translate Primary+ctrl into the right-button bit, otherwise we keep
    // the literal mapping (Identity) — documented in MAINTENANCE.md.
    let mac_click_preset = matches!(key_preset, KeyPreset::MacToLinux | KeyPreset::MacToWindows);
    let button_bit = move |button: egui::PointerButton, ctrl: bool| -> Option<u8> {
        match button {
            egui::PointerButton::Primary if mac_click_preset && ctrl => Some(4),
            egui::PointerButton::Primary => Some(1),
            egui::PointerButton::Middle => Some(2),
            egui::PointerButton::Secondary => Some(4),
            egui::PointerButton::Extra1 | egui::PointerButton::Extra2 => None,
        }
    };
    let tx = state.lock().unwrap().commands.clone();
    let bar_rect = state.lock().unwrap().bar_rect;
    let snapshot = ctx.input(|i| (i.modifiers, i.events.clone()));
    // Keyboard-only path to the floating bar: F8 toggles it, because hover is
    // unavailable to keyboard users and fullscreen has no other local escape.
    if snapshot.1.iter().any(|e| {
        matches!(
            e,
            egui::Event::Key {
                key: egui::Key::F8,
                pressed: true,
                ..
            }
        )
    }) {
        let mut s = state.lock().unwrap();
        s.controls_visible_until = if Instant::now() < s.controls_visible_until {
            Instant::now() - Duration::from_secs(1)
        } else {
            Instant::now() + Duration::from_secs(60)
        };
    }
    // egui may emit both Key{Enter|Tab} and Text("\r"|"\t") for one press.
    let key_has_return = snapshot.1.iter().any(|e| {
        matches!(
            e,
            egui::Event::Key {
                key: egui::Key::Enter,
                ..
            }
        )
    });
    let key_has_tab = snapshot.1.iter().any(|e| {
        matches!(
            e,
            egui::Event::Key {
                key: egui::Key::Tab,
                ..
            }
        )
    });
    // Button edges must not overtake a throttled move that is still waiting:
    // flush it first (noVNC _flushMouseMoveTimer, TigerVNC handlePointerTimeout).
    {
        let mut s = state.lock().unwrap();
        let now = throttle_now(&s);
        if let Some((x, y, mask)) = s.motion.flush_pending(now) {
            send_pointer(&mut s, x, y, mask);
        }
    }
    // ---- PointerButton press/release events: discrete, carry their own position
    // and modifier state. This is the authoritative source for the button mask.
    for event in &snapshot.1 {
        let egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers,
        } = event
        else {
            continue;
        };
        let Some(bit) = button_bit(*button, modifiers.ctrl) else {
            continue;
        };
        {
            let s = state.lock().unwrap();
            if bar_rect.is_some_and(|b| b.contains(*pos)) && s.button_mask == 0 {
                // Press/release lands on the floating bar: local input only, never on the wire.
                continue;
            }
        }
        let debug_in = std::env::var_os("GLINTVNC_DEBUG_INPUT").is_some();
        let mut s = state.lock().unwrap();
        if *pressed {
            if let Some((x, y)) = map_point(*pos) {
                s.button_mask |= bit;
                let mask = s.button_mask;
                if debug_in {
                    eprintln!(
                        "glintvnc: press  bit={bit} fb=({x},{y}) mask={mask} pos=({:.1},{:.1})",
                        pos.x, pos.y
                    );
                }
                send_pointer(&mut s, x, y, mask);
            } else if debug_in {
                eprintln!(
                    "glintvnc: press  bit={bit} SWALLOWED (outside canvas / bar) pos=({:.1},{:.1})",
                    pos.x, pos.y
                );
            }
            // Press outside the canvas (floating bar, letterbox) never reaches the remote.
        } else if s.button_mask & bit != 0 {
            s.button_mask &= !bit;
            let mask = s.button_mask;
            let mapped = clamp_map_point(*pos).or(s.last_pointer.map(|(x, y, _)| (x, y)));
            if let Some((x, y)) = mapped {
                if debug_in {
                    eprintln!(
                        "glintvnc: release bit={bit} fb=({x},{y}) mask={mask} pos=({:.1},{:.1})",
                        pos.x, pos.y
                    );
                }
                send_pointer(&mut s, x, y, mask);
            }
        }
    }
    // ---- Pointer motion: inside the canvas => exact fb coordinates; outside
    // while a button is held => clamped to the fb edge (drag continues, like
    // TigerVNC); otherwise not forwarded.
    if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
        let mask = state.lock().unwrap().button_mask;
        let bar_blocked = bar_rect.is_some_and(|b| b.contains(pos)) && mask == 0;
        if !bar_blocked {
            let mapped = map_point(pos).or_else(|| {
                if mask != 0 {
                    clamp_map_point(pos)
                } else {
                    None
                }
            });
            if let Some((x, y)) = mapped {
                let mut s = state.lock().unwrap();
                let now = throttle_now(&s);
                if let Some((px, py, pmask)) = s.motion.on_motion((x, y, mask), now) {
                    send_pointer(&mut s, px, py, pmask);
                }
            }
        }
    }
    // While a throttled move waits, wake ourselves every 16ms so it is flushed
    // even if the user holds the pointer perfectly still.
    if state.lock().unwrap().motion.has_pending() {
        ctx.request_repaint_after(Duration::from_millis(16));
    }
    let mods = [
        snapshot.0.shift,
        snapshot.0.ctrl,
        snapshot.0.alt,
        snapshot.0.command,
    ];
    let meta = [
        input::Key::Shift,
        input::Key::Control,
        input::Key::Alt,
        input::Key::Command,
    ];
    {
        let mut s = state.lock().unwrap();
        for i in 0..4 {
            if mods[i] != s.last_modifiers[i] {
                if let Some(keysym) = input::keysym(meta[i], &key_preset) {
                    let _ = tx.send(protocol::Command::Key {
                        keysym,
                        pressed: mods[i],
                    });
                    if mods[i] {
                        if !s.held_keysyms.contains(&keysym) {
                            s.held_keysyms.push(keysym);
                        }
                    } else {
                        s.held_keysyms.retain(|&k| k != keysym);
                    }
                }
                s.last_modifiers[i] = mods[i];
            }
        }
    }
    for event in snapshot.1 {
        match event {
            egui::Event::Key {
                key,
                pressed,
                repeat,
                ..
            } => {
                // F8 is the local control-bar toggle (handled above), never remote.
                if key == egui::Key::F8 {
                    continue;
                }
                // Escape exits fullscreen when the local bar is hidden; with the bar
                // visible it stays a regular remote key so dialogs can be dismissed.
                if key == egui::Key::Escape
                    && pressed
                    && !snapshot.0.command
                    && !snapshot.0.ctrl
                    && !snapshot.0.alt
                    && !snapshot.0.shift
                {
                    let mut s = state.lock().unwrap();
                    if s.active_fullscreen && Instant::now() >= s.controls_visible_until {
                        s.active_fullscreen = false;
                        drop(s);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                        continue;
                    }
                }
                // Cmd+R: local full repaint (never sent to the remote). Useful as a
                // guaranteed way to clear stale pixels from imperfect damage handling.
                if key == egui::Key::R
                    && pressed
                    && snapshot.0.command
                    && !snapshot.0.ctrl
                    && !snapshot.0.alt
                    && !snapshot.0.shift
                {
                    let _ = state
                        .lock()
                        .unwrap()
                        .commands
                        .send(protocol::Command::Refresh);
                    continue;
                }
                // RFB clients handle key autorepeat locally: they send repeated "down"
                // messages (never synthetic "up" between repeats) and the release once.
                // macOS autorepeat arrives here with repeat=true; forwarding it as a
                // normal down event is exactly what rfbproto §"KeyEvent" prescribes.
                // Printable latin characters are typed via the Text path instead (which
                // carries the produced case, e.g. Caps Lock); we only map them here when
                // a command/control/alt modifier is held (shortcut territory).
                let modified = snapshot.0.command || snapshot.0.ctrl || snapshot.0.alt;
                let base_char = |c: char| -> Option<input::Key> {
                    if modified {
                        Some(input::Key::Character(c))
                    } else {
                        None
                    }
                };
                let k = match key {
                    egui::Key::Enter => Some(input::Key::Return),
                    egui::Key::Escape => Some(input::Key::Escape),
                    egui::Key::Backspace => Some(input::Key::Backspace),
                    egui::Key::Tab => Some(input::Key::Tab),
                    egui::Key::Delete => Some(input::Key::Delete),
                    egui::Key::Insert => Some(input::Key::Insert),
                    egui::Key::Space => Some(input::Key::Space),
                    egui::Key::ArrowUp => Some(input::Key::Up),
                    egui::Key::ArrowDown => Some(input::Key::Down),
                    egui::Key::ArrowLeft => Some(input::Key::Left),
                    egui::Key::ArrowRight => Some(input::Key::Right),
                    egui::Key::Home => Some(input::Key::Home),
                    egui::Key::End => Some(input::Key::End),
                    egui::Key::PageUp => Some(input::Key::PageUp),
                    egui::Key::PageDown => Some(input::Key::PageDown),
                    egui::Key::PrintScreen => Some(input::Key::PrintScreen),
                    egui::Key::Pause => Some(input::Key::Pause),
                    egui::Key::Menu => Some(input::Key::Menu),
                    egui::Key::CapsLock => Some(input::Key::CapsLock),
                    egui::Key::NumLock => Some(input::Key::NumLock),
                    egui::Key::ScrollLock => Some(input::Key::ScrollLock),
                    egui::Key::F1 => Some(input::Key::F(1)),
                    egui::Key::F2 => Some(input::Key::F(2)),
                    egui::Key::F3 => Some(input::Key::F(3)),
                    egui::Key::F4 => Some(input::Key::F(4)),
                    egui::Key::F5 => Some(input::Key::F(5)),
                    egui::Key::F6 => Some(input::Key::F(6)),
                    egui::Key::F7 => Some(input::Key::F(7)),
                    egui::Key::F8 => Some(input::Key::F(8)),
                    egui::Key::F9 => Some(input::Key::F(9)),
                    egui::Key::F10 => Some(input::Key::F(10)),
                    egui::Key::F11 => Some(input::Key::F(11)),
                    egui::Key::F12 => Some(input::Key::F(12)),
                    egui::Key::F13 => Some(input::Key::F(13)),
                    egui::Key::F14 => Some(input::Key::F(14)),
                    egui::Key::F15 => Some(input::Key::F(15)),
                    egui::Key::F16 => Some(input::Key::F(16)),
                    egui::Key::F17 => Some(input::Key::F(17)),
                    egui::Key::F18 => Some(input::Key::F(18)),
                    egui::Key::F19 => Some(input::Key::F(19)),
                    egui::Key::F20 => Some(input::Key::F(20)),
                    egui::Key::F21 => Some(input::Key::F(21)),
                    egui::Key::F22 => Some(input::Key::F(22)),
                    egui::Key::F23 => Some(input::Key::F(23)),
                    egui::Key::F24 => Some(input::Key::F(24)),
                    egui::Key::F25 => Some(input::Key::F(25)),
                    egui::Key::F26 => Some(input::Key::F(26)),
                    egui::Key::F27 => Some(input::Key::F(27)),
                    egui::Key::F28 => Some(input::Key::F(28)),
                    egui::Key::F29 => Some(input::Key::F(29)),
                    egui::Key::F30 => Some(input::Key::F(30)),
                    egui::Key::F31 => Some(input::Key::F(31)),
                    egui::Key::F32 => Some(input::Key::F(32)),
                    egui::Key::F33 => Some(input::Key::F(33)),
                    egui::Key::F34 => Some(input::Key::F(34)),
                    egui::Key::F35 => Some(input::Key::F(35)),
                    // Latin shortcuts with modifiers (no Text is produced for these).
                    egui::Key::A => base_char('a'),
                    egui::Key::B => base_char('b'),
                    egui::Key::C => base_char('c'),
                    egui::Key::D => base_char('d'),
                    egui::Key::E => base_char('e'),
                    egui::Key::F => base_char('f'),
                    egui::Key::G => base_char('g'),
                    egui::Key::H => base_char('h'),
                    egui::Key::I => base_char('i'),
                    egui::Key::J => base_char('j'),
                    egui::Key::K => base_char('k'),
                    egui::Key::L => base_char('l'),
                    egui::Key::M => base_char('m'),
                    egui::Key::N => base_char('n'),
                    egui::Key::O => base_char('o'),
                    egui::Key::P => base_char('p'),
                    egui::Key::Q => base_char('q'),
                    egui::Key::R => base_char('r'),
                    egui::Key::S => base_char('s'),
                    egui::Key::T => base_char('t'),
                    egui::Key::U => base_char('u'),
                    egui::Key::V => base_char('v'),
                    egui::Key::W => base_char('w'),
                    egui::Key::X => base_char('x'),
                    egui::Key::Y => base_char('y'),
                    egui::Key::Z => base_char('z'),
                    egui::Key::Num0 => base_char('0'),
                    egui::Key::Num1 => base_char('1'),
                    egui::Key::Num2 => base_char('2'),
                    egui::Key::Num3 => base_char('3'),
                    egui::Key::Num4 => base_char('4'),
                    egui::Key::Num5 => base_char('5'),
                    egui::Key::Num6 => base_char('6'),
                    egui::Key::Num7 => base_char('7'),
                    egui::Key::Num8 => base_char('8'),
                    egui::Key::Num9 => base_char('9'),
                    egui::Key::Comma => base_char(','),
                    egui::Key::Period => base_char('.'),
                    egui::Key::Slash => base_char('/'),
                    egui::Key::Backslash => base_char('\\'),
                    egui::Key::Semicolon => base_char(';'),
                    egui::Key::Quote => base_char('\''),
                    egui::Key::Minus => base_char('-'),
                    egui::Key::Equals => base_char('='),
                    egui::Key::Plus => base_char('+'),
                    egui::Key::Backtick => base_char('`'),
                    egui::Key::OpenBracket => base_char('['),
                    egui::Key::CloseBracket => base_char(']'),
                    egui::Key::OpenCurlyBracket => base_char('{'),
                    egui::Key::CloseCurlyBracket => base_char('}'),
                    egui::Key::Questionmark => base_char('?'),
                    egui::Key::Exclamationmark => base_char('!'),
                    egui::Key::Colon => base_char(':'),
                    egui::Key::Pipe => base_char('|'),
                    _ => None,
                };
                if let Some(k) = k.and_then(|k| input::keysym(k, &key_preset)) {
                    let _ = tx.send(protocol::Command::Key { keysym: k, pressed });
                    if !repeat {
                        let mut s = state.lock().unwrap();
                        if pressed {
                            if !s.held_keysyms.contains(&k) {
                                s.held_keysyms.push(k);
                            }
                        } else {
                            s.held_keysyms.retain(|&x| x != k);
                        }
                    }
                }
            }
            egui::Event::MouseWheel { delta, .. } => {
                // VNC scroll buttons: 4=up (mask 8), 5=down (mask 16), 6=left (mask 32), 7=right (mask 64).
                if let Some((x, y)) = ctx.input(|i| i.pointer.hover_pos()).and_then(&map_point) {
                    let mut s = state.lock().unwrap();
                    s.wheel_acc_y += delta.y * scroll_speed;
                    s.wheel_acc_x += delta.x * scroll_speed;
                    let mut pulse_count = 0u32;
                    while s.wheel_acc_y >= SCROLL_NOTCH_PT
                        && pulse_count < MAX_SCROLL_PULSES_PER_FRAME
                    {
                        send_scroll_pulse(&mut s, x, y, 8);
                        s.wheel_acc_y -= SCROLL_NOTCH_PT;
                        pulse_count += 1;
                    }
                    while s.wheel_acc_y <= -SCROLL_NOTCH_PT
                        && pulse_count < MAX_SCROLL_PULSES_PER_FRAME
                    {
                        send_scroll_pulse(&mut s, x, y, 16);
                        s.wheel_acc_y += SCROLL_NOTCH_PT;
                        pulse_count += 1;
                    }
                    while s.wheel_acc_x >= SCROLL_NOTCH_PT
                        && pulse_count < MAX_SCROLL_PULSES_PER_FRAME
                    {
                        send_scroll_pulse(&mut s, x, y, 64);
                        s.wheel_acc_x -= SCROLL_NOTCH_PT;
                        pulse_count += 1;
                    }
                    while s.wheel_acc_x <= -SCROLL_NOTCH_PT
                        && pulse_count < MAX_SCROLL_PULSES_PER_FRAME
                    {
                        send_scroll_pulse(&mut s, x, y, 32);
                        s.wheel_acc_x += SCROLL_NOTCH_PT;
                        pulse_count += 1;
                    }
                    if delta.y == 0.0 && s.wheel_acc_y.abs() < SCROLL_NOTCH_PT * 0.5 {
                        s.wheel_acc_y = 0.0;
                    }
                    if delta.x == 0.0 && s.wheel_acc_x.abs() < SCROLL_NOTCH_PT * 0.5 {
                        s.wheel_acc_x = 0.0;
                    }
                    // MAX_SCROLL_PULSES_PER_FRAME caps work per frame; keep draining
                    // on the next frame instead of stranding the leftover notches.
                    if s.wheel_acc_x.abs() >= SCROLL_NOTCH_PT
                        || s.wheel_acc_y.abs() >= SCROLL_NOTCH_PT
                    {
                        ctx.request_repaint_after(Duration::from_millis(16));
                    }
                }
            }
            egui::Event::Text(t) => {
                for c in t.chars().take(512) {
                    if ((c == '\r' || c == '\n') && key_has_return) || (c == '\t' && key_has_tab) {
                        continue; // Key path already sent it; avoid a double Enter/Tab.
                    }
                    let k = match c {
                        '\r' | '\n' => input::Key::Return,
                        '\t' => input::Key::Tab,
                        _ => input::Key::Character(c),
                    };
                    if let Some(k) = input::keysym(k, &key_preset) {
                        let _ = tx.send(protocol::Command::Key {
                            keysym: k,
                            pressed: true,
                        });
                        let _ = tx.send(protocol::Command::Key {
                            keysym: k,
                            pressed: false,
                        });
                    }
                }
            }
            _ => {}
        }
    }
}
// ---------- shared helpers ----------
fn accent(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode {
        rgb(0x4c, 0x8d, 0xff)
    } else {
        rgb(0x0b, 0x69, 0xda)
    }
}
fn muted(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode {
        rgb(0x9a, 0xa3, 0xb2)
    } else {
        rgb(0x5b, 0x64, 0x70)
    }
}
fn ok_color(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode {
        rgb(0x3f, 0xb9, 0x7f)
    } else {
        rgb(0x1a, 0x7f, 0x4b)
    }
}
fn divider(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode {
        rgb(0x26, 0x2b, 0x36)
    } else {
        rgb(0xe0, 0xe0, 0xe6)
    }
}
fn card_frame<R>(
    ui: &mut egui::Ui,
    selected: bool,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let visuals = ui.visuals();
    let fill = if visuals.dark_mode {
        if selected {
            rgb(0x1f, 0x26, 0x37)
        } else {
            rgb(0x1a, 0x1d, 0x24)
        }
    } else if selected {
        rgb(0xe4, 0xec, 0xfb)
    } else {
        rgb(0xff, 0xff, 0xff)
    };
    let stroke = if selected {
        egui::Stroke::new(1.0, accent(visuals))
    } else {
        egui::Stroke::new(1.0, divider(visuals))
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .inner_margin(egui::Margin::same(12))
        .corner_radius(10.0)
        .show(ui, contents)
        .inner
}
fn send_scroll_pulse(shared: &mut SessionShared, x: u16, y: u16, wheel_bit: u8) {
    // Every PointerEvent must carry the FULL current button mask. Sending only the
    // wheel bit and then 0 released a held drag button twice mid-gesture.
    let held = shared.button_mask;
    let (pressed, released) = input::scroll_pulse_buttons(held, wheel_bit);
    send_pointer(shared, x, y, pressed);
    send_pointer(shared, x, y, released);
}
fn save_color_image_png(image: &egui::ColorImage, path: &str) -> io::Result<()> {
    let size = image.size;
    let bytes = image.as_raw().to_vec();
    let buffer =
        image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(size[0] as u32, size[1] as u32, bytes)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid image dimensions"))?;
    buffer.save(path).map_err(io::Error::other)
}

impl GlintApp {
    // ---------- settings window (root viewport only) ----------
    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new(format!(
            "{}  {}",
            egui_phosphor::regular::GEAR,
            self.tr("settings.title")
        ))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_min_width(480.0);
            ui.add_space(2.0);

            // ---- Group 1: Display
            self.settings_group(ui, "settings.display", |ui, this| {
                ui.label(egui::RichText::new(this.tr("settings.language")).strong());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for (code, label) in [("ja", "日本語"), ("en", "English")] {
                        let active = this.store.settings.language == code;
                        if ui
                            .add_sized([210.0, 34.0], egui::Button::selectable(active, label))
                            .clicked()
                        {
                            this.store.settings.language = code.into();
                            this.persist();
                        }
                    }
                });
                ui.add_space(10.0);
                ui.separator();

                ui.add_space(10.0);
                ui.label(egui::RichText::new(this.tr("settings.theme")).strong());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for (key, value) in [
                        ("settings.theme_dark", "dark"),
                        ("settings.theme_light", "light"),
                        ("settings.theme_system", "system"),
                    ] {
                        let active = this.store.settings.theme == value;
                        if ui
                            .add_sized(
                                [142.0, 34.0],
                                egui::Button::selectable(active, this.tr(key)),
                            )
                            .clicked()
                        {
                            this.store.settings.theme = value.into();
                            apply_theme(ui.ctx(), value);
                            this.persist();
                        }
                    }
                });
                ui.add_space(10.0);
                ui.separator();

                ui.add_space(10.0);
                ui.label(egui::RichText::new(this.tr("settings.scroll")).strong());
                ui.horizontal(|ui| {
                    let mut speed = this.store.settings.scroll_speed;
                    let resp = ui.add(
                        egui::Slider::new(&mut speed, 0.25..=4.0)
                            .step_by(0.25)
                            .suffix("×")
                            .show_value(false),
                    );
                    if resp.changed() {
                        this.set_scroll_speed(speed);
                    }
                    if resp.drag_stopped() || resp.lost_focus() {
                        this.persist();
                    }
                    ui.label(
                        egui::RichText::new(format!("{:.2}×", this.store.settings.scroll_speed))
                            .monospace(),
                    );
                    if ui.small_button(this.tr("settings.scroll_reset")).clicked() {
                        this.set_scroll_speed(1.0);
                        this.persist();
                    }
                });
            });

            ui.add_space(10.0);

            // ---- Group 2: Remote desktop
            self.settings_group(ui, "settings.remote", |ui, this| {
                let mut clipboard_sync = this.store.settings.clipboard_sync;
                if ui
                    .checkbox(&mut clipboard_sync, this.tr("settings.clipboard"))
                    .changed()
                {
                    this.store.settings.clipboard_sync = clipboard_sync;
                    this.persist();
                }
                ui.label(
                    egui::RichText::new(this.tr("settings.clipboard_hint"))
                        .small()
                        .color(muted(ui.visuals())),
                );
            });

            ui.add_space(10.0);

            // ---- Group 3: About
            self.settings_group(ui, "settings.about", |ui, this| {
                ui.label(
                    egui::RichText::new(format!("GlintVNC {}", env!("CARGO_PKG_VERSION"))).strong(),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(this.tr("settings.about_body"))
                        .small()
                        .color(muted(ui.visuals())),
                );
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);
            if ui
                .add(egui::Button::new(self.tr("settings.close")).min_size(egui::vec2(96.0, 32.0)))
                .clicked()
            {
                self.show_settings = false;
            }
        });
        self.show_settings = open;
    }
    fn settings_group(
        &mut self,
        ui: &mut egui::Ui,
        title_key: &str,
        contents: impl FnOnce(&mut egui::Ui, &mut Self),
    ) {
        ui.label(
            egui::RichText::new(self.tr(title_key))
                .small()
                .strong()
                .color(muted(ui.visuals())),
        );
        ui.add_space(4.0);
        let y = ui.cursor().min.y + 2.0;
        let rect = ui.available_rect_before_wrap();
        ui.painter().line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0, divider(ui.visuals())),
        );
        ui.add_space(8.0);
        contents(ui, self);
    }
    fn maybe_screenshot(&mut self, ctx: &egui::Context) {
        let Ok(path) = std::env::var("GLINTVNC_SCREENSHOT") else {
            return;
        };
        let delay = std::env::var("GLINTVNC_SCREENSHOT_DELAY_SEC")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(6);
        if self.screenshot_requested {
            return;
        }
        if self.started.elapsed() < Duration::from_secs(delay) {
            ctx.request_repaint_after(Duration::from_millis(250));
            return;
        }
        self.screenshot_requested = true;
        let ctx2 = ctx.clone();
        ctx.request_screenshot(move |image| {
            match save_color_image_png(&image, &path) {
                Ok(()) => eprintln!("glintvnc: screenshot saved to {path}"),
                Err(e) => eprintln!("glintvnc: screenshot failed: {e}"),
            }
            ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
        });
    }
}

impl eframe::App for GlintApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_sessions(&ctx);
        if self.status.is_empty() {
            self.status = self.tr("status.ready").into();
        }
        let visuals = ui.visuals().clone();
        let panel_fill = visuals.panel_fill;
        let divider = if visuals.dark_mode {
            rgb(0x26, 0x26, 0x2b)
        } else {
            rgb(0xe2, 0xe2, 0xe8)
        };
        let panel = |margin: egui::Margin| {
            egui::Frame::new()
                .fill(panel_fill)
                .stroke(egui::Stroke::new(1.0, divider))
                .inner_margin(margin)
        };
        egui::Panel::top("chrome")
            .frame(panel(egui::Margin::symmetric(6, 6)))
            .show(ui, |ui| self.chrome(ui));
        egui::Panel::bottom("statusbar")
            .frame(panel(egui::Margin::symmetric(6, 4)))
            .show(ui, |ui| self.statusbar(ui));
        egui::Panel::left("profiles")
            .resizable(true)
            .default_size(300.0)
            .size_range(230.0..=420.0)
            .frame(panel(egui::Margin::same(12)))
            .show(ui, |ui| self.home_left(ui));
        egui::CentralPanel::default().show(ui, |ui| self.home_form(ui));
        if self.show_settings {
            self.settings_window(&ctx);
        }
        // ---- connection windows (child viewports) ----
        let session_ids: Vec<egui::ViewportId> = self.sessions.keys().copied().collect();
        for (idx, vid) in session_ids.into_iter().enumerate() {
            let profile_name = self
                .sessions
                .get(&vid)
                .and_then(|slot| slot.state.try_lock().ok().map(|s| s.profile.name.clone()))
                .unwrap_or_else(|| "GlintVNC".into());
            let cascade = (idx % 6) as f32;
            let builder = egui::ViewportBuilder::default()
                .with_title(format!("{profile_name} — GlintVNC"))
                .with_inner_size([1100.0, 680.0])
                .with_min_inner_size([800.0, 500.0])
                .with_position(egui::pos2(280.0 + cascade * 40.0, 180.0 + cascade * 28.0));
            let state = self.sessions[&vid].state.clone();
            ctx.show_viewport_deferred(vid, builder, move |ui, _class| {
                session_window_ui(ui, state.clone());
            });
        }
        self.maybe_screenshot(&ctx);
        if !self.sessions.is_empty() {
            // keep the clipboard poll and reconnect backoff ticking
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
}
