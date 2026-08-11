//! Animation definitions, time-based player, and idle picker (tech §8.2).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Deserialize;
use tracing::{info, warn};

use crate::error::AppError;

#[derive(Debug, Clone, Deserialize)]
pub struct AnimationMeta {
    pub name: String,
    pub frame_width: u32,
    pub frame_height: u32,
    pub frames: u32,
    pub fps: f32,
    #[serde(default = "default_true")]
    pub r#loop: bool,
    #[serde(default)]
    pub files: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// One loaded animation clip with spritesheet RGBA (horizontal strip).
#[derive(Debug, Clone)]
pub struct AnimationClip {
    pub name: String,
    pub frame_width: u32,
    pub frame_height: u32,
    pub frame_count: u32,
    pub fps: f32,
    pub looping: bool,
    /// Horizontal strip: frame_count * frame_width by frame_height.
    pub sheet_rgba: Vec<u8>,
    pub sheet_width: u32,
    pub sheet_height: u32,
}

impl AnimationClip {
    pub fn uv_for_frame(&self, frame: u32) -> [f32; 4] {
        let n = self.frame_count.max(1);
        let f = frame % n;
        let u0 = f as f32 / n as f32;
        let u1 = (f + 1) as f32 / n as f32;
        [u0, 0.0, u1, 1.0]
    }

    pub fn frame_rgba(&self, frame: u32) -> Vec<u8> {
        let n = self.frame_count.max(1);
        let f = (frame % n) as usize;
        let w = self.frame_width as usize;
        let h = self.frame_height as usize;
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            let src_off = (y * self.sheet_width as usize + f * w) * 4;
            let dst_off = y * w * 4;
            out[dst_off..dst_off + w * 4]
                .copy_from_slice(&self.sheet_rgba[src_off..src_off + w * 4]);
        }
        out
    }

    /// Sample a frame for display.
    ///
    /// Uses **nearest frame** (no inter-frame blend). Sub-frame blending of two
    /// slightly-shifted mouth/nose poses was reading as ghosting/double features
    /// on the desktop, especially during idle micro-motion and stretch.
    /// Looping wraps; one-shot clamps to last frame.
    pub fn frame_rgba_smooth(&self, frame_f: f32) -> Vec<u8> {
        let n = self.frame_count.max(1);
        if n == 1 {
            return self.frame_rgba(0);
        }
        let max_f = (n - 1) as f32;
        let f = if self.looping {
            let m = n as f32;
            let mut x = frame_f % m;
            if x < 0.0 {
                x += m;
            }
            x
        } else {
            frame_f.clamp(0.0, max_f)
        };
        // Nearest: avoids mouth/nose double-exposure between frames.
        let nearest = f.round() as u32;
        let idx = if self.looping {
            nearest % n
        } else {
            nearest.min(n - 1)
        };
        self.frame_rgba(idx)
    }
}

/// Premultiplied-alpha lerp of two tightly packed RGBA buffers (same length).
/// Used for sub-frame sampling and clip crossfades.
pub fn blend_rgba_premul(a: &[u8], b: &[u8], t: f32) -> Vec<u8> {
    let n = a.len().min(b.len());
    let mut out = vec![0u8; n];
    let t = t.clamp(0.0, 1.0);
    let u = 1.0 - t;
    let mut i = 0;
    while i + 3 < n {
        let a0 = a[i + 3] as f32 / 255.0;
        let b0 = b[i + 3] as f32 / 255.0;
        let ar = a[i] as f32 * a0;
        let ag = a[i + 1] as f32 * a0;
        let ab = a[i + 2] as f32 * a0;
        let br = b[i] as f32 * b0;
        let bg = b[i + 1] as f32 * b0;
        let bb = b[i + 2] as f32 * b0;
        let oa = ar * u + br * t;
        let og = ag * u + bg * t;
        let ob = ab * u + bb * t;
        let aa = (a0 * u + b0 * t).clamp(0.0, 1.0);
        if aa < 1.0 / 255.0 {
            out[i] = 0;
            out[i + 1] = 0;
            out[i + 2] = 0;
            out[i + 3] = 0;
        } else {
            let inv = 1.0 / aa;
            out[i] = (oa * inv).clamp(0.0, 255.0) as u8;
            out[i + 1] = (og * inv).clamp(0.0, 255.0) as u8;
            out[i + 2] = (ob * inv).clamp(0.0, 255.0) as u8;
            out[i + 3] = (aa * 255.0).clamp(0.0, 255.0) as u8;
        }
        i += 4;
    }
    out
}

#[derive(Debug, Default)]
pub struct AnimationLibrary {
    clips: Vec<AnimationClip>,
}

impl AnimationLibrary {
    pub fn load_idle_set(pet_dir: &Path) -> Self {
        let names = [
            IDLE_BASE,
            "idle_stretch",
            "idle_cute",
            "idle_tail_wag",
            "idle_sleep",
            "idle_watch",
        ];
        let mut clips = Vec::new();
        for name in names {
            let dir = pet_dir.join(name);
            match load_clip_dir(&dir, name) {
                Ok(clip) => {
                    info!(anim = %clip.name, frames = clip.frame_count, "loaded animation");
                    clips.push(clip);
                }
                Err(e) => warn!(anim = name, error = %e, "failed to load animation"),
            }
        }

        if clips.is_empty() {
            warn!("no idle animations found; installing procedural fallback");
            clips.push(procedural_fallback_clip());
        }

        Self { clips }
    }

    pub fn get(&self, name: &str) -> Option<&AnimationClip> {
        self.clips.iter().find(|c| c.name == name)
    }

    /// One-shot idle action names actually scheduled by [`IdlePicker`].
    ///
    /// Only names in [`IDLE_ACTION_ENABLED`] that also loaded successfully.
    pub fn idle_action_names(&self) -> Vec<String> {
        IDLE_ACTION_ENABLED
            .iter()
            .filter(|n| self.get(n).is_some())
            .map(|n| (*n).to_string())
            .collect()
    }

    /// All idle-ish names (for debugging / legacy).
    pub fn idle_names(&self) -> Vec<String> {
        self.clips
            .iter()
            .map(|c| c.name.clone())
            .filter(|n| n.starts_with("idle_"))
            .collect()
    }

    pub fn first(&self) -> &AnimationClip {
        &self.clips[0]
    }

    /// Load interaction / movement animation clips (M2: ASSET-04, ASSET-05).
    /// Tries disk first, falls back to procedural generation.
    pub fn load_interaction_set(pet_dir: &Path) -> Vec<AnimationClip> {
        let specs = [
            ("approaching", "approaching", 8u32, 12f32),
            ("playing_interaction", "playing", 6, 10.0),
            ("edge_peek", "edge_peek", 4, 4.0),
            ("dragging", "dragging", 4, 8.0),
            ("reminder_wave", "reminder", 6, 8.0),
            ("reminder_feed", "feed", 6, 10.0),
        ];

        let mut clips = Vec::new();
        for (dir_name, style, frames, fps) in specs {
            let dir = pet_dir.join(dir_name);
            match load_clip_dir(&dir, dir_name) {
                Ok(clip) => {
                    info!(anim = %clip.name, frames = clip.frame_count, "loaded interaction animation");
                    clips.push(clip);
                }
                Err(_) => {
                    info!(
                        anim = dir_name,
                        "interaction dir not found; generating procedural clip"
                    );
                    clips.push(procedural_style_clip(dir_name, style, frames, fps));
                }
            }
        }
        clips
    }

    /// Load all animation clips (idle + interaction) in one call.
    pub fn load_all(pet_dir: &Path) -> Self {
        let mut clips = Vec::new();

        // Idle set: base blink + one-shot actions + watch (for Watching state)
        let names = [
            IDLE_BASE,
            "idle_stretch",
            "idle_cute",
            "idle_tail_wag",
            "idle_sleep",
            "idle_watch",
        ];
        for name in names {
            let dir = pet_dir.join(name);
            match load_clip_dir(&dir, name) {
                Ok(clip) => {
                    info!(anim = %clip.name, frames = clip.frame_count, "loaded animation");
                    clips.push(clip);
                }
                Err(e) => warn!(anim = name, error = %e, "failed to load animation"),
            }
        }

        // Interaction set
        clips.extend(Self::load_interaction_set(pet_dir));

        if clips.is_empty() {
            warn!("no animations found; installing procedural fallback");
            clips.push(procedural_fallback_clip());
        }

        Self { clips }
    }
}

fn load_clip_dir(dir: &Path, fallback_name: &str) -> Result<AnimationClip, AppError> {
    let meta_path = dir.join("meta.json");
    let meta_text = std::fs::read_to_string(&meta_path).map_err(|source| AppError::Io {
        path: meta_path.clone(),
        source,
    })?;
    let meta: AnimationMeta = serde_json::from_str(&meta_text)
        .map_err(|e| AppError::Asset(format!("parse {}: {e}", meta_path.display())))?;

    let files: Vec<PathBuf> = if meta.files.is_empty() {
        (0..meta.frames)
            .map(|i| dir.join(format!("{i:02}.png")))
            .collect()
    } else {
        meta.files.iter().map(|f| dir.join(f)).collect()
    };

    let mut frames_rgba = Vec::new();
    let mut fw = meta.frame_width;
    let mut fh = meta.frame_height;

    for path in &files {
        let img = image::open(path)
            .map_err(|e| AppError::Asset(format!("open {}: {e}", path.display())))?
            .to_rgba8();
        let (w, h) = img.dimensions();
        fw = w;
        fh = h;
        frames_rgba.push(img.into_raw());
    }

    if frames_rgba.is_empty() {
        return Err(AppError::Asset(format!("no frames in {}", dir.display())));
    }

    let frame_count = frames_rgba.len() as u32;
    let sheet_width = fw * frame_count;
    let sheet_height = fh;
    let mut sheet = vec![0u8; (sheet_width * sheet_height * 4) as usize];
    for (i, frame) in frames_rgba.iter().enumerate() {
        for y in 0..fh as usize {
            let src = y * fw as usize * 4;
            let dst = (y * sheet_width as usize + i * fw as usize) * 4;
            sheet[dst..dst + fw as usize * 4].copy_from_slice(&frame[src..src + fw as usize * 4]);
        }
    }

    Ok(AnimationClip {
        name: if meta.name.is_empty() {
            fallback_name.to_string()
        } else {
            meta.name
        },
        frame_width: fw,
        frame_height: fh,
        frame_count,
        fps: if meta.fps <= 0.0 { 10.0 } else { meta.fps },
        looping: meta.r#loop,
        sheet_rgba: sheet,
        sheet_width,
        sheet_height,
    })
}

fn procedural_fallback_clip() -> AnimationClip {
    let size = 128u32;
    let frames = 4u32;
    let mut sheet = vec![0u8; (size * frames * size * 4) as usize];
    for f in 0..frames {
        let phase = f as f32 / frames as f32;
        let frame = draw_cow_frame(size, "fallback", phase);
        for y in 0..size as usize {
            let src = y * size as usize * 4;
            let dst = (y * (size * frames) as usize + f as usize * size as usize) * 4;
            sheet[dst..dst + size as usize * 4]
                .copy_from_slice(&frame[src..src + size as usize * 4]);
        }
    }
    AnimationClip {
        name: "idle_fallback".into(),
        frame_width: size,
        frame_height: size,
        frame_count: frames,
        fps: 6.0,
        looping: true,
        sheet_rgba: sheet,
        sheet_width: size * frames,
        sheet_height: size,
    }
}

/// Generate a procedural clip for a specific interaction style (M2 ASSET-04/05).
fn procedural_style_clip(name: &str, style: &str, frames: u32, fps: f32) -> AnimationClip {
    let size = 128u32;
    let mut sheet = vec![0u8; (size * frames * size * 4) as usize];
    for f in 0..frames {
        let phase = f as f32 / frames as f32;
        let frame = draw_cow_frame(size, style, phase);
        for y in 0..size as usize {
            let src = y * size as usize * 4;
            let dst = (y * (size * frames) as usize + f as usize * size as usize) * 4;
            sheet[dst..dst + size as usize * 4]
                .copy_from_slice(&frame[src..src + size as usize * 4]);
        }
    }
    AnimationClip {
        name: name.to_string(),
        frame_width: size,
        frame_height: size,
        frame_count: frames,
        fps,
        looping: true,
        sheet_rgba: sheet,
        sheet_width: size * frames,
        sheet_height: size,
    }
}

/// Shared procedural cow-cat frame for tooling / fallback.
pub fn draw_cow_frame(size: u32, style: &str, phase: f32) -> Vec<u8> {
    let mut data = vec![0u8; (size * size * 4) as usize];
    let s = size as i32;
    let wobble = (phase * std::f32::consts::TAU).sin();

    let (stretch_y, ear_boost, eye_closed, eye_dx, tail_wobble, blush) = match style {
        "tail_wag" => (1.0, 0, false, 0, (wobble * 10.0) as i32, false),
        "stretch" => (1.0 + wobble.abs() * 0.15, 0, false, 0, 0, false),
        "cute" => (1.0, 4, false, 0, 0, true),
        "sleep" => (0.9, -2, true, 0, 0, false),
        "watch" => (1.0, 0, false, (wobble * 6.0) as i32, 0, false),
        // M2 interaction styles
        "approaching" => (
            0.95,
            2,
            false,
            (wobble * 3.0) as i32,
            (-wobble.abs() * 12.0) as i32,
            false,
        ),
        "playing" => (0.92, 6, false, 0, (wobble * 8.0) as i32, true),
        "edge_peek" => (1.0, 0, wobble < 0.0, 0, 0, false),
        "dragging" => (1.05, -1, false, 0, (wobble * 6.0) as i32, false),
        "reminder" => (1.0, 4, false, 0, (wobble * 6.0) as i32, false),
        "feed" => (0.95, 2, false, 0, 0, true),
        _ => (1.0, 0, false, 0, (wobble * 4.0) as i32, false),
    };

    let cx = s / 2 + tail_wobble / 3;
    let cy = (s as f32 / 2.0 + 8.0 * stretch_y) as i32;

    for y in 0..s {
        for x in 0..s {
            let mut a = 0u8;
            let mut r = 0u8;
            let mut g = 0u8;
            let mut b = 0u8;

            let dx = x - cx;
            let dy = ((y - cy) as f32 / stretch_y) as i32;
            let head_r = s * 38 / 100;
            let in_head = dx * dx + dy * dy <= head_r * head_r;

            let ear_y = cy - s * 32 / 100 - ear_boost;
            let left_ear = {
                let ex = x - (cx - s * 22 / 100);
                let ey = y - ear_y;
                ex * ex + ey * ey <= (s * 12 / 100) * (s * 12 / 100)
            };
            let right_ear = {
                let ex = x - (cx + s * 22 / 100);
                let ey = y - ear_y;
                ex * ex + ey * ey <= (s * 12 / 100) * (s * 12 / 100)
            };

            let body_cy = cy + (s as f32 * 0.28 * stretch_y) as i32;
            let in_body = {
                let bx = x - cx;
                let by = y - body_cy;
                bx * bx * 2 + by * by <= (s * 28 / 100) * (s * 28 / 100)
            };

            // Tail blob for wag
            let in_tail = {
                let tx = x - (cx + s * 30 / 100 + tail_wobble);
                let ty = y - (cy + s * 20 / 100);
                tx * tx + ty * ty <= (s * 8 / 100) * (s * 8 / 100)
            };

            // edge_peek: only draw head + ears (body is hidden off-screen).
            let suppress_body = style == "edge_peek";

            if in_head
                || left_ear
                || right_ear
                || (in_body && !suppress_body)
                || (in_tail && !suppress_body)
            {
                let spot = ((x * 13 + y * 7) % 47 < 12) || (dx.abs() < s / 10 && dy.abs() < s / 8);
                if spot {
                    r = 0x2B;
                    g = 0x2B;
                    b = 0x2E;
                } else {
                    r = 0xFF;
                    g = 0xFF;
                    b = 0xFF;
                }

                // Eyes
                let eye_y = cy - 4;
                let left_eye = (x - (cx - 10 + eye_dx)).abs() <= 2 && (y - eye_y).abs() <= 2;
                let right_eye = (x - (cx + 10 + eye_dx)).abs() <= 2 && (y - eye_y).abs() <= 2;
                if left_eye || right_eye {
                    if eye_closed {
                        r = 0x2B;
                        g = 0x2B;
                        b = 0x2E;
                    } else {
                        r = 0x1A;
                        g = 0x1A;
                        b = 0x1E;
                    }
                }

                // Nose
                if dx.abs() < 3 && (y - (cy + 6)).abs() < 2 {
                    r = 0xFF;
                    g = 0xB6;
                    b = 0xC1;
                }

                // Blush for cute
                if blush {
                    let bl = (x - (cx - 16)).abs() <= 3 && (y - (cy + 4)).abs() <= 2;
                    let br = (x - (cx + 16)).abs() <= 3 && (y - (cy + 4)).abs() <= 2;
                    if bl || br {
                        r = 0xFF;
                        g = 0x9E;
                        b = 0xC4;
                    }
                }

                // Sleep Z
                if style == "sleep" && phase > 0.3 {
                    let zx = cx + 28;
                    let zy = cy - 28 - (phase * 10.0) as i32;
                    if (x - zx).abs() <= 4 && (y - zy).abs() <= 1 {
                        r = 0x8E;
                        g = 0xD1;
                        b = 0xD6;
                    }
                }

                a = 255;
            }

            let i = ((y as u32 * size + x as u32) * 4) as usize;
            data[i] = r;
            data[i + 1] = g;
            data[i + 2] = b;
            data[i + 3] = a;
        }
    }
    data
}

#[derive(Debug)]
pub struct AnimationPlayer {
    clip_name: String,
    started_at: Instant,
    fps: f32,
    frame_count: u32,
    looping: bool,
    last_frame: u32,
}

impl AnimationPlayer {
    pub fn start(clip: &AnimationClip, now: Instant) -> Self {
        Self {
            clip_name: clip.name.clone(),
            started_at: now,
            fps: clip.fps,
            frame_count: clip.frame_count.max(1),
            looping: clip.looping,
            last_frame: 0,
        }
    }

    pub fn clip_name(&self) -> &str {
        &self.clip_name
    }

    /// Returns `(frame_index, finished_or_looped)`.
    ///
    /// - Looping clips: second flag is true once per cycle when wrapping to frame 0.
    /// - Non-looping clips: second flag is true when the last frame has been held
    ///   past the clip duration (one-shot finished).
    pub fn tick(&mut self, now: Instant) -> (u32, bool) {
        let elapsed = now.duration_since(self.started_at).as_secs_f32();
        let frame_f = elapsed * self.fps;
        let total = self.frame_count as f32;
        let frame = if self.looping {
            (frame_f as u32) % self.frame_count
        } else {
            (frame_f as u32).min(self.frame_count - 1)
        };
        let changed = frame != self.last_frame;
        self.last_frame = frame;

        let finished = if self.looping {
            // One cycle completed (wrap to 0 after advancing).
            changed && frame == 0 && frame_f >= total
        } else {
            // One-shot done: past last frame duration.
            frame_f >= total
        };
        (frame, finished)
    }

    pub fn current_frame(&self) -> u32 {
        self.last_frame
    }

    pub fn is_looping(&self) -> bool {
        self.looping
    }

    pub fn frame_count_pub(&self) -> u32 {
        self.frame_count
    }

    pub fn started_at_pub(&self) -> Instant {
        self.started_at
    }

    pub fn is_finished(&self, now: Instant) -> bool {
        if self.looping {
            return false;
        }
        let elapsed = now.duration_since(self.started_at).as_secs_f32();
        elapsed * self.fps >= self.frame_count as f32
    }
}

/// Default sit+blink clip name (looping base idle).
pub const IDLE_BASE: &str = "idle_blink";

/// Seconds between random one-shot idle actions while sitting / watching.
/// Product: one cute action about every minute (low-disturbance desktop pet).
pub const IDLE_ACTION_INTERVAL_SECS: f32 = 60.0;

/// One-shot pool currently enabled for polish.
///
/// Debug focus (2026-08-10): **only `idle_stretch`**.  
/// Other authored clips (`idle_cute` / `tail_wag` / `sleep`) stay on disk but are
/// **not** scheduled until re-listed here.
pub const IDLE_ACTION_ENABLED: &[&str] = &["idle_stretch"];

/// Picks one-shot idle actions on a fixed wall-clock interval.
///
/// Timer is **not** reset by brief `Watching` (mouse medium range) — only by
/// actually starting an action (and optionally forced reset after heavy states).
#[derive(Debug)]
pub struct IdlePicker {
    history: VecDeque<String>,
    history_limit: usize,
    /// One-shot action clip names (stretch, cute, …) — not the base blink.
    action_names: Vec<String>,
    action_interval_secs: f32,
    /// Wall clock of last completed / started cute action (or app start).
    last_action_at: Instant,
}

impl IdlePicker {
    pub fn new(action_names: Vec<String>, now: Instant) -> Self {
        // Optional override for local testing: PAWDESK_CUTE_SECS=10
        let interval = std::env::var("PAWDESK_CUTE_SECS")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|v| *v >= 1.0 && *v <= 600.0)
            .unwrap_or(IDLE_ACTION_INTERVAL_SECS);
        Self {
            history: VecDeque::new(),
            history_limit: 2,
            action_names,
            action_interval_secs: interval,
            last_action_at: now,
        }
    }

    pub fn base_name(&self) -> &'static str {
        IDLE_BASE
    }

    /// Always start from sit+blink. Does **not** reset the 30s cute timer
    /// (so Watching → Idle does not starve timed actions).
    pub fn pick_initial(&mut self, now: Instant) -> String {
        let _ = now;
        IDLE_BASE.to_string()
    }

    /// Call only after a cute action finishes or when leaving heavy states
    /// (menu / reminder / drag) if you want a full cooldown again.
    pub fn mark_action_done(&mut self, now: Instant) {
        self.last_action_at = now;
    }

    /// Legacy name kept for call sites that meant "back on base clip".
    pub fn mark_base(&mut self, _now: Instant) {
        // Intentionally empty: do not reset 30s on every go_idle.
    }

    /// True when the cute-action cooldown has elapsed (does **not** consume the timer).
    pub fn action_due(&self, now: Instant) -> bool {
        if self.action_names.is_empty() {
            return false;
        }
        let need = std::time::Duration::from_secs_f32(self.action_interval_secs.max(0.5));
        now.duration_since(self.last_action_at) >= need
    }

    /// Peek next action name without advancing history / timer (for logging).
    pub fn peek_action_names(&self) -> &[String] {
        &self.action_names
    }

    /// Pick and commit a cute action. Call only when you will actually start it.
    /// Updates `last_action_at` so the next cooldown starts from now.
    pub fn take_action(&mut self, now: Instant) -> Option<String> {
        if !self.action_due(now) {
            return None;
        }
        let choice = self.pick_action();
        self.last_action_at = now;
        Some(choice)
    }

    /// Legacy: due-check + take. Prefer [`action_due`] / [`take_action`] so a failed
    /// start does not burn the cooldown.
    pub fn maybe_start_action(&mut self, now: Instant) -> Option<String> {
        self.take_action(now)
    }

    /// Seconds until next scheduled cute action (for debug / UI).
    pub fn secs_until_action(&self, now: Instant) -> f32 {
        let elapsed = now.duration_since(self.last_action_at).as_secs_f32();
        (self.action_interval_secs - elapsed).max(0.0)
    }

    /// Interval used by this picker (may differ from default if overridden).
    pub fn interval_secs(&self) -> f32 {
        self.action_interval_secs
    }

    fn pick_action(&mut self) -> String {
        let candidates: Vec<&String> = self
            .action_names
            .iter()
            .filter(|n| !self.history.contains(n))
            .collect();
        let pool: Vec<&String> = if candidates.is_empty() {
            self.action_names.iter().collect()
        } else {
            candidates
        };
        let choice = if pool.is_empty() {
            "idle_stretch".into()
        } else {
            let idx = simple_index(pool.len());
            pool[idx].clone()
        };
        self.history.push_back(choice.clone());
        while self.history.len() > self.history_limit {
            self.history.pop_front();
        }
        choice
    }
}

#[cfg(test)]
mod idle_picker_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn initial_is_base_blink() {
        let t0 = Instant::now();
        let mut p = IdlePicker::new(
            vec!["idle_stretch".into(), "idle_cute".into()],
            t0,
        );
        assert_eq!(p.pick_initial(t0), IDLE_BASE);
    }

    #[test]
    fn no_action_before_interval() {
        let t0 = Instant::now();
        let mut p = IdlePicker::new(vec!["idle_stretch".into()], t0);
        assert!(!p.action_due(t0 + Duration::from_secs(30)));
        assert!(p.maybe_start_action(t0 + Duration::from_secs(30)).is_none());
    }

    #[test]
    fn action_after_interval() {
        let t0 = Instant::now();
        let mut p = IdlePicker::new(vec!["idle_stretch".into()], t0);
        // Wall clock: one cute action about every minute.
        assert!(p.action_due(t0 + Duration::from_secs(61)));
        let a = p.take_action(t0 + Duration::from_secs(61));
        assert_eq!(a.as_deref(), Some("idle_stretch"));
        // Cooldown restarts after take.
        assert!(!p.action_due(t0 + Duration::from_secs(90)));
    }

    #[test]
    fn enabled_pool_is_stretch_only() {
        assert_eq!(IDLE_ACTION_ENABLED, &["idle_stretch"]);
    }

    #[test]
    fn watching_go_idle_does_not_reset_timer() {
        let t0 = Instant::now();
        let mut p = IdlePicker::new(vec!["idle_stretch".into()], t0);
        // Simulate ~40s on base, then "go_idle" from watching (mark_base no-op).
        p.mark_base(t0 + Duration::from_secs(40));
        // At 61s wall time, action should still fire (not reset at 40s).
        let a = p.maybe_start_action(t0 + Duration::from_secs(61));
        assert!(a.is_some());
    }

    #[test]
    fn action_due_uses_duration_not_float_edge() {
        let t0 = Instant::now();
        let p = IdlePicker::new(vec!["idle_stretch".into()], t0);
        assert!(!p.action_due(t0 + Duration::from_millis(59_900)));
        assert!(p.action_due(t0 + Duration::from_secs(60)));
    }

    #[test]
    fn oneshot_player_finishes() {
        let clip = AnimationClip {
            name: "act".into(),
            frame_width: 1,
            frame_height: 1,
            frame_count: 4,
            fps: 10.0,
            looping: false,
            sheet_rgba: vec![0; 4 * 4],
            sheet_width: 4,
            sheet_height: 1,
        };
        let t0 = Instant::now();
        let mut player = AnimationPlayer::start(&clip, t0);
        let (_f, done) = player.tick(t0 + Duration::from_millis(50));
        assert!(!done);
        let (_f, done) = player.tick(t0 + Duration::from_millis(450));
        assert!(done);
    }
}

fn simple_index(len: usize) -> usize {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    if len == 0 {
        return 0;
    }
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seed = t
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(c.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    (seed as usize) % len
}
