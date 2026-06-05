#![windows_subsystem = "windows"]

use eframe::egui;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(false)
            .with_inner_size([120.0, 180.0])
            .with_position([100.0, 100.0]),
        ..Default::default()
    };

    eframe::run_native(
        "mascot",
        options,
        Box::new(|_cc| Ok(Box::new(MascotApp::new()))),
    )
}

const WIN_W: f32 = 120.0;
const WIN_H: f32 = 180.0;
const TASKBAR_H: f32 = 50.0;
// пустое пространство ниже ног персонажа внутри окна
const FEET_PAD: f32 = 40.0;
const GRAVITY: f32 = 600.0;

#[derive(PartialEq, Clone, Copy)]
enum State {
    Walking,
    Idle,
    Dragged,
    Falling,
}

struct MascotApp {
    pos: egui::Pos2,
    target: egui::Pos2,
    state: State,
    state_before_drag: State,
    idle_timer: f32,
    vy: f32, // вертикальная скорость (для падения)
    facing_left: bool,
    walk_frame: f32,
    hovered: bool,
    rng: u64,
    screen: egui::Vec2,
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
            state_before_drag: State::Idle,
            idle_timer: 1.0,
            facing_left: false,
            walk_frame: 0.0,
            hovered: false,
            vy: 0.0,
            rng,
            screen: egui::vec2(1920.0, 1040.0),
        }
    }

    fn rand(&mut self) -> f32 {
        self.rng = self.rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // возвращаем float 0..1
        (self.rng >> 11) as f32 / (1u64 << 53) as f32
    }

    fn ground_y(&self) -> f32 {
        // ноги персонажа касаются верха панели задач
        // FEET_PAD — пустое место ниже ног в окне
        self.screen.y - TASKBAR_H - WIN_H + FEET_PAD
    }

    fn pick_target(&mut self) {
        let max_x = (self.screen.x - WIN_W).max(100.0);
        // только по X, Y всегда = земля
        self.target = egui::pos2(self.rand() * max_x, self.ground_y());
    }

    fn clamp_pos(&self, p: egui::Pos2) -> egui::Pos2 {
        let max_x = (self.screen.x - WIN_W).max(0.0);
        // X ограничен экраном, Y всегда прижат к земле
        egui::pos2(p.x.clamp(0.0, max_x), self.ground_y())
    }
}

impl eframe::App for MascotApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dt = ctx.input(|i| i.unstable_dt).min(0.05);

        // обновляем размер экрана
        if let Some(sz) = ctx.input(|i| i.viewport().monitor_size) {
            if sz.x > 100.0 {
                self.screen = sz;
            }
        }

        // ВАЖНО: синхронизируем self.pos с реальным положением окна каждый кадр
        // Это ловит и ручное перетаскивание и подтверждает наши команды
        if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
            if self.state == State::Dragged {
                self.pos = outer.min;
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let response = ui.allocate_rect(ui.max_rect(), egui::Sense::click_and_drag());

                if response.drag_started() {
                    self.state_before_drag = self.state;
                    self.state = State::Dragged;
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                if response.drag_stopped() {
                    if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                        self.pos = outer.min;
                    }
                    self.vy = 0.0;
                    self.state = State::Falling; // падаем вниз
                }

                if response.secondary_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }

                self.hovered = response.hovered();

                // авто-движение только когда не тащим мышкой
                if self.state != State::Dragged {
                    match self.state {
                        State::Walking => {
                            let speed = 80.0;
                            let dir = self.target - self.pos;
                            let dist = dir.length();

                            if dist < 4.0 {
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
                        State::Idle => {
                            self.idle_timer -= dt;
                            if self.idle_timer <= 0.0 {
                                self.pick_target();
                                self.state = State::Walking;
                            }
                        }
                        State::Falling => {
                            self.vy += GRAVITY * dt;
                            self.pos.y += self.vy * dt;
                            let ground = self.ground_y();
                            if self.pos.y >= ground {
                                self.pos.y = ground;
                                self.vy = 0.0;
                                self.target = self.pos;
                                self.state = State::Idle;
                                self.idle_timer = 0.5;
                            }
                            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.pos));
                        }
                        State::Dragged => {}
                    }
                }

                // рисуем персонажа
                let rect = ui.max_rect();
                let painter = ui.painter();
                let center = rect.center();

                let bob = if self.state == State::Walking {
                    (self.walk_frame * std::f32::consts::PI).sin() * 3.0
                } else {
                    0.0
                };
                let flip: f32 = if self.facing_left { -1.0 } else { 1.0 };

                // ноги
                let leg = if self.state == State::Walking {
                    (self.walk_frame * std::f32::consts::PI).sin() * 8.0
                } else {
                    0.0
                };
                painter.line_segment(
                    [center + egui::vec2(-6.0, 20.0 + bob), center + egui::vec2(flip * (-6.0 + leg), 50.0 + bob)],
                    egui::Stroke::new(5.0, egui::Color32::from_rgb(180, 140, 210)),
                );
                painter.line_segment(
                    [center + egui::vec2(6.0, 20.0 + bob), center + egui::vec2(flip * (6.0 - leg), 50.0 + bob)],
                    egui::Stroke::new(5.0, egui::Color32::from_rgb(180, 140, 210)),
                );

                // тело
                painter.circle_filled(
                    center + egui::vec2(0.0, -10.0 + bob),
                    30.0,
                    egui::Color32::from_rgb(180, 140, 210),
                );

                // руки
                let arm = if self.state == State::Walking {
                    (self.walk_frame * std::f32::consts::PI).sin() * 10.0
                } else {
                    0.0
                };
                painter.line_segment(
                    [center + egui::vec2(-28.0, -15.0 + bob), center + egui::vec2(-38.0, 5.0 + arm + bob)],
                    egui::Stroke::new(4.0, egui::Color32::from_rgb(255, 220, 200)),
                );
                painter.line_segment(
                    [center + egui::vec2(28.0, -15.0 + bob), center + egui::vec2(38.0, 5.0 - arm + bob)],
                    egui::Stroke::new(4.0, egui::Color32::from_rgb(255, 220, 200)),
                );

                // голова
                painter.circle_filled(
                    center + egui::vec2(0.0, -55.0 + bob),
                    25.0,
                    egui::Color32::from_rgb(255, 220, 200),
                );
                // волосы
                painter.circle_filled(
                    center + egui::vec2(0.0, -62.0 + bob),
                    22.0,
                    egui::Color32::from_rgb(200, 200, 220),
                );

                // глаза
                let eye_x = flip * 3.0;
                painter.circle_filled(
                    center + egui::vec2(-7.0 + eye_x, -55.0 + bob),
                    3.5,
                    egui::Color32::from_rgb(70, 50, 110),
                );
                painter.circle_filled(
                    center + egui::vec2(7.0 + eye_x, -55.0 + bob),
                    3.5,
                    egui::Color32::from_rgb(70, 50, 110),
                );

                // подсказка
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
