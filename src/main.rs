#![windows_subsystem = "windows"]

use eframe::egui;
use std::time::{SystemTime, UNIX_EPOCH};

const WIN_W: f32 = 160.0;
const WIN_H: f32 = 220.0;
const TASKBAR_H: f32 = 50.0;
const FEET_PAD: f32 = 40.0;
const GRAVITY: f32 = 600.0;
const WORK_WARN_SECS: f32 = 2.0 * 3600.0; // предупреждение через 2 часа

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
enum State { Walking, Idle, Dragged, Falling }

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

    activity:     Activity,
    app_timer:    f32,              // интервал опроса активного окна
    work_timer:   f32,              // сколько секунд кодим подряд
    anim_timer:   f32,              // таймер анимации для поз
    speech:       Option<(String, f32)>, // (текст, сколько ещё показывать)
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

    fn say(&mut self, text: &str, secs: f32) {
        self.speech = Some((text.to_string(), secs));
    }
}

// ── Рисование ────────────────────────────────────────────────────────────────

fn draw_mascot(
    painter: &egui::Painter,
    center: egui::Pos2,
    state: State,
    activity: Activity,
    walk_frame: f32,
    facing_left: bool,
    anim: f32,
) {
    let flip: f32 = if facing_left { -1.0 } else { 1.0 };
    let pi = std::f32::consts::PI;

    let skin  = egui::Color32::from_rgb(255, 220, 200);
    let hair  = egui::Color32::from_rgb(200, 200, 220);
    let body  = egui::Color32::from_rgb(180, 140, 210);
    let eye   = egui::Color32::from_rgb(70, 50, 110);

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
                            let phrases = [
                                "О, опять кодишь.",
                                "Снова за работу...",
                                "Посмотрим сколько продержишься.",
                                "Ладно, работай.",
                            ];
                            self.say(phrases[self.rng as usize % phrases.len()], 4.0);
                        }
                        Activity::Watching => {
                            let phrases = [
                                "YouTube? Серьёзно?",
                                "Ну и ладно, я тоже смотрю.",
                                "Отдыхаешь? Хм.",
                            ];
                            self.say(phrases[self.rng as usize % phrases.len()], 4.0);
                        }
                        Activity::Music => {
                            let phrases = [
                                "О, музыка!",
                                "Неплохой вкус.",
                                "Потанцуем!",
                            ];
                            self.say(phrases[self.rng as usize % phrases.len()], 4.0);
                        }
                        Activity::Normal => {
                            // не каждый раз, только иногда
                            if self.rng % 3 == 0 {
                                self.say("Куда теперь?", 3.0);
                            }
                        }
                    }
                    self.activity = a;
                }
            }
        }

        // таймер работы
        self.anim_timer += dt;
        if self.activity == Activity::Coding {
            self.work_timer += dt;
            if self.work_timer >= WORK_WARN_SECS && self.speech.is_none() {
                self.say("Ты уже 2 часа кодишь...\nМожет перерыв?", 6.0);
                self.work_timer = 0.0;
            }
        } else {
            self.work_timer = 0.0;
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
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.pos));
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

                draw_mascot(
                    painter, center,
                    self.state, self.activity,
                    self.walk_frame, self.facing_left,
                    self.anim_timer,
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
