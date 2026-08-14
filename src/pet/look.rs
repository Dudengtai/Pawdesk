//! Cursor look: pupils first, then a small head turn. No whole-sprite flip.

use crate::event::Point;
use crate::pet::interaction::FAR_THRESHOLD;

/// Screen px at which look reaches full strength.
const LOOK_RANGE_PX: f64 = 220.0;
/// Start fading look back to center past this distance.
const LOOK_FADE_START: f64 = 180.0;

/// Eye ease time constant (seconds) — pupils lead.
const EYE_TAU: f32 = 0.075;
/// Head ease time constant while tracking.
const HEAD_TAU: f32 = 0.155;
/// Slower return when the cursor walks away.
const EYE_RETURN_TAU: f32 = 0.22;
const HEAD_RETURN_TAU: f32 = 0.32;

/// Max iris shift on a 256×256 master frame. Keep inside the socket.
const PUPIL_MAX_X: f32 = 3.0;
const PUPIL_MAX_Y: f32 = 2.2;
/// Layout measured on `idle_blink/000.png` (256×256).
const REF: f32 = 256.0;
const EYE_L: (f32, f32, f32, f32) = (122.5, 70.3, 15.0, 16.0);
const EYE_R: (f32, f32, f32, f32) = (170.4, 70.4, 15.0, 16.0);
pub const LOOK_YAW: &str = "look_yaw";
pub const LOOK_PITCH: &str = "look_pitch";
pub const LOOK_DIAG: &str = "look_diag";
/// Use baked look frames once the head has turned this far.
/// Kept near the first authored key (~0.33 on a 7-key yaw) so we don't
/// flash a half-turn while pupils are still carrying the look.
pub const YAW_STRIP_DEADZONE: f32 = 0.20;
/// Stay on the last pose unless the new one is clearly closer (stops Voronoi flicker).
const POSE_HYSTERESIS: f32 = 0.12;
/// Extra distance required before leaving the current strip (yaw↔pitch↔diag).
const STRIP_STICK: f32 = 0.16;
/// `look_yaw` was packed as 7 authored keys + 6 DIS in-betweens (13 frames).
/// Odd frames warp the body and read as shake; only even keys are used.
const YAW_PACKED_WITH_INBETWEENS: u32 = 13;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookStrip {
    Yaw,
    Pitch,
    Diag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LookPosePick {
    pub strip: LookStrip,
    pub frame: u32,
}

#[derive(Debug, Clone)]
pub struct LookController {
    pub target: (f32, f32),
    pub eye: (f32, f32),
    pub head: (f32, f32),
    pub curious_target: f32,
    pub curious: f32,
    last_pose: Option<LookPosePick>,
}

impl Default for LookController {
    fn default() -> Self {
        Self {
            target: (0.0, 0.0),
            eye: (0.0, 0.0),
            head: (0.0, 0.0),
            curious_target: 0.0,
            curious: 0.0,
            last_pose: None,
        }
    }
}

impl LookController {
    pub fn set_from_cursor(&mut self, cursor: Point, pet_center: Point, track: bool) {
        if !track {
            self.target = (0.0, 0.0);
            return;
        }
        self.target = look_target(cursor, pet_center);
    }

    pub fn set_curious(&mut self, on: bool) {
        self.curious_target = if on { 1.0 } else { 0.0 };
    }

    pub fn snap_curious_off(&mut self) {
        self.curious = 0.0;
        self.curious_target = 0.0;
    }

    /// Instantly face front (reminder hop / hard interrupt).
    pub fn snap_front(&mut self) {
        self.target = (0.0, 0.0);
        self.eye = (0.0, 0.0);
        self.head = (0.0, 0.0);
        self.curious = 0.0;
        self.curious_target = 0.0;
        self.last_pose = None;
    }

    /// Exponential approach. Returns true if the pose moved enough to redraw.
    pub fn tick(&mut self, dt: f32) -> bool {
        let dt = dt.clamp(0.0, 0.08);
        let returning = self.target.0.abs() < 0.04 && self.target.1.abs() < 0.04;
        let (te, th) = if returning {
            (EYE_RETURN_TAU, HEAD_RETURN_TAU)
        } else {
            (EYE_TAU, HEAD_TAU)
        };
        let ke = 1.0 - (-dt / te).exp();
        let kh = 1.0 - (-dt / th).exp();
        let kc = 1.0 - (-dt / 0.22).exp();
        let prev = (self.eye, self.head, self.curious);
        self.eye.0 += (self.target.0 - self.eye.0) * ke;
        self.eye.1 += (self.target.1 - self.eye.1) * ke;
        self.head.0 += (self.target.0 - self.head.0) * kh;
        self.head.1 += (self.target.1 - self.head.1) * kh;
        self.curious += (self.curious_target - self.curious) * kc;
        (self.eye.0 - prev.0.0).abs() > 0.002
            || (self.eye.1 - prev.0.1).abs() > 0.002
            || (self.head.0 - prev.1.0).abs() > 0.002
            || (self.head.1 - prev.1.1).abs() > 0.002
            || (self.curious - prev.2).abs() > 0.002
    }

    pub fn is_active(&self) -> bool {
        self.eye.0.abs() > 0.01
            || self.eye.1.abs() > 0.01
            || self.head.0.abs() > 0.01
            || self.head.1.abs() > 0.01
            || self.target.0.abs() > 0.01
            || self.target.1.abs() > 0.01
            || self.curious > 0.01
            || self.curious_target > 0.01
    }
}

pub fn look_target(cursor: Point, pet_center: Point) -> (f32, f32) {
    let dx = cursor.x - pet_center.x;
    let dy = cursor.y - pet_center.y;
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < 1.0 {
        return (0.0, 0.0);
    }
    let mut tx = (dx / LOOK_RANGE_PX).clamp(-1.0, 1.0) as f32;
    let mut ty = (dy / LOOK_RANGE_PX).clamp(-1.0, 1.0) as f32;
    if dist > FAR_THRESHOLD {
        return (0.0, 0.0);
    }
    if dist > LOOK_FADE_START {
        let u = ((FAR_THRESHOLD - dist) / (FAR_THRESHOLD - LOOK_FADE_START)) as f32;
        tx *= u.clamp(0.0, 1.0);
        ty *= u.clamp(0.0, 1.0);
    }
    (tx, ty)
}

/// Map look.x ∈ [-1, 1] onto a baked yaw strip (0 = far left).
pub fn yaw_frame_index(look_x: f32, frame_count: u32) -> u32 {
    if frame_count == YAW_PACKED_WITH_INBETWEENS {
        return axis_frame_index(look_x, 7).saturating_mul(2);
    }
    axis_frame_index(look_x, frame_count)
}

/// Map look.y onto a baked pitch strip (file 0 = look down).
/// Screen +y is down, so a cursor below the pet (+look_y) selects frame 0.
pub fn pitch_frame_index(look_y: f32, frame_count: u32) -> u32 {
    axis_frame_index(-look_y, frame_count)
}

fn axis_frame_index(look: f32, frame_count: u32) -> u32 {
    let n = frame_count.max(1);
    if n == 1 {
        return 0;
    }
    let t = ((look + 1.0) * 0.5).clamp(0.0, 1.0);
    (t * (n - 1) as f32).round() as u32
}

/// Built-in (x, y) for `look_diag` in screen space (+x right, +y down).
pub fn diag_pose_xy(frame: u32) -> (f32, f32) {
    match frame {
        0 => (-0.50, 0.75),  // down-left
        1 => (-0.50, -0.75), // up-left
        2 => (0.50, 0.75),   // down-right
        _ => (0.50, -0.75),  // up-right
    }
}

fn pose_dist2(head: (f32, f32), pose: (f32, f32)) -> f32 {
    let dx = head.0 - pose.0;
    let dy = head.1 - pose.1;
    dx * dx + dy * dy
}

/// Pick the nearest baked pose among yaw / pitch / diagonal strips.
///
/// `last` is the previous pick (hysteresis). Screen Y is positive down, so
/// looking at the desk is +y and maps onto pitch frame 0.
pub fn pick_look_pose(
    head: (f32, f32),
    yaw_frames: u32,
    pitch_frames: u32,
    diag_frames: u32,
    last: Option<LookPosePick>,
) -> LookPosePick {
    let mut best = LookPosePick {
        strip: LookStrip::Yaw,
        frame: yaw_frame_index(head.0, yaw_frames),
    };
    let mut best_d = pose_dist2(
        head,
        (
            axis_to_look(best.frame, yaw_frames),
            0.0,
        ),
    );

    if pitch_frames > 0 {
        let f = pitch_frame_index(head.1, pitch_frames);
        let d = pose_dist2(head, (0.0, -axis_to_look(f, pitch_frames)));
        if d < best_d {
            best_d = d;
            best = LookPosePick {
                strip: LookStrip::Pitch,
                frame: f,
            };
        }
    }

    if diag_frames > 0 {
        for i in 0..diag_frames {
            let xy = diag_pose_xy(i);
            let d = pose_dist2(head, xy);
            if d < best_d {
                best_d = d;
                best = LookPosePick {
                    strip: LookStrip::Diag,
                    frame: i,
                };
            }
        }
    }

    if let Some(prev) = last {
        let prev_xy = match prev.strip {
            LookStrip::Yaw => (axis_to_look(prev.frame, yaw_frames), 0.0),
            LookStrip::Pitch => (0.0, -axis_to_look(prev.frame, pitch_frames)),
            LookStrip::Diag => diag_pose_xy(prev.frame),
        };
        let prev_d = pose_dist2(head, prev_xy);
        let extra = if prev.strip != best.strip {
            STRIP_STICK
        } else {
            POSE_HYSTERESIS
        };
        if prev_d <= best_d + extra * extra {
            return prev;
        }
    }
    best
}

fn axis_to_look(frame: u32, frame_count: u32) -> f32 {
    let n = frame_count.max(1);
    if n == 1 {
        return 0.0;
    }
    -1.0 + 2.0 * frame as f32 / (n - 1) as f32
}

impl LookController {
    pub fn last_pose(&self) -> Option<LookPosePick> {
        self.last_pose
    }

    pub fn set_last_pose(&mut self, pick: Option<LookPosePick>) {
        self.last_pose = pick;
    }
}

/// Lock paws / tail / lower chest to the front master so authored look keys
/// don't bounce the sit silhouette when the head steps.
pub fn stabilize_look_rgba(look: &[u8], master: &[u8], w: u32, h: u32) -> Vec<u8> {
    stabilize_look_rgba_ex(look, master, w, h, false)
}

/// `lock_silhouette`: keep the master's sit outline (look-down keys are
/// face-only; their authored stroke is a jagged chroma-key fringe).
pub fn stabilize_look_rgba_ex(
    look: &[u8],
    master: &[u8],
    w: u32,
    h: u32,
    lock_silhouette: bool,
) -> Vec<u8> {
    const BODY_Y0: u32 = 160;
    const LOCK_Y: i32 = 158;
    const FEATHER: i32 = 16;
    let need = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    if w == 0 || h == 0 || look.len() < need || master.len() < need {
        return look.to_vec();
    }
    let mut dx = 0i32;
    let mut dy = 0i32;
    if let (Some((lcx, lcy)), Some((mcx, mcy))) = (
        body_centroid(look, w, h, BODY_Y0),
        body_centroid(master, w, h, BODY_Y0),
    ) {
        let ox = mcx - lcx;
        let oy = mcy - lcy;
        if ox.abs() >= 0.6 {
            dx = ox.round() as i32;
        }
        if oy.abs() >= 0.6 {
            dy = oy.round() as i32;
        }
        dx = dx.clamp(-8, 8);
        dy = dy.clamp(-6, 6);
    }
    let mut out = if dx == 0 && dy == 0 {
        look.to_vec()
    } else {
        translate_rgba(look, w, h, dx, dy)
    };
    for y in 0..h as i32 {
        let t = if y >= LOCK_Y + FEATHER {
            1.0f32
        } else if y <= LOCK_Y - FEATHER {
            continue;
        } else {
            (y - (LOCK_Y - FEATHER)) as f32 / (2.0 * FEATHER as f32)
        };
        for x in 0..w {
            let i = ((y as u32 * w + x) * 4) as usize;
            // Premultiplied lerp: straight RGB lerp pulls edge texels toward
            // black/magenta whenever the two silhouettes disagree.
            let a0 = out[i + 3] as f32 / 255.0;
            let b0 = master[i + 3] as f32 / 255.0;
            let oa = a0 * (1.0 - t) + b0 * t;
            if oa < 1.0 / 255.0 {
                out[i] = 0;
                out[i + 1] = 0;
                out[i + 2] = 0;
                out[i + 3] = 0;
                continue;
            }
            for c in 0..3 {
                let ac = out[i + c] as f32 * a0;
                let bc = master[i + c] as f32 * b0;
                out[i + c] = ((ac * (1.0 - t) + bc * t) / oa).round() as u8;
            }
            out[i + 3] = (oa * 255.0).round() as u8;
        }
    }
    if lock_silhouette {
        apply_master_silhouette(&mut out, master, w, h);
    }
    out
}

fn apply_master_silhouette(out: &mut [u8], master: &[u8], w: u32, h: u32) {
    let wi = w as i32;
    let hi = h as i32;
    for y in 0..hi {
        for x in 0..wi {
            let i = ((y as u32 * w + x as u32) * 4) as usize;
            let ma = master[i + 3];
            if ma < 8 {
                out[i] = 0;
                out[i + 1] = 0;
                out[i + 2] = 0;
                out[i + 3] = 0;
                continue;
            }
            let mut edge = false;
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = x + dx;
                let ny = y + dy;
                if nx < 0 || ny < 0 || nx >= wi || ny >= hi {
                    edge = true;
                    break;
                }
                let ni = ((ny as u32 * w + nx as u32) * 4 + 3) as usize;
                if master[ni] < 40 {
                    edge = true;
                    break;
                }
            }
            if edge {
                out[i..i + 4].copy_from_slice(&master[i..i + 4]);
            } else {
                out[i + 3] = ma;
            }
        }
    }
}

fn body_centroid(rgba: &[u8], w: u32, h: u32, y0: u32) -> Option<(f32, f32)> {
    let mut sx = 0.0f64;
    let mut sy = 0.0f64;
    let mut n = 0.0f64;
    for y in y0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4 + 3) as usize;
            if rgba[i] >= 16 {
                sx += f64::from(x);
                sy += f64::from(y);
                n += 1.0;
            }
        }
    }
    if n < 8.0 {
        None
    } else {
        Some(((sx / n) as f32, (sy / n) as f32))
    }
}

fn translate_rgba(src: &[u8], w: u32, h: u32, dx: i32, dy: i32) -> Vec<u8> {
    let mut out = vec![0u8; src.len()];
    let wi = w as i32;
    let hi = h as i32;
    for y in 0..hi {
        let sy = y - dy;
        if sy < 0 || sy >= hi {
            continue;
        }
        for x in 0..wi {
            let sx = x - dx;
            if sx < 0 || sx >= wi {
                continue;
            }
            let di = ((y as u32 * w + x as u32) * 4) as usize;
            let si = ((sy as u32 * w + sx as u32) * 4) as usize;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    out
}

/// `blink_frame`: 0 open, 1 half, 2 closed (idle_blink layout).
pub fn apply_look(
    rgba: &[u8],
    w: u32,
    h: u32,
    eye: (f32, f32),
    _head: (f32, f32),
    blink_frame: u32,
    _curious: f32,
) -> Vec<u8> {
    let mut out = rgba.to_vec();
    if w == 0 || h == 0 || out.len() < (w * h * 4) as usize {
        return out;
    }
    let lid = match blink_frame {
        2 => 0.0,
        1 => 0.35,
        _ => 1.0,
    };
    if lid > 0.01 {
        shift_pupils(&mut out, w, h, eye.0 * lid, eye.1 * lid);
    }
    out
}

fn shift_pupils(px: &mut [u8], w: u32, h: u32, lx: f32, ly: f32) {
    let sx = w as f32 / REF;
    let sy = h as f32 / REF;
    let dx = lx * PUPIL_MAX_X * sx;
    let dy = ly * PUPIL_MAX_Y * sy;
    if dx.abs() < 0.05 && dy.abs() < 0.05 {
        return;
    }
    let src = px.to_vec();
    for (cx, cy, rx, ry) in [EYE_L, EYE_R] {
        shift_ellipse(px, &src, w, h, cx * sx, cy * sy, rx * sx, ry * sy, dx, dy);
    }
}

fn shift_ellipse(
    dst: &mut [u8],
    src: &[u8],
    w: u32,
    h: u32,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    dx: f32,
    dy: f32,
) {
    let x0 = (cx - rx - 1.0).floor().max(0.0) as u32;
    let y0 = (cy - ry - 1.0).floor().max(0.0) as u32;
    let x1 = (cx + rx + 1.0).ceil().min(w as f32) as u32;
    let y1 = (cy + ry + 1.0).ceil().min(h as f32) as u32;
    let rx2 = rx * rx;
    let ry2 = ry * ry;
    if rx2 < 1.0 || ry2 < 1.0 {
        return;
    }
    for y in y0..y1 {
        for x in x0..x1 {
            let nx = (x as f32 - cx) / rx;
            let ny = (y as f32 - cy) / ry;
            if nx * nx + ny * ny > 0.92 {
                continue;
            }
            let i = ((y * w + x) * 4) as usize;
            if !is_iris(&src[i..i + 4]) {
                continue;
            }
            let sx = x as f32 - dx;
            let sy = y as f32 - dy;
            let snx = (sx - cx) / rx;
            let sny = (sy - cy) / ry;
            if snx * snx + sny * sny > 0.92 {
                continue;
            }
            let s = sample_bilinear(src, w, h, sx, sy);
            if s[3] < 200 || is_dark_fur(&s) {
                continue;
            }
            dst[i] = s[0];
            dst[i + 1] = s[1];
            dst[i + 2] = s[2];
            dst[i + 3] = s[3];
        }
    }
}

fn is_dark_fur(p: &[u8]) -> bool {
    if p[3] < 200 {
        return false;
    }
    let l = (p[0] as u16 + p[1] as u16 + p[2] as u16) / 3;
    l < 78 && p[0] < 90 && p[1] < 90
}

fn is_iris(p: &[u8]) -> bool {
    if p[3] < 200 {
        return false;
    }
    let r = p[0] as i16;
    let g = p[1] as i16;
    let b = p[2] as i16;
    let l = (r + g + b) / 3;
    // Amber iris or dark pupil, not the black socket ring / cheek fur.
    (r > 130 && g > 75 && b < 130 && r > b + 25) || (l < 48 && r < 70)
}

fn sample_bilinear(src: &[u8], w: u32, h: u32, x: f32, y: f32) -> [u8; 4] {
    if x < 0.0 || y < 0.0 || x >= (w as f32) - 1.0 || y >= (h as f32) - 1.0 {
        return [0, 0, 0, 0];
    }
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let p00 = pix(src, w, x0, y0);
    let p10 = pix(src, w, x1, y0);
    let p01 = pix(src, w, x0, y1);
    let p11 = pix(src, w, x1, y1);
    let mut out = [0u8; 4];
    for c in 0..4 {
        let a = p00[c] as f32 + (p10[c] as f32 - p00[c] as f32) * fx;
        let b = p01[c] as f32 + (p11[c] as f32 - p01[c] as f32) * fx;
        out[c] = (a + (b - a) * fy).clamp(0.0, 255.0) as u8;
    }
    out
}

fn pix(src: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [src[i], src[i + 1], src[i + 2], src[i + 3]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_cursor_looks_center() {
        let c = Point::new(1000.0, 0.0);
        let p = Point::new(0.0, 0.0);
        let t = look_target(c, p);
        assert_eq!(t, (0.0, 0.0));
    }

    #[test]
    fn near_right_looks_right() {
        let t = look_target(Point::new(120.0, 0.0), Point::new(0.0, 0.0));
        assert!(t.0 > 0.4 && t.0 <= 1.0);
        assert!(t.1.abs() < 0.01);
    }

    #[test]
    fn curious_eases_up() {
        let mut l = LookController::default();
        l.set_curious(true);
        for _ in 0..4 {
            l.tick(0.05);
        }
        assert!(l.curious > 0.5);
        l.set_curious(false);
        for _ in 0..8 {
            l.tick(0.05);
        }
        assert!(l.curious < 0.3);
    }

    #[test]
    fn apply_look_keeps_size() {
        let w = 32u32;
        let h = 32u32;
        let mut src = vec![0u8; (w * h * 4) as usize];
        for i in (0..src.len()).step_by(4) {
            src[i] = 200;
            src[i + 1] = 180;
            src[i + 2] = 160;
            src[i + 3] = 255;
        }
        let out = apply_look(&src, w, h, (0.8, -0.4), (0.5, 0.2), 0, 0.0);
        assert_eq!(out.len(), src.len());
    }

    #[test]
    fn yaw_index_maps_left_center_right() {
        assert_eq!(yaw_frame_index(-1.0, 7), 0);
        assert_eq!(yaw_frame_index(0.0, 7), 3);
        assert_eq!(yaw_frame_index(1.0, 7), 6);
        assert_eq!(yaw_frame_index(-1.0, 13), 0);
        assert_eq!(yaw_frame_index(0.0, 13), 6);
        assert_eq!(yaw_frame_index(1.0, 13), 12);
    }

    #[test]
    fn yaw_13_skips_flow_inbetweens() {
        for x in [-0.9, -0.5, -0.2, 0.2, 0.5, 0.9] {
            assert_eq!(
                yaw_frame_index(x, 13) % 2,
                0,
                "look_x={x} should land on an authored even frame"
            );
        }
    }

    #[test]
    fn pitch_index_maps_down_center_up() {
        // Screen +y is down → pitch file 0.
        assert_eq!(pitch_frame_index(1.0, 5), 0);
        assert_eq!(pitch_frame_index(0.0, 5), 2);
        assert_eq!(pitch_frame_index(-1.0, 5), 4);
    }

    #[test]
    fn pick_prefers_pitch_when_looking_down() {
        let p = pick_look_pose((0.05, 0.85), 13, 5, 4, None);
        assert_eq!(p.strip, LookStrip::Pitch);
        assert!(p.frame <= 1);
    }

    #[test]
    fn pick_prefers_diag_when_looking_up_right() {
        let p = pick_look_pose((0.55, -0.70), 13, 5, 4, None);
        assert_eq!(p.strip, LookStrip::Diag);
        assert_eq!(p.frame, 3);
    }

    #[test]
    fn pick_stays_on_yaw_for_mild_diagonal() {
        let last = LookPosePick {
            strip: LookStrip::Yaw,
            frame: 8,
        };
        let p = pick_look_pose((0.40, 0.28), 13, 5, 4, Some(last));
        assert_eq!(p.strip, LookStrip::Yaw);
        assert_eq!(p.frame, 8);
    }

    #[test]
    fn stabilize_keeps_size_and_locks_shifted_body() {
        let w = 32u32;
        let h = 32u32;
        let mut master = vec![0u8; (w * h * 4) as usize];
        for y in 20..30 {
            for x in 10..22 {
                let i = ((y * w + x) * 4) as usize;
                master[i] = 200;
                master[i + 1] = 180;
                master[i + 2] = 160;
                master[i + 3] = 255;
            }
        }
        let look = translate_rgba(&master, w, h, -3, 0);
        let out = stabilize_look_rgba(&look, &master, w, h);
        assert_eq!(out.len(), master.len());
        // Lower rows should match the unshifted master sit.
        let y = 28u32;
        let i = ((y * w + 16) * 4) as usize;
        assert_eq!(&out[i..i + 4], &master[i..i + 4]);
    }

    #[test]
    fn stabilize_premul_does_not_blacken_look_edge() {
        // Tall enough to hit LOCK_Y ± FEATHER (~142..174).
        let w = 8u32;
        let h = 180u32;
        let mut look = vec![0u8; (w * h * 4) as usize];
        let master = vec![0u8; look.len()];
        // Opaque cream fur on the look frame at the blend line; master is clear.
        let y = 158u32;
        let i = ((y * w + 3) * 4) as usize;
        look[i] = 220;
        look[i + 1] = 200;
        look[i + 2] = 180;
        look[i + 3] = 255;
        let out = stabilize_look_rgba(&look, &master, w, h);
        // Straight RGB lerp would pull (220,200,180) toward 0. Premul keeps hue.
        assert!(out[i] > 180, "r={}", out[i]);
        assert!(out[i + 1] > 160, "g={}", out[i + 1]);
        assert!(out[i + 2] > 140, "b={}", out[i + 2]);
        assert!(out[i + 3] > 80 && out[i + 3] < 200, "a={}", out[i + 3]);
    }

    #[test]
    fn lock_silhouette_drops_look_outline_outside_master() {
        let w = 16u32;
        let h = 16u32;
        let mut master = vec![0u8; (w * h * 4) as usize];
        let mut look = vec![0u8; master.len()];
        for y in 4..12 {
            for x in 4..12 {
                let i = ((y * w + x) * 4) as usize;
                master[i] = 30;
                master[i + 1] = 30;
                master[i + 2] = 30;
                master[i + 3] = 255;
            }
        }
        // Jagged look stroke one pixel outside the master.
        for y in 3..13 {
            for x in 3..13 {
                let i = ((y * w + x) * 4) as usize;
                look[i] = 180;
                look[i + 1] = 40;
                look[i + 2] = 80;
                look[i + 3] = 255;
            }
        }
        let out = stabilize_look_rgba_ex(&look, &master, w, h, true);
        let outside = ((3u32 * w + 3) * 4) as usize;
        assert_eq!(out[outside + 3], 0, "look fringe outside master must vanish");
        let inside = ((8u32 * w + 8) * 4) as usize;
        assert_eq!(out[inside + 3], 255);
    }
}
