#![windows_subsystem = "windows"]

use eframe::egui;
use std::time::{SystemTime, UNIX_EPOCH};

const WIN_W: f32 = 160.0;
const WIN_H: f32 = 220.0;
const TASKBAR_H: f32 = 50.0;
const FEET_PAD: f32 = 40.0;
const GRAVITY: f32 = 600.0;
const WORK_WARN_SECS: f32 = 2.0 * 3600.0; // предупреждение через 2 часа кодинга
const CHAOS_GRACE_SECS: f32 = 300.0;      // 5 минут после предупреждения до хаоса
const JUMP_V: f32 = 720.0;          // сила прыжка за курсором
const GRAB_RADIUS: f32 = 50.0;      // на каком расстоянии руки ловят курсор
const FIGHT_THRESHOLD: f32 = 45.0;  // насколько дёрнуть курсор чтобы вырвать

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
fn get_cursor_screen_pos() -> egui::Pos2 { egui::Pos2::ZERO }

#[cfg(windows)]
fn set_cursor_screen_pos(x: f32, y: f32) {
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
    unsafe { let _ = SetCursorPos(x as i32, y as i32); }
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
            .with_position([200.0, 780.0]),
        ..Default::default()
    };
    eframe::run_native("mascot", options, Box::new(|_cc| Ok(Box::new(MascotApp::new()))))
}

// ── Определяем активное окно (только Windows) ──────────────────────────────

#[cfg(windows)]
fn foreground_info() -> (String, String) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
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
fn foreground_info() -> (String, String) { (String::new(), String::new()) }

// ── Активность ──────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone, Copy)]
enum Activity { Normal, Coding, Watching, Music }

fn classify(process: &str, title: &str) -> Option<Activity> {
    if process.is_empty() || process == "ai_agent.exe" { return None; }

    // Claude Code desktop (Electron) — любой процесс с "claude" в имени
    if process.contains("claude") { return Some(Activity::Coding); }

    // IDE и редакторы
    if process.contains("webstorm") || process.contains("rider")
        || process.contains("clion")  || process.contains("idea")
        || process == "code.exe"      || process == "zed.exe"
        || process == "devenv.exe"    || process == "fleet.exe"
        || process == "cursor.exe"    || process == "notepad.exe"
        || process == "notepad++.exe" || process == "sublime_text.exe"
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

    if process == "spotify.exe" { return Some(Activity::Music); }

    if (process == "chrome.exe" || process == "firefox.exe" || process == "msedge.exe")
        && title.contains("youtube")
    {
        return Some(Activity::Watching);
    }

    Some(Activity::Normal)
}

// ── Состояния движения ───────────────────────────────────────────────────────

#[derive(PartialEq, Clone, Copy)]
enum State { Walking, Idle, Dragged, Falling, ChaosChase, ChaosHold }

// ── Приложение ───────────────────────────────────────────────────────────────

struct MascotApp {
    pos:          egui::Pos2,
    target:       egui::Pos2,
    state:        State,
    idle_timer:   f32,
    vy:           f32,
    facing_left:  bool,
    walk_frame:   f32,
    hovered:      bool,
    rng:          u64,
    screen:       egui::Vec2,

    activity:        Activity,
    app_timer:       f32,
    work_timer:      f32,
    anim_timer:      f32,
    speech:          Option<(String, f32)>,
    afk_timer:       f32,
    afk_warned:      bool,
    boredom_timer:   f32,
    last_mouse:      egui::Pos2,

    chaos_armed:     bool,
    chaos_timer:     f32,
    chaos_mode:      bool,
    last_set_cursor: Option<egui::Pos2>, // куда мы поставили курсор в прошлом кадре
    chaos_target_x:  f32,                // куда бежим, когда держим курсор
    chaos_vx:        f32,                // горизонтальная скорость в прыжке (баллистика)
    chaos_catches:   u32,                // сколько раз поймала курсор
    chaos_forgiving: bool,               // поймала 2й раз: держит, приземлится и простит
}

impl MascotApp {
    fn new() -> Self {
        let rng = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
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
        }
    }

    fn rand(&mut self) -> f32 {
        self.rng = self.rng
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
        egui::pos2(p.x.clamp(0.0, (self.screen.x - WIN_W).max(0.0)), self.ground_y())
    }

    fn clamp_chaos_x(&self, x: f32) -> f32 {
        x.clamp(0.0, (self.screen.x - WIN_W).max(0.0))
    }

    // точка где у персонажа "руки" — туда ловится/тащится курсор
    fn grab_point(&self) -> egui::Pos2 {
        self.pos + egui::vec2(WIN_W / 2.0, 35.0)
    }

    fn pick_chaos_target_x(&mut self) {
        let max_x = (self.screen.x - WIN_W).max(100.0);
        self.chaos_target_x = self.rand() * max_x;
    }

    fn say(&mut self, text: &str, secs: f32) {
        self.speech = Some((text.to_string(), secs));
    }

    fn choose<'a>(&mut self, phrases: &[&'a str]) -> &'a str {
        self.rand();
        phrases[self.rng as usize % phrases.len()]
    }
}

// ── Утилиты ──────────────────────────────────────────────────────────────────

// ── Рисование ────────────────────────────────────────────────────────────────

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
) {
    let flip: f32 = if facing_left { -1.0 } else { 1.0 };
    let pi = std::f32::consts::PI;

    let skin  = egui::Color32::from_rgb(255, 220, 200);
    let hair  = egui::Color32::from_rgb(200, 200, 220);
    let body  = egui::Color32::from_rgb(180, 140, 210);
    let eye   = egui::Color32::from_rgb(70, 50, 110);

    // ── Хаос: бег / прыжок / потягивание / падение ────────────────────────
    if state == State::ChaosChase || state == State::ChaosHold {
        let ex = flip * 4.0;

        // вспомогательная отрисовка головы + решительного лица
        let draw_head = |dy: f32| {
            painter.circle_filled(center + egui::vec2(flip * 4.0, -50.0 + dy), 24.0, skin);
            painter.circle_filled(center + egui::vec2(flip * 4.0, -57.0 + dy), 21.0, hair);
            painter.circle_filled(center + egui::vec2(-6.0 + ex, -50.0 + dy), 3.5, eye);
            painter.circle_filled(center + egui::vec2( 6.0 + ex, -50.0 + dy), 3.5, eye);
            painter.line_segment(
                [center + egui::vec2(-11.0 + ex, -57.0 + dy), center + egui::vec2(-2.0 + ex, -54.0 + dy)],
                egui::Stroke::new(2.0, eye),
            );
            painter.line_segment(
                [center + egui::vec2(11.0 + ex, -57.0 + dy), center + egui::vec2(2.0 + ex, -54.0 + dy)],
                egui::Stroke::new(2.0, eye),
            );
        };

        if on_ground {
            // ── БЕГ ──────────────────────────────────────────────────────
            let s = (walk_frame * pi).sin();
            let bob = s.abs() * 4.0;
            let leg = s * 16.0;
            painter.line_segment(
                [center + egui::vec2(-5.0, 20.0 - bob), center + egui::vec2(flip * (-5.0 + leg), 52.0 - bob)],
                egui::Stroke::new(5.0, body),
            );
            painter.line_segment(
                [center + egui::vec2(5.0, 20.0 - bob), center + egui::vec2(flip * (5.0 - leg), 52.0 - bob)],
                egui::Stroke::new(5.0, body),
            );
            painter.circle_filled(center + egui::vec2(flip * 4.0, -10.0 - bob), 30.0, body);
            if state == State::ChaosHold {
                // держит курсор — руки подняты вверх к курсору над головой
                painter.line_segment(
                    [center + egui::vec2(-18.0, -18.0 - bob), center + egui::vec2(-8.0, -72.0 - bob)],
                    egui::Stroke::new(4.0, skin),
                );
                painter.line_segment(
                    [center + egui::vec2(18.0, -18.0 - bob), center + egui::vec2(8.0, -72.0 - bob)],
                    egui::Stroke::new(4.0, skin),
                );
            } else {
                // гонится — руки качаются в беге
                let arm = s * 12.0;
                painter.line_segment(
                    [center + egui::vec2(-26.0, -15.0 - bob), center + egui::vec2(flip * -34.0, 5.0 + arm - bob)],
                    egui::Stroke::new(4.0, skin),
                );
                painter.line_segment(
                    [center + egui::vec2(26.0, -15.0 - bob), center + egui::vec2(flip * 34.0, 5.0 - arm - bob)],
                    egui::Stroke::new(4.0, skin),
                );
            }
            draw_head(-bob);
        } else if vy < -120.0 {
            // ── ПРЫЖОК ВВЕРХ — ноги поджаты, руки замахиваются вверх ─────
            painter.line_segment(
                [center + egui::vec2(-6.0, 18.0), center + egui::vec2(-12.0, 40.0)],
                egui::Stroke::new(5.0, body),
            );
            painter.line_segment(
                [center + egui::vec2(6.0, 18.0), center + egui::vec2(12.0, 40.0)],
                egui::Stroke::new(5.0, body),
            );
            painter.circle_filled(center + egui::vec2(0.0, -10.0), 30.0, body);
            // руки вверх
            painter.line_segment(
                [center + egui::vec2(-22.0, -18.0), center + egui::vec2(-14.0, -58.0)],
                egui::Stroke::new(4.0, skin),
            );
            painter.line_segment(
                [center + egui::vec2(22.0, -18.0), center + egui::vec2(14.0, -58.0)],
                egui::Stroke::new(4.0, skin),
            );
            draw_head(0.0);
        } else if vy.abs() <= 120.0 {
            // ── ВЕРШИНА — тянется к курсору, тело вытянуто ───────────────
            painter.line_segment(
                [center + egui::vec2(-5.0, 20.0), center + egui::vec2(-7.0, 50.0)],
                egui::Stroke::new(5.0, body),
            );
            painter.line_segment(
                [center + egui::vec2(5.0, 20.0), center + egui::vec2(7.0, 50.0)],
                egui::Stroke::new(5.0, body),
            );
            painter.circle_filled(center + egui::vec2(0.0, -8.0), 29.0, body);
            // руки вытянуты максимально вверх
            painter.line_segment(
                [center + egui::vec2(-18.0, -20.0), center + egui::vec2(-8.0, -78.0)],
                egui::Stroke::new(4.0, skin),
            );
            painter.line_segment(
                [center + egui::vec2(18.0, -20.0), center + egui::vec2(8.0, -78.0)],
                egui::Stroke::new(4.0, skin),
            );
            draw_head(2.0);
        } else {
            // ── ПАДЕНИЕ — ноги болтаются вниз, руки держат вверху ────────
            painter.line_segment(
                [center + egui::vec2(-5.0, 20.0), center + egui::vec2(flip * -10.0, 56.0)],
                egui::Stroke::new(5.0, body),
            );
            painter.line_segment(
                [center + egui::vec2(5.0, 20.0), center + egui::vec2(flip * 12.0, 54.0)],
                egui::Stroke::new(5.0, body),
            );
            painter.circle_filled(center + egui::vec2(0.0, -6.0), 30.0, body);
            // руки вверх (держат курсор / схватились)
            painter.line_segment(
                [center + egui::vec2(-20.0, -16.0), center + egui::vec2(-10.0, -70.0)],
                egui::Stroke::new(4.0, skin),
            );
            painter.line_segment(
                [center + egui::vec2(20.0, -16.0), center + egui::vec2(10.0, -70.0)],
                egui::Stroke::new(4.0, skin),
            );
            draw_head(4.0);
        }
        return;
    }

    match activity {
        // ── Обычная ходьба / стоит ────────────────────────────────────────
        Activity::Normal => {
            let bob = if state == State::Walking {
                (walk_frame * pi).sin() * 3.0
            } else { 0.0 };

            let leg = if state == State::Walking {
                (walk_frame * pi).sin() * 8.0
            } else { 0.0 };
            let arm = if state == State::Walking {
                (walk_frame * pi).sin() * 10.0
            } else { 0.0 };

            // ноги
            painter.line_segment(
                [center + egui::vec2(-6.0, 20.0 + bob), center + egui::vec2(flip * (-6.0 + leg), 50.0 + bob)],
                egui::Stroke::new(5.0, body),
            );
            painter.line_segment(
                [center + egui::vec2(6.0, 20.0 + bob), center + egui::vec2(flip * (6.0 - leg), 50.0 + bob)],
                egui::Stroke::new(5.0, body),
            );
            // тело
            painter.circle_filled(center + egui::vec2(0.0, -10.0 + bob), 30.0, body);
            // руки
            painter.line_segment(
                [center + egui::vec2(-28.0, -15.0 + bob), center + egui::vec2(-38.0, 5.0 + arm + bob)],
                egui::Stroke::new(4.0, skin),
            );
            painter.line_segment(
                [center + egui::vec2(28.0, -15.0 + bob), center + egui::vec2(38.0, 5.0 - arm + bob)],
                egui::Stroke::new(4.0, skin),
            );
            // голова
            painter.circle_filled(center + egui::vec2(0.0, -55.0 + bob), 25.0, skin);
            painter.circle_filled(center + egui::vec2(0.0, -62.0 + bob), 22.0, hair);
            let ex = flip * 3.0;
            painter.circle_filled(center + egui::vec2(-7.0 + ex, -55.0 + bob), 3.5, eye);
            painter.circle_filled(center + egui::vec2( 7.0 + ex, -55.0 + bob), 3.5, eye);
        }

        // ── Кодит — сидит с ноутом ────────────────────────────────────────
        Activity::Coding => {
            let blink = if (anim * 0.7).sin() > 0.96 { 1.0 } else { 3.5 };
            let type_bob = (anim * 4.0).sin() * 1.5; // едва заметное покачивание при печати

            // ноги (сидит — горизонтально)
            painter.line_segment(
                [center + egui::vec2(-8.0, 22.0), center + egui::vec2(-30.0, 30.0)],
                egui::Stroke::new(5.0, body),
            );
            painter.line_segment(
                [center + egui::vec2(8.0, 22.0), center + egui::vec2(30.0, 30.0)],
                egui::Stroke::new(5.0, body),
            );
            // тело
            painter.circle_filled(center + egui::vec2(0.0, -5.0 + type_bob), 28.0, body);
            // ноутбук — прямоугольник
            painter.rect_filled(
                egui::Rect::from_center_size(center + egui::vec2(0.0, 28.0), egui::vec2(44.0, 24.0)),
                3.0,
                egui::Color32::from_rgb(60, 60, 80),
            );
            painter.rect_filled(
                egui::Rect::from_center_size(center + egui::vec2(0.0, 27.0), egui::vec2(38.0, 16.0)),
                2.0,
                egui::Color32::from_rgb(100, 180, 255),
            );
            // руки на клавиатуре
            painter.line_segment(
                [center + egui::vec2(-20.0, 8.0 + type_bob), center + egui::vec2(-16.0, 28.0)],
                egui::Stroke::new(4.0, skin),
            );
            painter.line_segment(
                [center + egui::vec2(20.0, 8.0 + type_bob), center + egui::vec2(16.0, 28.0)],
                egui::Stroke::new(4.0, skin),
            );
            // голова наклонена к экрану
            painter.circle_filled(center + egui::vec2(flip * 3.0, -48.0 + type_bob), 25.0, skin);
            painter.circle_filled(center + egui::vec2(flip * 3.0, -55.0 + type_bob), 22.0, hair);
            painter.circle_filled(center + egui::vec2(-6.0 + flip * 3.0, -48.0 + type_bob), blink, eye);
            painter.circle_filled(center + egui::vec2( 6.0 + flip * 3.0, -48.0 + type_bob), blink, eye);
        }

        // ── Смотрит YouTube — сидит и пялится ────────────────────────────
        Activity::Watching => {
            // ноги
            painter.line_segment(
                [center + egui::vec2(-8.0, 22.0), center + egui::vec2(-28.0, 32.0)],
                egui::Stroke::new(5.0, body),
            );
            painter.line_segment(
                [center + egui::vec2(8.0, 22.0), center + egui::vec2(28.0, 32.0)],
                egui::Stroke::new(5.0, body),
            );
            // тело
            painter.circle_filled(center + egui::vec2(0.0, -5.0), 28.0, body);
            // руки опущены
            painter.line_segment(
                [center + egui::vec2(-26.0, -8.0), center + egui::vec2(-26.0, 12.0)],
                egui::Stroke::new(4.0, skin),
            );
            painter.line_segment(
                [center + egui::vec2(26.0, -8.0), center + egui::vec2(26.0, 12.0)],
                egui::Stroke::new(4.0, skin),
            );
            // голова прямо, глаза широкие (смотрит вперёд)
            painter.circle_filled(center + egui::vec2(0.0, -52.0), 25.0, skin);
            painter.circle_filled(center + egui::vec2(0.0, -59.0), 22.0, hair);
            // широкие глаза
            painter.circle_filled(center + egui::vec2(-8.0, -52.0), 5.0, eye);
            painter.circle_filled(center + egui::vec2( 8.0, -52.0), 5.0, eye);
            painter.circle_filled(center + egui::vec2(-7.0, -51.0), 2.0, egui::Color32::WHITE);
            painter.circle_filled(center + egui::vec2( 9.0, -51.0), 2.0, egui::Color32::WHITE);
        }

        // ── Музыка — танцует ──────────────────────────────────────────────
        Activity::Music => {
            let b = (anim * 4.0 * pi).sin();
            let bob = b * 5.0;
            let arm_l = (anim * 4.0 * pi).sin() * 20.0;
            let arm_r = -(anim * 4.0 * pi).sin() * 20.0;

            // ноги (подпрыгивает)
            painter.line_segment(
                [center + egui::vec2(-6.0, 22.0 + bob), center + egui::vec2(-10.0, 48.0 + bob)],
                egui::Stroke::new(5.0, body),
            );
            painter.line_segment(
                [center + egui::vec2(6.0, 22.0 + bob), center + egui::vec2(10.0, 48.0 + bob)],
                egui::Stroke::new(5.0, body),
            );
            // тело
            painter.circle_filled(center + egui::vec2(0.0, -10.0 + bob), 30.0, body);
            // руки вверх
            painter.line_segment(
                [center + egui::vec2(-28.0, -15.0 + bob), center + egui::vec2(-42.0, -35.0 + arm_l + bob)],
                egui::Stroke::new(4.0, skin),
            );
            painter.line_segment(
                [center + egui::vec2(28.0, -15.0 + bob), center + egui::vec2(42.0, -35.0 + arm_r + bob)],
                egui::Stroke::new(4.0, skin),
            );
            // голова
            painter.circle_filled(center + egui::vec2(0.0, -55.0 + bob), 25.0, skin);
            painter.circle_filled(center + egui::vec2(0.0, -62.0 + bob), 22.0, hair);
            // улыбка (дуга)
            painter.circle_filled(center + egui::vec2(-7.0, -55.0 + bob), 3.5, eye);
            painter.circle_filled(center + egui::vec2( 7.0, -55.0 + bob), 3.5, eye);
            // нотки
            if b > 0.3 {
                painter.text(
                    center + egui::vec2(42.0, -50.0 + bob),
                    egui::Align2::CENTER_CENTER,
                    "♪",
                    egui::FontId::proportional(14.0),
                    egui::Color32::from_rgb(200, 150, 255),
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
    let bubble_w = lines.iter()
        .map(|l| l.chars().count() as f32 * 7.0 + pad.x * 2.0)
        .fold(60.0f32, f32::max)
        .min(max_w);

    // пузырёк над головой — внутри окна
    let pos = egui::pos2(center.x, center.y - 80.0);
    let rect = egui::Rect::from_center_size(pos, egui::vec2(bubble_w, bubble_h));

    painter.rect_filled(rect, 7.0, egui::Color32::from_rgba_unmultiplied(25, 25, 35, 235));
    painter.rect_stroke(rect, 7.0, egui::Stroke::new(1.5, egui::Color32::from_rgb(180, 140, 210)));

    // хвостик
    let tip = egui::pos2(center.x, rect.max.y + 8.0);
    painter.add(egui::Shape::convex_polygon(
        vec![tip, tip + egui::vec2(-7.0, -10.0), tip + egui::vec2(7.0, -10.0)],
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
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] { [0.0; 4] }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dt = ctx.input(|i| i.unstable_dt).min(0.05);

        // размер экрана
        if let Some(sz) = ctx.input(|i| i.viewport().monitor_size) {
            if sz.x > 100.0 { self.screen = sz; }
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
        if !self.chaos_mode
            && (self.state == State::ChaosChase || self.state == State::ChaosHold)
        {
            self.pos.y = self.ground_y();
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.pos));
            self.state = State::Idle;
            self.idle_timer = 1.0;
            self.vy = 0.0;
            self.last_set_cursor = None;
        }

        // AFK — мышь не двигается
        let mouse = ctx.input(|i| i.pointer.hover_pos()).unwrap_or(self.last_mouse);
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
            let threshold = if self.activity == Activity::Coding { 120.0 } else { 300.0 };
            if self.afk_timer >= threshold {
                let phrase = match self.activity {
                    Activity::Coding => self.choose(&[
                        "Эй. Ты живой?",
                        "Ctrl+S хотя бы нажми.",
                        "Заснул с открытым IDE.\nКлассика.",
                    ]),
                    Activity::Watching => self.choose(&[
                        "Ты там уснул?",
                        "Ты уже час смотришь.\nЭто рекорд.",
                    ]),
                    _ => self.choose(&["Куда ушёл?", "Ладно. Жду.", "Тихо стало."]),
                };
                self.say(phrase, 5.0);
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
                    .as_secs() % 86400) / 3600;

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
                    Activity::Normal if hour < 9 => self.choose(&[
                        "Кофе хоть выпил?",
                        "Доброе утро.\nХотя не факт.",
                    ]),
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
        if self.speech.as_ref().map(|(_, t)| *t <= 0.0).unwrap_or(false) {
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
                        self.state = State::Idle;
                        self.idle_timer = 0.5;
                        // не перебиваем уже активную реплику (напр. "Прощаю")
                        if self.speech.is_none() {
                            let p = self.choose(&[
                                "Ладно.",
                                "Поставил.",
                                "И зачем это было?",
                                "Уф.",
                            ]);
                            self.say(p, 2.0);
                        }
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
                            self.pos.x += if run > dx.abs() { dx } else { dx.signum() * run };
                            self.facing_left = dx < 0.0;
                        }
                        // прыжок: если под курсором и курсор выше рук
                        if dx.abs() < 70.0 && cursor.y < self.grab_point().y {
                            self.vy = -JUMP_V;
                            // баллистический толчок к курсору по горизонтали
                            self.chaos_vx = ((cursor.x - self.grab_point().x) * 1.2)
                                .clamp(-260.0, 260.0);
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
                    if (cursor - self.grab_point()).length() < GRAB_RADIUS {
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
                        self.last_set_cursor = None;     // отпускаем курсор
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
                                self.pos.x += if run > dx.abs() { dx } else { dx.signum() * run };
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

        // рисуем
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let response = ui.allocate_rect(ui.max_rect(), egui::Sense::click_and_drag());

                if response.drag_started() {
                    self.state = State::Dragged;
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    let p = self.choose(&["Эй!", "Куда?", "Руки.", "Хватит."]);
                    self.say(p, 2.0);
                }
                if response.clicked() {
                    let p = self.choose(&[
                        "Не тронь.",
                        "Что надо?",
                        "Стоп.",
                        "Я вижу тебя.",
                        "Зачем?",
                    ]);
                    self.say(p, 3.0);
                }
                if response.drag_stopped() {
                    if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
                        self.pos = egui::pos2(r.min.x, self.ground_y());
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.pos));
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
                draw_mascot(
                    painter, center,
                    self.state, self.activity,
                    self.walk_frame, self.facing_left,
                    self.anim_timer,
                    self.vy, on_ground,
                );

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
                    painter.text(hp, egui::Align2::CENTER_CENTER, "ПКМ = выход",
                        egui::FontId::proportional(11.0), egui::Color32::WHITE);
                }
            });

        ctx.request_repaint();
    }
}
