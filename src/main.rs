#![windows_subsystem = "windows"]

use eframe::egui;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const WIN_W: f32 = 180.0;
const WIN_H: f32 = 180.0;
const TASKBAR_H: f32 = 50.0;
const FEET_PAD: f32 = 0.0;
const GRAVITY: f32 = 600.0;
const WORK_WARN_SECS: f32 = 30.0; //2.0 * 3600.0; // предупреждение через 2 часа кодинга
const CHAOS_GRACE_SECS: f32 = 10.0; //300.0;      // 5 минут после предупреждения до хаоса
const JUMP_V: f32 = 720.0; // сила прыжка за курсором
const GRAB_RADIUS: f32 = 50.0; // на каком расстоянии руки ловят курсор
const FIGHT_THRESHOLD: f32 = 45.0; // насколько дёрнуть курсор чтобы вырвать

// ── Chaos: позиция курсора ────────────────────────────────────────────────────

#[cfg(windows)]
fn get_cursor_screen_pos() -> egui::Pos2 {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    unsafe {
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        egui::pos2(p.x as f32, p.y as f32)
    }
}

#[cfg(not(windows))]
fn get_cursor_screen_pos() -> egui::Pos2 {
    egui::Pos2::ZERO
}

#[cfg(windows)]
fn set_cursor_screen_pos(x: f32, y: f32) {
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
    unsafe {
        let _ = SetCursorPos(x as i32, y as i32);
    }
}

#[cfg(not(windows))]
fn set_cursor_screen_pos(_x: f32, _y: f32) {}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(false)
            .with_inner_size([WIN_W, WIN_H])
            .with_position([200.0, 810.0]),
        ..Default::default()
    };
    eframe::run_native(
        "mascot",
        options,
        Box::new(|cc| Ok(Box::new(MascotApp::new(cc)))),
    )
}

// ── Определяем активное окно (только Windows) ──────────────────────────────

#[cfg(windows)]
fn foreground_info() -> (String, String) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };
    unsafe {
        let hwnd = GetForegroundWindow();

        let mut tbuf = [0u16; 512];
        let tlen = GetWindowTextW(hwnd, &mut tbuf);
        let title = String::from_utf16_lossy(&tbuf[..tlen as usize]).to_lowercase();

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .map(|h| {
                let mut buf = [0u16; 260];
                let mut sz = buf.len() as u32;
                let _ = QueryFullProcessImageNameW(
                    h,
                    PROCESS_NAME_WIN32,
                    windows::core::PWSTR(buf.as_mut_ptr()),
                    &mut sz,
                );
                let _ = CloseHandle(h);
                let path = String::from_utf16_lossy(&buf[..sz as usize]);
                path.split(['/', '\\']).last().unwrap_or("").to_lowercase()
            })
            .unwrap_or_default();

        (process, title)
    }
}

#[cfg(not(windows))]
fn foreground_info() -> (String, String) {
    (String::new(), String::new())
}

// ── Анимация спрайтов ────────────────────────────────────────────────────────

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum AnimKind {
    Idle, Walk, Coding, Watching, Dance,
    Angry, Grab, Hold, FallAnim, Drag, Sleep, Play,
}

impl AnimKind {
    fn folder(self) -> &'static str {
        match self {
            Self::Idle     => "idle",
            Self::Walk     => "walk",
            Self::Coding   => "coding",
            Self::Watching => "watching",
            Self::Dance    => "dance",
            Self::Angry    => "angry",
            Self::Grab     => "grab",
            Self::Hold     => "hold",
            Self::FallAnim => "fall",
            Self::Drag     => "drag",
            Self::Sleep    => "sleep",
            Self::Play     => "play",
        }
    }

    fn fps(self) -> f32 {
        match self {
            Self::Idle     => 2.0,
            Self::Walk     => 8.0,
            Self::Coding   => 3.0,
            Self::Watching => 1.0,
            Self::Dance    => 8.0,
            Self::Angry    => 6.0,
            Self::Grab     => 4.0,
            Self::Hold     => 4.0,
            Self::FallAnim => 2.0,
            Self::Drag     => 2.0,
            Self::Sleep    => 0.5,
            Self::Play     => 6.0,
        }
    }

    fn all() -> &'static [Self] {
        &[
            Self::Idle, Self::Walk, Self::Coding, Self::Watching, Self::Dance,
            Self::Angry, Self::Grab, Self::Hold, Self::FallAnim, Self::Drag,
            Self::Sleep, Self::Play,
        ]
    }
}

fn load_sprites(ctx: &egui::Context) -> HashMap<AnimKind, Vec<egui::TextureHandle>> {
    let mut map = HashMap::new();
    for &kind in AnimKind::all() {
        let folder = format!("assets/{}", kind.folder());
        let mut frames = Vec::new();
        for i in 1u32.. {
            let path = format!("{}/{}.png", folder, i);
            match image::open(&path) {
                Ok(img) => {
                    let img = img.into_rgba8();
                    let (w, h) = img.dimensions();
                    let pixels = img.into_raw();
                    let ci = egui::ColorImage::from_rgba_unmultiplied(
                        [w as usize, h as usize],
                        &pixels,
                    );
                    let name = format!("{}-{}", kind.folder(), i);
                    frames.push(ctx.load_texture(name, ci, egui::TextureOptions::LINEAR));
                }
                Err(_) => break,
            }
        }
        if !frames.is_empty() {
            map.insert(kind, frames);
        }
    }
    map
}

// ── Активность ──────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone, Copy)]
enum Activity {
    Normal,
    Coding,
    Watching,
    Music,
}

fn classify(process: &str, title: &str) -> Option<Activity> {
    if process.is_empty() || process == "ai_agent.exe" {
        return None;
    }

    // Claude Code desktop (Electron) — любой процесс с "claude" в имени
    if process.contains("claude") {
        return Some(Activity::Coding);
    }

    // IDE и редакторы
    if process.contains("webstorm")
        || process.contains("rider")
        || process.contains("clion")
        || process.contains("idea")
        || process == "code.exe"
        || process == "zed.exe"
        || process == "devenv.exe"
        || process == "fleet.exe"
        || process == "cursor.exe"
        || process == "notepad.exe"
        || process == "notepad++.exe"
        || process == "sublime_text.exe"
    {
        return Some(Activity::Coding);
    }

    // Claude Code / Codex / Cursor в терминале — смотрим заголовок окна
    let is_terminal = process == "windowsterminal.exe"
        || process == "cmd.exe"
        || process == "powershell.exe"
        || process == "pwsh.exe"
        || process == "wt.exe"
        || process == "alacritty.exe"
        || process == "wezterm-gui.exe";

    if is_terminal
        && (title.contains("claude") || title.contains("codex") || title.contains("cursor"))
    {
        return Some(Activity::Coding);
    }

    if process == "spotify.exe" {
        return Some(Activity::Music);
    }

    if (process == "chrome.exe" || process == "firefox.exe" || process == "msedge.exe")
        && title.contains("youtube")
    {
        return Some(Activity::Watching);
    }

    Some(Activity::Normal)
}

// ── Состояния движения ───────────────────────────────────────────────────────

#[derive(PartialEq, Clone, Copy)]
enum State {
    Walking,
    Idle,
    Dragged,
    Falling,
    Landing,
    ChaosChase,
    ChaosHold,
}

// ── Приложение ───────────────────────────────────────────────────────────────

struct MascotApp {
    pos: egui::Pos2,
    target: egui::Pos2,
    state: State,
    idle_timer: f32,
    vy: f32,
    facing_left: bool,
    walk_frame: f32,
    hovered: bool,
    rng: u64,
    screen: egui::Vec2,

    activity: Activity,
    app_timer: f32,
    work_timer: f32,
    anim_timer: f32,
    speech: Option<(String, f32)>,
    afk_timer: f32,
    afk_warned: bool,
    boredom_timer: f32,
    last_mouse: egui::Pos2,

    chaos_armed: bool,
    chaos_timer: f32,
    chaos_mode: bool,
    last_set_cursor: Option<egui::Pos2>,
    chaos_target_x: f32,
    chaos_vx: f32,
    chaos_catches: u32,
    chaos_forgiving: bool,

    sprites: HashMap<AnimKind, Vec<egui::TextureHandle>>,
    sprite_frame: usize,
    sprite_timer: f32,
    last_anim_kind: AnimKind,

    mood: Expression,
    mood_timer: f32,
    land_timer: f32,
}

impl MascotApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let rng = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let sprites = load_sprites(&cc.egui_ctx);
        Self {
            pos: egui::pos2(200.0, 810.0),
            target: egui::pos2(400.0, 810.0),
            state: State::Idle,
            idle_timer: 1.0,
            vy: 0.0,
            facing_left: false,
            walk_frame: 0.0,
            hovered: false,
            rng,
            screen: egui::vec2(1920.0, 1040.0),
            activity: Activity::Normal,
            app_timer: 0.0,
            work_timer: 0.0,
            anim_timer: 0.0,
            speech: None,
            afk_timer: 0.0,
            afk_warned: false,
            boredom_timer: 600.0,
            last_mouse: egui::Pos2::ZERO,
            chaos_armed: false,
            chaos_timer: CHAOS_GRACE_SECS,
            chaos_mode: false,
            last_set_cursor: None,
            chaos_target_x: 0.0,
            chaos_vx: 0.0,
            chaos_catches: 0,
            chaos_forgiving: false,
            sprites,
            sprite_frame: 0,
            sprite_timer: 0.0,
            last_anim_kind: AnimKind::Idle,
            mood: Expression::Bored,
            mood_timer: 0.0,
            land_timer: 0.0,
        }
    }

    fn rand(&mut self) -> f32 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.rng >> 11) as f32 / (1u64 << 53) as f32
    }

    fn ground_y(&self) -> f32 {
        self.screen.y - TASKBAR_H - WIN_H + FEET_PAD
    }

    fn pick_target(&mut self) {
        let max_x = (self.screen.x - WIN_W).max(100.0);
        self.target = egui::pos2(self.rand() * max_x, self.ground_y());
    }

    fn clamp_pos(&self, p: egui::Pos2) -> egui::Pos2 {
        egui::pos2(
            p.x.clamp(0.0, (self.screen.x - WIN_W).max(0.0)),
            self.ground_y(),
        )
    }

    fn clamp_chaos_x(&self, x: f32) -> f32 {
        x.clamp(0.0, (self.screen.x - WIN_W).max(0.0))
    }

    // точка где у персонажа "руки" — туда ловится/тащится курсор
    fn grab_point(&self) -> egui::Pos2 {
        self.pos + egui::vec2(WIN_W / 2.0, 35.0)
    }

    // может ли дотянуться до курсора: по X — близко, по Y — от чуть выше рук
    // и до самого низа (включая панель задач), т.е. умеет нагнуться за низким курсором
    fn can_grab(&self, cursor: egui::Pos2) -> bool {
        let gp = self.grab_point();
        let dy = cursor.y - gp.y;
        (cursor.x - gp.x).abs() < GRAB_RADIUS + 6.0 && dy > -GRAB_RADIUS && dy < 210.0
    }

    fn pick_chaos_target_x(&mut self) {
        let max_x = (self.screen.x - WIN_W).max(100.0);
        self.chaos_target_x = self.rand() * max_x;
    }

    fn current_anim_kind(&self) -> AnimKind {
        match self.state {
            State::Dragged    => AnimKind::Drag,
            State::Falling    => AnimKind::FallAnim,
            State::Landing    => AnimKind::FallAnim,
            State::ChaosChase => {
                if self.pos.y >= self.ground_y() - 0.5 { AnimKind::Angry } else { AnimKind::Grab }
            }
            State::ChaosHold  => AnimKind::Hold,
            State::Walking    => AnimKind::Walk,
            State::Idle       => match self.activity {
                Activity::Coding   => AnimKind::Coding,
                Activity::Watching => AnimKind::Watching,
                Activity::Music    => AnimKind::Dance,
                Activity::Normal   => AnimKind::Idle,
            },
        }
    }

    fn say(&mut self, text: &str, secs: f32) {
        self.speech = Some((text.to_string(), secs));
    }

    fn set_mood(&mut self, e: Expression, secs: f32) {
        self.mood = e;
        self.mood_timer = secs;
    }

    fn current_expr(&self) -> Expression {
        if self.mood_timer > 0.0 {
            return self.mood;
        }
        match self.state {
            State::ChaosChase | State::ChaosHold => Expression::Angry,
            State::Falling | State::Dragged => Expression::Wide,
            _ => match self.activity {
                Activity::Music => Expression::Happy,
                Activity::Watching => Expression::Wide,
                Activity::Coding => Expression::Focus,
                Activity::Normal => Expression::Bored,
            },
        }
    }

    fn choose<'a>(&mut self, phrases: &[&'a str]) -> &'a str {
        self.rand();
        phrases[self.rng as usize % phrases.len()]
    }
}

// ── Утилиты ──────────────────────────────────────────────────────────────────

// ── Рисование ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Expression {
    Bored,
    Happy,
    Sad,
    Angry,
    Wide,
    Focus,
}

fn draw_mascot(
    painter: &egui::Painter,
    center: egui::Pos2,
    state: State,
    activity: Activity,
    walk_frame: f32,
    facing_left: bool,
    anim: f32,
    vy: f32,
    on_ground: bool,
    expr: Expression,
    land: f32,
) {
    use egui::{pos2, Align2, Color32, FontId, Rect, Shape, Stroke};
    use Expression::*;

    let flip: f32 = if facing_left { -1.0 } else { 1.0 };
    let pi = std::f32::consts::PI;
    let v = egui::vec2;

    // ── палитра ───────────────────────────────────────────────────────────
    let skin = Color32::from_rgb(255, 226, 205);
    let hair = Color32::from_rgb(216, 218, 234);
    let hair2 = Color32::from_rgb(180, 184, 210);
    let hoodie = Color32::from_rgb(156, 142, 216);
    let hoodie2 = Color32::from_rgb(126, 114, 186);
    let shorts = Color32::from_rgb(74, 74, 98);
    let iris = Color32::from_rgb(132, 98, 210);
    let dark = Color32::from_rgb(42, 34, 62);
    let mouthc = Color32::from_rgb(198, 98, 118);
    let blush = Color32::from_rgba_unmultiplied(255, 150, 162, 120);

    // ── короткие помощники ────────────────────────────────────────────────
    let fill = |c: egui::Pos2, r: f32, col: Color32| painter.circle_filled(c, r, col);
    let limb = |a: egui::Pos2, b: egui::Pos2, w: f32, col: Color32| {
        painter.line_segment([a, b], Stroke::new(w, col));
    };
    let arc = |c: egui::Pos2, r: f32, a0: f32, a1: f32, w: f32, col: Color32| {
        let n = 14;
        let pts: Vec<egui::Pos2> = (0..=n)
            .map(|i| {
                let t = a0 + (a1 - a0) * i as f32 / n as f32;
                c + v(t.cos() * r, t.sin() * r)
            })
            .collect();
        painter.add(Shape::line(pts, Stroke::new(w, col)));
    };

    // ── туловище в худи ───────────────────────────────────────────────────
    let torso = |bc: egui::Pos2, lean: f32| {
        fill(bc, 23.0, hoodie);
        fill(bc + v(lean, -13.0), 15.0, hoodie);
        // нижняя кромка худи
        painter.add(Shape::convex_polygon(
            vec![
                bc + v(-22.0, 6.0),
                bc + v(22.0, 6.0),
                bc + v(18.0, 22.0),
                bc + v(-18.0, 22.0),
            ],
            hoodie2,
            Stroke::NONE,
        ));
    };

    // ── лицо с мимикой ────────────────────────────────────────────────────
    let draw_face = |hc: egui::Pos2, e: Expression, blink: f32, look: f32| {
        // волосы сзади
        fill(hc + v(0.0, -2.0), 27.5, hair2);
        fill(hc + v(0.0, -3.0), 26.0, hair);
        // кожа
        fill(hc, 24.5, skin);
        // боковые пряди
        limb(hc + v(-23.0, -6.0), hc + v(-22.0, 18.0), 9.0, hair);
        limb(hc + v(23.0, -6.0), hc + v(22.0, 18.0), 9.0, hair);
        // чёлка
        for &dx in &[-17.0_f32, -7.0, 3.0, 13.0] {
            fill(hc + v(dx, -15.0), 9.5, hair);
        }
        fill(hc + v(-2.0, -19.0), 15.0, hair);

        let ey = hc.y + 2.0;
        let lx = hc.x - 9.5;
        let rx = hc.x + 9.5;
        let lidded = e == Bored || e == Focus;

        if blink < 0.5 {
            // моргает — закрытые глаза
            for &x in &[lx, rx] {
                arc(pos2(x, ey - 1.0), 5.0, 0.15 * pi, 0.85 * pi, 2.0, dark);
            }
        } else {
            for &x in &[lx, rx] {
                fill(pos2(x, ey), 7.0, Color32::WHITE);
                fill(pos2(x + look, ey + 1.0), 5.2, iris);
                fill(pos2(x + look, ey + 1.0), 2.3, dark);
                fill(pos2(x + look - 1.7, ey - 1.9), 1.8, Color32::WHITE);
                // верхнее веко
                limb(pos2(x - 7.0, ey - 4.5), pos2(x + 7.0, ey - 3.5), 1.8, dark);
                if lidded {
                    // полуприкрытые (скучающие) глаза
                    fill(pos2(x, ey - 6.5), 7.0, skin);
                    limb(pos2(x - 7.0, ey - 1.0), pos2(x + 7.0, ey - 1.5), 2.0, dark);
                }
            }
        }

        // брови
        let by = ey - 11.5;
        match e {
            Angry => {
                limb(pos2(lx - 6.0, by - 1.0), pos2(lx + 5.0, by + 3.0), 2.2, hair2);
                limb(pos2(rx + 6.0, by - 1.0), pos2(rx - 5.0, by + 3.0), 2.2, hair2);
            }
            Sad => {
                limb(pos2(lx - 6.0, by + 3.0), pos2(lx + 5.0, by - 1.0), 2.2, hair2);
                limb(pos2(rx + 6.0, by + 3.0), pos2(rx - 5.0, by - 1.0), 2.2, hair2);
            }
            _ => {
                limb(pos2(lx - 5.0, by), pos2(lx + 5.0, by), 2.0, hair2);
                limb(pos2(rx - 5.0, by), pos2(rx + 5.0, by), 2.0, hair2);
            }
        }

        // румянец
        if matches!(e, Happy | Wide) {
            fill(pos2(lx - 4.0, ey + 7.5), 4.5, blush);
            fill(pos2(rx + 4.0, ey + 7.5), 4.5, blush);
        }

        // рот
        let mx = hc.x;
        let my = hc.y + 15.0;
        match e {
            Happy => arc(pos2(mx, my - 2.0), 6.0, 0.15 * pi, 0.85 * pi, 2.4, mouthc),
            Sad => arc(pos2(mx, my + 5.0), 6.0, -0.85 * pi, -0.15 * pi, 2.2, mouthc),
            Wide => {
                fill(pos2(mx, my + 1.0), 4.0, dark);
            }
            Angry => {
                fill(pos2(mx, my + 1.0), 3.2, mouthc);
            }
            _ => limb(pos2(mx - 3.0, my), pos2(mx + 4.0, my), 2.0, mouthc),
        }
    };

    let blink = if (anim * 0.9).sin() > 0.93 { 0.0 } else { 1.0 };

    // ── ПРЫЖОК / ПАДЕНИЕ / ПЕРЕТАСКИВАНИЕ ─────────────────────────────────
    if state == State::Dragged
        || state == State::Falling
        || ((state == State::ChaosChase || state == State::ChaosHold) && !on_ground)
    {
        let rising = vy < -40.0;
        let bc = center + v(0.0, 2.0);
        let hc = center + v(0.0, -50.0);
        if rising {
            limb(bc + v(-8.0, 16.0), bc + v(-14.0, 32.0), 7.0, shorts);
            limb(bc + v(8.0, 16.0), bc + v(14.0, 32.0), 7.0, shorts);
        } else {
            limb(bc + v(-7.0, 16.0), bc + v(-9.0, 42.0), 7.0, shorts);
            limb(bc + v(7.0, 16.0), bc + v(9.0, 42.0), 7.0, shorts);
        }
        // руки вверх
        limb(bc + v(-16.0, -6.0), hc + v(-8.0, -22.0), 6.0, hoodie);
        limb(bc + v(16.0, -6.0), hc + v(8.0, -22.0), 6.0, hoodie);
        fill(hc + v(-8.0, -22.0), 3.5, skin);
        fill(hc + v(8.0, -22.0), 3.5, skin);
        torso(bc, 0.0);
        let e = if state == State::Falling || state == State::Dragged || !rising {
            Wide
        } else {
            expr
        };
        draw_face(hc, e, 1.0, flip * 1.5);
        return;
    }

    // ── БЕГ / ПОГОНЯ ПО ЗЕМЛЕ ─────────────────────────────────────────────
    if state == State::ChaosChase || state == State::ChaosHold {
        let s = (walk_frame * pi).sin();
        let bob = s.abs() * 4.0;
        let lean = flip * 7.0;
        let bc = center + v(lean, 4.0 - bob);
        let hc = center + v(lean * 1.4, -50.0 - bob);
        let leg = s * 18.0;

        // линии скорости + тормозной след сзади
        for i in 0..3 {
            let yy = -18.0 + i as f32 * 16.0;
            painter.line_segment(
                [center + v(-flip * 30.0, yy), center + v(-flip * 56.0, yy)],
                Stroke::new(2.0, Color32::from_rgba_unmultiplied(205, 205, 225, 110)),
            );
        }
        fill(
            center + v(-flip * 18.0, 44.0),
            4.5,
            Color32::from_rgba_unmultiplied(210, 210, 222, 110),
        );

        limb(bc + v(-6.0, 16.0), bc + v(flip * (-6.0 + leg), 42.0), 7.0, shorts);
        limb(bc + v(6.0, 16.0), bc + v(flip * (6.0 - leg), 42.0), 7.0, shorts);

        if state == State::ChaosHold {
            // руки подняты к курсору над головой
            limb(bc + v(-16.0, -8.0), hc + v(-6.0, -26.0), 6.0, hoodie);
            limb(bc + v(16.0, -8.0), hc + v(6.0, -26.0), 6.0, hoodie);
            fill(hc + v(-6.0, -26.0), 3.5, skin);
            fill(hc + v(6.0, -26.0), 3.5, skin);
        } else {
            let arm = s * 14.0;
            limb(bc + v(-18.0, -6.0), bc + v(flip * (-20.0 - arm), 12.0), 6.0, hoodie);
            limb(bc + v(18.0, -6.0), bc + v(flip * (20.0 + arm), 12.0), 6.0, hoodie);
        }
        torso(bc, lean * 0.2);
        draw_face(hc, expr, 1.0, flip * 3.5);
        return;
    }

    // ── ПРИЗЕМЛЕНИЕ — присела, пружинит ───────────────────────────────────
    if state == State::Landing {
        // squash: 1.0 в момент удара → 0.0 к концу
        let squash = (land / 0.32).clamp(0.0, 1.0);
        let drop = squash * 14.0; // насколько присела
        let spread = squash * 6.0; // ноги в стороны
        let bc = center + v(0.0, 8.0 + drop);
        let hc = center + v(0.0, -44.0 + drop);
        // согнутые ноги
        limb(bc + v(-7.0, 14.0), bc + v(-12.0 - spread, 32.0), 7.0, shorts);
        limb(bc + v(7.0, 14.0), bc + v(12.0 + spread, 32.0), 7.0, shorts);
        fill(bc + v(-12.0 - spread, 33.0), 4.0, dark);
        fill(bc + v(12.0 + spread, 33.0), 4.0, dark);
        // руки в стороны для равновесия
        limb(bc + v(-18.0, -2.0), bc + v(-30.0 - spread, 6.0), 6.0, hoodie);
        limb(bc + v(18.0, -2.0), bc + v(30.0 + spread, 6.0), 6.0, hoodie);
        fill(bc + v(-30.0 - spread, 7.0), 3.5, skin);
        fill(bc + v(30.0 + spread, 7.0), 3.5, skin);
        torso(bc, 0.0);
        draw_face(hc, if squash > 0.5 { Wide } else { expr }, 1.0, flip * 1.5);
        // пыль при ударе
        if squash > 0.6 {
            let a = ((squash - 0.6) / 0.4 * 120.0) as u8;
            let dust = Color32::from_rgba_unmultiplied(210, 210, 222, a);
            fill(center + v(-20.0, 46.0), 5.0, dust);
            fill(center + v(20.0, 46.0), 5.0, dust);
            fill(center + v(0.0, 48.0), 4.0, dust);
        }
        return;
    }

    match activity {
        Activity::Normal => {
            if state == State::Walking {
                // ── ХОДЬБА ────────────────────────────────────────────────
                let s = (walk_frame * pi).sin();
                let bob = s.abs() * 3.0;
                let bc = center + v(0.0, 6.0 - bob);
                let hc = center + v(0.0, -48.0 - bob);
                let leg = s * 10.0;
                let arm = s * 8.0;
                limb(bc + v(-7.0, 16.0), bc + v(flip * (-7.0 + leg), 40.0), 7.0, shorts);
                limb(bc + v(7.0, 16.0), bc + v(flip * (7.0 - leg), 40.0), 7.0, shorts);
                limb(bc + v(-19.0, -4.0), bc + v(flip * (-17.0 - arm), 18.0), 6.0, hoodie);
                limb(bc + v(19.0, -4.0), bc + v(flip * (17.0 + arm), 18.0), 6.0, hoodie);
                torso(bc, 0.0);
                draw_face(hc, expr, blink, flip * 2.5);
            } else {
                // ── IDLE — дышит, изредка моргает ─────────────────────────
                let breath = (anim * 1.6).sin();
                let bob = breath * 2.0;
                let sway = (anim * 0.7).sin() * 1.5;
                let bc = center + v(sway, 6.0 + bob * 0.5);
                let hc = center + v(sway, -48.0 + bob);
                limb(bc + v(-8.0, 16.0), bc + v(-8.0, 40.0), 7.0, shorts);
                limb(bc + v(8.0, 16.0), bc + v(8.0, 40.0), 7.0, shorts);
                fill(bc + v(-8.0, 41.0), 4.0, dark);
                fill(bc + v(8.0, 41.0), 4.0, dark);
                limb(bc + v(-20.0, -4.0), bc + v(-19.0, 18.0), 6.0, hoodie);
                limb(bc + v(20.0, -4.0), bc + v(19.0, 18.0), 6.0, hoodie);
                fill(bc + v(-19.0, 19.0), 3.5, skin);
                fill(bc + v(19.0, 19.0), 3.5, skin);
                torso(bc, 0.0);
                draw_face(hc, expr, blink, flip * 1.5);
            }
        }

        // ── КОДИТ — сидит, очки, ноутбук, печатает ────────────────────────
        Activity::Coding => {
            let tb = (anim * 8.0).sin() * 1.2;
            let bc = center + v(0.0, 10.0);
            let hc = center + v(flip * 2.0, -42.0 + tb);
            limb(bc + v(-8.0, 16.0), bc + v(-24.0, 24.0), 7.0, shorts);
            limb(bc + v(8.0, 16.0), bc + v(24.0, 24.0), 7.0, shorts);
            fill(bc + v(-25.0, 25.0), 4.0, dark);
            fill(bc + v(25.0, 25.0), 4.0, dark);
            torso(bc, 0.0);
            // ноутбук
            painter.rect_filled(
                Rect::from_center_size(center + v(0.0, 31.0), v(46.0, 7.0)),
                2.0,
                Color32::from_rgb(72, 72, 88),
            );
            painter.rect_filled(
                Rect::from_center_size(center + v(0.0, 18.0), v(42.0, 24.0)),
                2.0,
                Color32::from_rgb(60, 60, 80),
            );
            painter.rect_filled(
                Rect::from_center_size(center + v(0.0, 18.0), v(35.0, 17.0)),
                1.0,
                Color32::from_rgb(120, 190, 255),
            );
            // руки на клавиатуре
            limb(bc + v(-16.0, 2.0), center + v(-12.0, 28.0 + tb), 6.0, hoodie);
            limb(bc + v(16.0, 2.0), center + v(12.0, 28.0 - tb), 6.0, hoodie);
            draw_face(hc, Focus, blink, flip * 1.0);
            // очки
            let ey = hc.y + 2.0;
            let lx = hc.x - 9.5;
            let rx = hc.x + 9.5;
            painter.circle_stroke(pos2(lx, ey), 7.5, Stroke::new(1.8, dark));
            painter.circle_stroke(pos2(rx, ey), 7.5, Stroke::new(1.8, dark));
            limb(pos2(lx + 7.0, ey), pos2(rx - 7.0, ey), 1.8, dark);
        }

        // ── СМОТРИТ — сидит с попкорном ───────────────────────────────────
        Activity::Watching => {
            let munch = (anim * 3.0).sin() * 2.0;
            let bc = center + v(0.0, 10.0);
            let hc = center + v(0.0, -44.0);
            limb(bc + v(-8.0, 16.0), bc + v(-24.0, 24.0), 7.0, shorts);
            limb(bc + v(8.0, 16.0), bc + v(24.0, 24.0), 7.0, shorts);
            fill(bc + v(-25.0, 25.0), 4.0, dark);
            fill(bc + v(25.0, 25.0), 4.0, dark);
            torso(bc, 0.0);
            // рука к рту (ест)
            limb(bc + v(16.0, 0.0), hc + v(11.0, 16.0 + munch), 6.0, hoodie);
            fill(hc + v(11.0, 16.0 + munch), 3.5, skin);
            // рука у ведёрка
            limb(bc + v(-16.0, 2.0), center + v(-6.0, 24.0), 6.0, hoodie);
            // ведёрко попкорна
            let bk = center + v(0.0, 30.0);
            painter.add(Shape::convex_polygon(
                vec![
                    bk + v(-12.0, -8.0),
                    bk + v(12.0, -8.0),
                    bk + v(9.0, 11.0),
                    bk + v(-9.0, 11.0),
                ],
                Color32::WHITE,
                Stroke::new(1.0, Color32::from_rgb(210, 70, 70)),
            ));
            for &sx in &[-8.0_f32, -2.0, 4.0, 10.0] {
                limb(bk + v(sx, -8.0), bk + v(sx * 0.78, 11.0), 2.4, Color32::from_rgb(222, 74, 74));
            }
            for &(px, py) in &[(-7.0, -9.0), (-2.0, -11.0), (3.0, -9.0), (8.0, -10.0), (0.0, -7.0)] {
                fill(bk + v(px, py), 3.2, Color32::from_rgb(250, 236, 172));
            }
            draw_face(hc, Wide, 1.0, 0.0);
        }

        // ── ТАНЦУЕТ ───────────────────────────────────────────────────────
        Activity::Music => {
            let beat = (anim * 4.0 * pi).sin();
            let bob = beat.abs() * 5.0;
            let hip = (anim * 2.0 * pi).sin() * 5.0;
            let bc = center + v(hip, 6.0 - bob);
            let hc = center + v(hip * 0.8, -48.0 - bob);
            let lk = beat * 6.0;
            limb(bc + v(-7.0, 16.0), bc + v(-9.0 + lk, 40.0), 7.0, shorts);
            limb(bc + v(7.0, 16.0), bc + v(9.0 + lk, 40.0), 7.0, shorts);
            let up = 22.0 * beat;
            limb(bc + v(-18.0, -4.0), bc + v(-34.0, -20.0 + up), 6.0, hoodie);
            limb(bc + v(18.0, -4.0), bc + v(34.0, -20.0 - up), 6.0, hoodie);
            fill(bc + v(-34.0, -20.0 + up), 3.5, skin);
            fill(bc + v(34.0, -20.0 - up), 3.5, skin);
            torso(bc, hip * 0.2);
            draw_face(hc, Happy, 1.0, hip * 0.3);
            if beat > 0.3 {
                painter.text(
                    center + v(40.0, -50.0 - bob),
                    Align2::CENTER_CENTER,
                    "♪",
                    FontId::proportional(15.0),
                    Color32::from_rgb(210, 160, 255),
                );
            }
            if beat < -0.3 {
                painter.text(
                    center + v(-40.0, -44.0),
                    Align2::CENTER_CENTER,
                    "♫",
                    FontId::proportional(13.0),
                    Color32::from_rgb(190, 150, 240),
                );
            }
        }
    }
}

fn draw_speech(painter: &egui::Painter, center: egui::Pos2, text: &str) {
    let font = egui::FontId::proportional(12.0);
    let max_w = WIN_W - 16.0; // не вылезаем за края окна
    let pad = egui::vec2(10.0, 7.0);

    // разбиваем длинный текст на строки по 18 символов
    let lines: Vec<&str> = if text.len() <= 20 {
        vec![text]
    } else {
        text.split('\n').collect()
    };

    let line_h = 15.0;
    let bubble_h = line_h * lines.len() as f32 + pad.y * 2.0;
    let bubble_w = lines
        .iter()
        .map(|l| l.chars().count() as f32 * 7.0 + pad.x * 2.0)
        .fold(60.0f32, f32::max)
        .min(max_w);

    // пузырёк над головой — внутри окна (зафиксирован вверху)
    let pos = egui::pos2(center.x, bubble_h / 2.0 + 4.0);
    let rect = egui::Rect::from_center_size(pos, egui::vec2(bubble_w, bubble_h));

    painter.rect_filled(
        rect,
        7.0,
        egui::Color32::from_rgba_unmultiplied(25, 25, 35, 235),
    );
    painter.rect_stroke(
        rect,
        7.0,
        egui::Stroke::new(1.5, egui::Color32::from_rgb(180, 140, 210)),
    );

    // хвостик вниз (к персонажу)
    let tip = egui::pos2(center.x, rect.max.y + 8.0);
    painter.add(egui::Shape::convex_polygon(
        vec![
            tip,
            tip + egui::vec2(-7.0, -10.0),
            tip + egui::vec2(7.0, -10.0),
        ],
        egui::Color32::from_rgba_unmultiplied(25, 25, 35, 235),
        egui::Stroke::NONE,
    ));

    // текст
    for (i, line) in lines.iter().enumerate() {
        let y = rect.min.y + pad.y + line_h * i as f32 + line_h * 0.5;
        painter.text(
            egui::pos2(center.x, y),
            egui::Align2::CENTER_CENTER,
            line,
            font.clone(),
            egui::Color32::WHITE,
        );
    }
}

// ── App impl ──────────────────────────────────────────────────────────────────

impl eframe::App for MascotApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dt = ctx.input(|i| i.unstable_dt).min(0.05);

        if self.mood_timer > 0.0 {
            self.mood_timer -= dt;
        }

        // размер экрана
        if let Some(sz) = ctx.input(|i| i.viewport().monitor_size) {
            if sz.x > 100.0 {
                self.screen = sz;
            }
        }

        // синхронизация позиции при перетаскивании
        if self.state == State::Dragged {
            if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
                self.pos = r.min;
            }
        }

        // опрос активного окна каждые 2 секунды
        self.app_timer -= dt;
        if self.app_timer <= 0.0 {
            self.app_timer = 2.0;
            let (proc, title) = foreground_info();
            if let Some(a) = classify(&proc, &title) {
                if a != self.activity {
                    // смена активности — говорим что-то
                    match a {
                        Activity::Coding => {
                            let p = self.choose(&[
                                "Ладно. Работай.",
                                "Опять за это...",
                                "Посмотрим сколько\nпродержишься.",
                                "Молча наблюдаю.",
                            ]);
                            self.say(p, 4.0);
                        }
                        Activity::Watching => {
                            let p = self.choose(&[
                                "YouTube. Конечно.",
                                "Перерыв?\nЧасовой, небось.",
                                "Ладно, и я смотрю.",
                            ]);
                            self.say(p, 4.0);
                        }
                        Activity::Music => {
                            let p = self.choose(&["О. Музыка.", "Неплохо.", "Наконец-то."]);
                            self.say(p, 3.0);
                        }
                        Activity::Normal => {
                            let p = self.choose(&["И куда теперь?", "Хм.", "Ладно."]);
                            self.say(p, 3.0);
                        }
                    }
                    self.activity = a;
                }
            }
        }

        // таймер работы + chaos
        self.anim_timer += dt;
        if self.activity == Activity::Coding {
            self.work_timer += dt;

            // первое предупреждение — взводим таймер хаоса
            if self.work_timer >= WORK_WARN_SECS && !self.chaos_armed {
                self.say("2 часа уже. Встань.\nИли пожалеешь.", 8.0);
                self.chaos_armed = true;
                self.chaos_timer = CHAOS_GRACE_SECS;
                self.work_timer = 0.0;
            }

            // отсчёт до хаоса
            if self.chaos_armed {
                self.chaos_timer -= dt;
                if self.chaos_timer <= 60.0 && self.chaos_timer > 59.0 {
                    self.say("Последнее предупреждение.", 5.0);
                }
                if self.chaos_timer <= 0.0 && !self.chaos_mode {
                    self.chaos_mode = true;
                    self.chaos_catches = 0;
                    self.chaos_forgiving = false;
                    self.say("Сама предупреждала.", 4.0);
                }
            }
        } else {
            // взял перерыв — снимаем хаос
            if self.chaos_mode {
                let p = self.choose(&["Наконец-то.", "Молодец.", "Давно бы так."]);
                self.say(p, 4.0);
            }
            self.work_timer = 0.0;
            self.chaos_armed = false;
            self.chaos_mode = false;
            self.chaos_timer = CHAOS_GRACE_SECS;
        }

        // хаос — запускаем chase если ещё не в хаос-состоянии
        if self.chaos_mode
            && self.state != State::ChaosChase
            && self.state != State::ChaosHold
            && self.state != State::Dragged
        {
            self.state = State::ChaosChase;
            self.vy = 0.0;
        }
        // хаос отключён — возвращаемся на землю
        if !self.chaos_mode && (self.state == State::ChaosChase || self.state == State::ChaosHold) {
            self.pos.y = self.ground_y();
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.pos));
            self.state = State::Idle;
            self.idle_timer = 1.0;
            self.vy = 0.0;
            self.last_set_cursor = None;
        }

        // AFK — мышь не двигается (глобальная позиция курсора, а не только над окном)
        let mouse = get_cursor_screen_pos();
        if (mouse - self.last_mouse).length() > 5.0 {
            // вернулся после долгого AFK
            if self.afk_warned {
                let p = self.choose(&["О. Вернулся.", "Живой?", "Наконец-то."]);
                self.say(p, 3.0);
            }
            self.afk_timer = 0.0;
            self.afk_warned = false;
            self.last_mouse = mouse;
        } else {
            self.afk_timer += dt;
        }

        // AFK — одна фраза когда пересекаем порог
        if !self.afk_warned && self.speech.is_none() {
            let threshold = if self.activity == Activity::Coding {
                120.0
            } else {
                300.0
            };
            if self.afk_timer >= threshold {
                let phrase = match self.activity {
                    Activity::Coding => self.choose(&[
                        "Эй. Ты живой?",
                        "Ctrl+S хотя бы нажми.",
                        "Заснул с открытым IDE.\nКлассика.",
                    ]),
                    Activity::Watching => {
                        self.choose(&["Ты там уснул?", "Ты уже час смотришь.\nЭто рекорд."])
                    }
                    _ => self.choose(&["Куда ушёл?", "Ладно. Жду.", "Тихо стало."]),
                };
                self.say(phrase, 5.0);
                self.set_mood(Expression::Sad, 5.0);
                self.afk_warned = true;
            }
        }

        // фоллбэк — если давно ничего не происходило (8-12 минут)
        if self.speech.is_none() {
            self.boredom_timer -= dt;
            if self.boredom_timer <= 0.0 {
                let hour = (SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    % 86400)
                    / 3600;

                let phrase: &str = match self.activity {
                    Activity::Coding => self.choose(&[
                        "Уверен что это\nскомпилируется?",
                        "Опять тот же баг?",
                        "Хм. Интересное решение.\nНеправильное, но интересное.",
                        "Комментарии писать\nне модно, да.",
                        "Я не сплю, я наблюдаю.",
                    ]),
                    Activity::Watching => self.choose(&[
                        "Ещё один ролик и всё, да?",
                        "YouTube это не\n'небольшой перерыв'.",
                        "Продуктивно.",
                    ]),
                    Activity::Music => self.choose(&[
                        "Эта песня снова.",
                        "Неплохо. Хотя я бы\nвыбрала другое.",
                        "Хороший вкус. Почти.",
                    ]),
                    Activity::Normal if hour < 5 || hour >= 23 => self.choose(&[
                        "Нормальные люди спят.",
                        "Поздно уже.",
                        "Только мы двое\nне спим. Грустно.",
                    ]),
                    Activity::Normal if hour < 9 => {
                        self.choose(&["Кофе хоть выпил?", "Доброе утро.\nХотя не факт."])
                    }
                    Activity::Normal => self.choose(&[
                        "Скучно.",
                        "Иди уже работай.",
                        "Я жду.",
                        "Хоть бы музыку\nвключил.",
                    ]),
                };
                self.say(phrase, 5.0);
                self.boredom_timer = 480.0 + self.rand() * 240.0;
            }
        }

        // пузырёк
        if let Some((_, ref mut t)) = self.speech {
            *t -= dt;
        }
        if self
            .speech
            .as_ref()
            .map(|(_, t)| *t <= 0.0)
            .unwrap_or(false)
        {
            self.speech = None;
        }

        // движение (только не при перетаскивании и только в Normal)
        if self.state != State::Dragged {
            match self.state {
                State::Walking if self.activity == Activity::Normal => {
                    let speed = 80.0;
                    let dir = self.target - self.pos;
                    if dir.length() < 4.0 {
                        self.state = State::Idle;
                        self.idle_timer = 1.5 + self.rand() * 2.5;
                        self.walk_frame = 0.0;
                    } else {
                        let step = dir.normalized() * speed * dt;
                        self.pos += step;
                        self.pos = self.clamp_pos(self.pos);
                        self.facing_left = dir.x < 0.0;
                        self.walk_frame += dt * 6.0;
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.pos));
                    }
                }
                // если не Normal — останавливаемся на месте
                State::Walking => {
                    self.state = State::Idle;
                    self.idle_timer = 999.0; // ждём пока activity не вернётся в Normal
                }
                State::Idle => {
                    if self.activity == Activity::Normal {
                        self.idle_timer -= dt;
                        if self.idle_timer <= 0.0 {
                            self.pick_target();
                            self.state = State::Walking;
                        }
                    }
                }
                State::Falling => {
                    self.vy += GRAVITY * dt;
                    self.pos.y += self.vy * dt;
                    let g = self.ground_y();
                    if self.pos.y >= g {
                        self.pos.y = g;
                        self.vy = 0.0;
                        self.target = self.pos;
                        // короткая поза приземления перед обычными анимациями
                        self.state = State::Landing;
                        self.land_timer = 0.32;
                        if self.speech.is_none() {
                            let p =
                                self.choose(&["Ладно.", "Поставил.", "И зачем это было?", "Уф."]);
                            self.say(p, 2.0);
                        }
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.pos));
                }
                State::Landing => {
                    self.pos.y = self.ground_y();
                    self.land_timer -= dt;
                    if self.land_timer <= 0.0 {
                        self.state = State::Idle;
                        self.idle_timer = 0.6;
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.pos));
                }
                // ── Хаос: бежим по земле и прыгаем за курсором ───────────
                State::ChaosChase => {
                    let ground = self.ground_y();
                    let cursor = get_cursor_screen_pos();
                    let on_ground = self.pos.y >= ground - 0.5;

                    if on_ground {
                        // на земле — управляем бегом под курсор (только X)
                        let want_x = self.clamp_chaos_x(cursor.x - WIN_W / 2.0);
                        let dx = want_x - self.pos.x;
                        let run = 360.0 * dt;
                        if dx.abs() > 2.0 {
                            self.pos.x += if run > dx.abs() {
                                dx
                            } else {
                                dx.signum() * run
                            };
                            self.facing_left = dx < 0.0;
                        }
                        // прыжок: только если курсор заметно ВЫШЕ рук.
                        // низкий курсор (на панели задач) не требует прыжка —
                        // его поймает дотягивание вниз (can_grab) ниже по коду.
                        if dx.abs() < 70.0 && cursor.y < self.grab_point().y - 40.0 {
                            self.vy = -JUMP_V;
                            // баллистический толчок к курсору по горизонтали
                            self.chaos_vx =
                                ((cursor.x - self.grab_point().x) * 1.2).clamp(-260.0, 260.0);
                        }
                    } else {
                        // в воздухе — баллистика, рулить нельзя
                        self.pos.x += self.chaos_vx * dt;
                    }
                    self.walk_frame += dt * 16.0;

                    // гравитация
                    self.vy += GRAVITY * dt;
                    self.pos.y += self.vy * dt;
                    if self.pos.y > ground {
                        self.pos.y = ground;
                        self.vy = 0.0;
                        self.chaos_vx = 0.0;
                    }
                    self.pos.x = self.clamp_chaos_x(self.pos.x);

                    // поймал? (vy сохраняем — продолжит падать с курсором)
                    if self.can_grab(cursor) {
                        self.chaos_catches += 1;
                        self.state = State::ChaosHold;
                        self.last_set_cursor = None;
                        self.pick_chaos_target_x();
                        if self.chaos_catches >= 2 {
                            // поймала второй раз: держит курсор, приземлится — и простит
                            self.chaos_forgiving = true;
                            let p = self.choose(&["Опять ты.", "Снова попался.", "Ну всё."]);
                            self.say(p, 2.0);
                        } else {
                            let p = self.choose(&["Попался.", "Мой.", "Ха."]);
                            self.say(p, 2.0);
                        }
                    }

                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.pos));
                }

                // ── Хаос: держим курсор руками ───────────────────────────
                State::ChaosHold => {
                    let ground = self.ground_y();
                    let on_ground = self.pos.y >= ground - 0.5;

                    // вырывание игнорируем когда прощаем (она уже не отпустит)
                    if !self.chaos_forgiving {
                        let actual = get_cursor_screen_pos();
                        if let Some(last) = self.last_set_cursor {
                            if (actual - last).length() > FIGHT_THRESHOLD {
                                self.state = State::ChaosChase;
                                self.last_set_cursor = None;
                                let p = self.choose(&["Э! Вернись!", "Куда?!", "Не уйдёшь."]);
                                self.say(p, 2.0);
                            }
                        }
                    }

                    // прощение: приземлилась с курсором → говорит и отпускает
                    if self.chaos_forgiving && on_ground {
                        self.pos.y = ground;
                        self.vy = 0.0;
                        self.chaos_vx = 0.0;
                        self.last_set_cursor = None; // отпускаем курсор
                        self.chaos_mode = false;
                        self.chaos_armed = false;
                        self.chaos_forgiving = false;
                        self.chaos_timer = CHAOS_GRACE_SECS;
                        self.work_timer = 0.0;
                        self.state = State::Idle;
                        self.idle_timer = 1.0;
                        let p = self.choose(&[
                            "Ладно. Прощаю.",
                            "В этот раз прощаю.",
                            "Так и быть. Иди.",
                        ]);
                        self.say(p, 4.0);
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.pos));
                    } else if self.state == State::ChaosHold {
                        if on_ground && !self.chaos_forgiving {
                            // на земле — бегаем к случайной точке
                            let dx = self.chaos_target_x - self.pos.x;
                            if dx.abs() < 8.0 {
                                self.pick_chaos_target_x();
                            } else {
                                let run = 300.0 * dt;
                                self.pos.x += if run > dx.abs() {
                                    dx
                                } else {
                                    dx.signum() * run
                                };
                                self.facing_left = dx < 0.0;
                            }
                        } else if !on_ground {
                            // ещё падаем после поимки — баллистика
                            self.pos.x += self.chaos_vx * dt;
                        }
                        self.walk_frame += dt * 16.0;

                        // гравитация (падаем с курсором в руках)
                        self.vy += GRAVITY * dt;
                        self.pos.y += self.vy * dt;
                        if self.pos.y > ground {
                            self.pos.y = ground;
                            self.vy = 0.0;
                            self.chaos_vx = 0.0;
                        }
                        self.pos.x = self.clamp_chaos_x(self.pos.x);

                        // тащим курсор за руками
                        let hand = self.grab_point();
                        set_cursor_screen_pos(hand.x, hand.y);
                        self.last_set_cursor = Some(hand);

                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.pos));
                    }
                }
                _ => {}
            }
        }

        // прокручиваем кадр спрайта
        {
            let kind = self.current_anim_kind();
            if kind != self.last_anim_kind {
                self.sprite_frame = 0;
                self.sprite_timer = 0.0;
                self.last_anim_kind = kind;
            }
            self.sprite_timer += dt;
            let frame_dur = 1.0 / kind.fps();
            if self.sprite_timer >= frame_dur {
                self.sprite_timer -= frame_dur;
                let len = self.sprites.get(&kind).map(|f| f.len()).unwrap_or(1).max(1);
                self.sprite_frame = (self.sprite_frame + 1) % len;
            }
        }

        // рисуем
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let response = ui.allocate_rect(ui.max_rect(), egui::Sense::click_and_drag());

                if response.drag_started() {
                    self.state = State::Dragged;
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    self.set_mood(Expression::Angry, 2.0);
                    let p = self.choose(&["Эй!", "Куда?", "Руки.", "Хватит."]);
                    self.say(p, 2.0);
                }
                if response.clicked() {
                    self.set_mood(Expression::Happy, 2.0);
                    let p =
                        self.choose(&["Не тронь.", "Что надо?", "Стоп.", "Я вижу тебя.", "Зачем?"]);
                    self.say(p, 3.0);
                }
                if response.drag_stopped() {
                    // оставляем персонажа там, где отпустили — гравитация уронит его сама
                    if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
                        self.pos = egui::pos2(
                            r.min.x.clamp(0.0, (self.screen.x - WIN_W).max(0.0)),
                            r.min.y,
                        );
                        self.target = self.pos;
                    }
                    self.vy = 0.0;
                    self.state = State::Falling;
                }
                if response.secondary_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                self.hovered = response.hovered();

                let center = ui.max_rect().center();
                let painter = ui.painter();
                let on_ground = self.pos.y >= self.ground_y() - 0.5;

                let kind = self.current_anim_kind();
                let texture = self.sprites.get(&kind)
                    .or_else(|| self.sprites.get(&AnimKind::Idle))
                    .and_then(|frames| {
                        let idx = self.sprite_frame % frames.len().max(1);
                        frames.get(idx)
                    });

                if let Some(tex) = texture {
                    let rect = ui.max_rect();
                    let uv = if self.facing_left {
                        egui::Rect::from_min_max(egui::pos2(1.0, 0.0), egui::pos2(0.0, 1.0))
                    } else {
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
                    };
                    painter.image(tex.id(), rect, uv, egui::Color32::WHITE);
                } else {
                    draw_mascot(
                        painter,
                        center,
                        self.state,
                        self.activity,
                        self.walk_frame,
                        self.facing_left,
                        self.anim_timer,
                        self.vy,
                        on_ground,
                        self.current_expr(),
                        self.land_timer,
                    );
                }

                if let Some((ref text, _)) = self.speech {
                    draw_speech(painter, center, text);
                }

                if self.hovered {
                    let hp = center + egui::vec2(0.0, 72.0);
                    painter.rect_filled(
                        egui::Rect::from_center_size(hp, egui::vec2(90.0, 18.0)),
                        4.0,
                        egui::Color32::from_rgba_unmultiplied(30, 30, 30, 200),
                    );
                    painter.text(
                        hp,
                        egui::Align2::CENTER_CENTER,
                        "ПКМ = выход",
                        egui::FontId::proportional(11.0),
                        egui::Color32::WHITE,
                    );
                }
            });

        // адаптивная частота кадров: быстрые состояния — плавно, спокойные — экономно
        let fast = matches!(
            self.state,
            State::Walking
                | State::Falling
                | State::Landing
                | State::Dragged
                | State::ChaosChase
                | State::ChaosHold
        ) || self.activity == Activity::Music;
        let interval = if fast { 1.0 / 60.0 } else { 1.0 / 30.0 };
        ctx.request_repaint_after(std::time::Duration::from_secs_f32(interval));
    }
}
