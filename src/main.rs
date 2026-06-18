// Десктоп-маскот на Bevy + bevy_vrm1 (VRM 1.0).
//
// Реагирует на реальную активность пользователя машиной состояний:
//   - кодит (активно окно редактора)      → typing
//   - ничего не делает                     → idle, иногда walking
//   - бездействие 15 мин                   → sleeping_idle
//   - клик мышью по персонажу (его «бьют») → crying
//
// TODO (следующие шаги): танцы под музыку, погоня за курсором (run/catch/…),
// гнев перед «хаосом», перемещение окна при ходьбе.

mod platform;
mod voice;

use std::fs;
use std::time::Duration;

use bevy::animation::RepeatAnimation;
use bevy::prelude::*;
use bevy::render::settings::{Backends, RenderCreation, WgpuSettings};
use bevy::render::RenderPlugin;
use bevy::window::{CompositeAlphaMode, Window, WindowLevel, WindowPosition};
use bevy_vrm1::prelude::*;

use platform::{cursor_pos, foreground_exe, idle_seconds, is_editor, set_cursor_pos, work_area};
use std::f32::consts::FRAC_PI_2;

const ASSETS_DIR: &str = "assets";
const MODEL: &str = "vrm1_model.vrm";
const ANIM_SUBDIR: &str = "anim";

fn arg_value(name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == name {
            return args.next();
        }
        if let Some(value) = arg.strip_prefix(&prefix) {
            return Some(value.to_string());
        }
    }
    None
}

fn render_backends_from_args() -> Option<Backends> {
    match arg_value("--backend")
        .or_else(|| std::env::var("MASCOT_BACKEND").ok())
        .as_deref()
    {
        Some("auto") | None => Some(Backends::VULKAN),
        Some("vulkan") => Some(Backends::VULKAN),
        Some("dx12") => Some(Backends::DX12),
        Some("all") => Some(Backends::all()),
        Some(other) => {
            eprintln!("unknown --backend={other}; using vulkan");
            Some(Backends::VULKAN)
        }
    }
}

fn alpha_mode_from_args() -> CompositeAlphaMode {
    match arg_value("--alpha")
        .or_else(|| std::env::var("MASCOT_ALPHA").ok())
        .as_deref()
    {
        Some("auto") => CompositeAlphaMode::Auto,
        Some("opaque") => CompositeAlphaMode::Opaque,
        Some("pre") | None => CompositeAlphaMode::PreMultiplied,
        Some("post") => CompositeAlphaMode::PostMultiplied,
        Some("inherit") => CompositeAlphaMode::Inherit,
        Some(other) => {
            eprintln!("unknown --alpha={other}; using pre");
            CompositeAlphaMode::PreMultiplied
        }
    }
}

/// Путь к ресурсу относительно папки с exe (для распакованной сборки по двойному
/// клику), с фолбэком на рабочую папку — чтобы `cargo run` из корня тоже работал.
pub(crate) fn resource_path(rel: &str) -> std::path::PathBuf {
    let rel_path = std::path::PathBuf::from(rel);
    if rel_path.is_absolute() {
        return rel_path;
    }

    fn usable(path: &std::path::Path, rel: &str) -> bool {
        if rel == ASSETS_DIR {
            path.join(MODEL).exists() && path.join(ANIM_SUBDIR).join("idle.vrma").exists()
        } else {
            path.exists()
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(rel);
            if usable(&p, rel) {
                return p;
            }
        }
    }

    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    if usable(&p, rel) {
        return p;
    }

    rel_path
}

// --- Доли рабочей области экрана (всё остальное вычисляется из них при старте) ---
const WIN_H_FRAC: f32 = 0.30; // высота окна = 30% высоты рабочей области
const WIN_ASPECT: f32 = 0.66; // win_w / win_h
const CHAR_FILL: f32 = 0.68; // доля высоты окна, занятая персонажем (связано с CAM_Z)
const RUN_FRAC: f32 = 0.27; // скорость бега к курсору / ширину экрана (в секунду)
const WALK_FRAC: f32 = 0.047; // скорость ходьбы / ширину экрана
const JUMP_FRAC: f32 = 0.45; // скорость взлёта к цели / высоту экрана (в секунду)
const CATCH_RANGE_FRAC: f32 = 0.05; // дистанция ловли (допуск «dx») / ширину окна
const GRAVITY_FRAC: f32 = 2.5; // гравитация / высоту экрана (пикс/с²)
                               // Дистанция камеры: чем больше, тем мельче персонаж в кадре (связано с CHAR_FILL).
const CAM_Z: f32 = 3.3;

// Пороги поведения (секунды)
const TYPING_GRACE: f32 = 120.0; // редактор активен и ввод был не дольше этого назад
const SLEEP_AFTER: f32 = 900.0; // 15 минут бездействия → сон
const WALK_DURATION: f32 = 8.0; // как долго длится одна прогулка
const WALK_DELAY_MIN: f32 = 60.0; // мин. ожидание в idle до прогулки (1 мин)
const WALK_DELAY_MAX: f32 = 300.0; // макс. ожидание в idle до прогулки (5 мин)
const CRY_DURATION: f32 = 4.0; // как долго плачет после удара
const FALL_END_DURATION: f32 = 1.05; // длина клипа falling_end (~1.04 с)

// --- Хаус (погоня за курсором) ---
const WORK_LIMIT: f32 = 1500.0; // переработка: 25 мин кодинга → хаус (тест: клавиша C)
const ANGRY_DURATION: f32 = 2.5; // длительность предупреждения перед погоней
const CHASE_TIMEOUT: f32 = 12.0; // если не догнал — сдаёмся
const JUMP_DELAY: f32 = 0.4; // задержка перед взлётом (замах) после входа в jumping_up
const PARACHUTE_FRAC: f32 = 0.12; // скорость равномерного спуска «на парашюте» после поимки / высоту экрана (в сек)

// --- Голос ---
const SALUTE_DURATION: f32 = 2.83; // длина клипа salute: держим состояние, пока играет

// Конфигурация в пикселях, вычисленная из рабочей области экрана при старте.
#[derive(Resource, Clone, Copy)]
struct Config {
    work_left: i32,
    work_top: i32,
    work_w: i32,
    work_h: i32,
    win_w: i32,
    win_h: i32,
    char_px: f32,
    run_speed: f32,
    walk_speed: f32,
    jump_speed: f32,
    catch_range: f32,
    gravity: f32,
    parachute_speed: f32, // равномерная скорость спуска после поимки (hanging)
}

impl Config {
    fn from_work_area() -> Self {
        let (left, top, right, bottom) = work_area();
        let work_w = (right - left).max(1);
        let work_h = (bottom - top).max(1);
        let win_h = (work_h as f32 * WIN_H_FRAC) as i32;
        let win_w = (win_h as f32 * WIN_ASPECT) as i32;
        Self {
            work_left: left,
            work_top: top,
            work_w,
            work_h,
            win_w,
            win_h,
            char_px: win_h as f32 * CHAR_FILL,
            run_speed: work_w as f32 * RUN_FRAC,
            walk_speed: work_w as f32 * WALK_FRAC,
            jump_speed: work_h as f32 * JUMP_FRAC,
            catch_range: win_w as f32 * CATCH_RANGE_FRAC,
            gravity: work_h as f32 * GRAVITY_FRAC,
            parachute_speed: work_h as f32 * PARACHUTE_FRAC,
        }
    }
    // Y «земли»: низ окна на уровне верха таскбара.
    fn ground_y(&self) -> f32 {
        (self.work_top + self.work_h - self.win_h) as f32
    }
    // Смещение (пиксели от верха окна) до макушки/поднятой руки.
    fn hand_top_offset(&self) -> f32 {
        self.win_h as f32 - self.char_px
    }
    fn min_x(&self) -> f32 {
        self.work_left as f32
    }
    fn max_x(&self) -> f32 {
        (self.work_left + self.work_w - self.win_w) as f32
    }
}

#[derive(Component)]
struct VrmRoot;

// Имя анимации (файл), кладём на сущность VRMA при спавне.
#[derive(Component)]
struct AnimName(String);

// Состояния маскота. Каждому соответствует анимация и мимика.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Boot, // стартовое, до загрузки анимаций
    Idle,
    Walking,
    Typing,
    Sleeping,
    Crying,
    Hanging, // тащат мышью
    Falling, // отпустили — падает (внутри есть фаза приземления falling_end)
    // --- хаус ---
    Angry,     // предупреждение перед погоней
    ChaseRun,  // бежит к курсору
    JumpingUp, // прыгает к цели (клип играет 1 раз и замирает на апексе)
    Saluting,  // приветствие-подтверждение голосовой команды (one-shot)
}

impl State {
    // имя анимации (файл без .vrma) для состояния
    fn anim(self) -> &'static str {
        match self {
            State::Boot | State::Idle => "idle",
            State::Walking => "walking",
            State::Typing => "typing",
            State::Sleeping => "sleeping_idle",
            State::Crying => "crying",
            State::Hanging => "hanging",
            State::Falling => "on_falling",
            State::Angry => "angry",
            State::ChaseRun => "run",
            State::JumpingUp => "jumping_up",
            State::Saluting => "salute",
        }
    }

    fn is_drag(self) -> bool {
        matches!(self, State::Hanging | State::Falling)
    }

    fn is_chaos(self) -> bool {
        matches!(self, State::Angry | State::ChaseRun | State::JumpingUp)
    }

    // Разовые анимации (прыжок, салют) — играем один раз, замирают на последнем кадре.
    fn one_shot(self) -> bool {
        matches!(self, State::JumpingUp | State::Saluting)
    }

    // Боком к зрителю (бег/ходьба вдоль экрана).
    fn faces_sideways(self) -> bool {
        matches!(self, State::Walking | State::ChaseRun)
    }

    // Мгновенный переход без кроссфейда (резкие, динамичные анимации).
    fn instant(self) -> bool {
        self.is_drag()
            || matches!(
                self,
                State::Walking | State::Angry | State::ChaseRun | State::JumpingUp
            )
    }
}

// Библиотека анимаций: имя (без .vrma) → сущность VRMA.
#[derive(Resource, Default)]
struct AnimLibrary {
    map: std::collections::HashMap<String, Entity>,
}

// Состояние маскота + тайминги/рандом.
#[derive(Resource)]
struct Mascot {
    state: State,
    entered: f32,          // elapsed_secs на момент входа в состояние
    walk_at: Option<f32>,  // когда из idle выйти на прогулку (None = таймер снят)
    sleep_at: Option<f32>, // когда заснуть; idle/walk его НЕ сбрасывают
    cry_until: f32,        // если now < cry_until — плачет
    pos_x: f32,            // текущий X окна
    pos_y: f32,            // текущий Y окна (меняется при таскании/падении)
    walk_dir: f32,         // направление ходьбы: -1 влево, +1 вправо
    dragging: bool,        // тащат ли мышью прямо сейчас
    vel_y: f32,            // вертикальная скорость при падении
    fall_switch: f32,      // через сколько секунд падения включить falling_end
    fall_switched: bool,   // уже переключились на falling_end?
    work_secs: f32,        // накопленное время кодинга (для переработки → хаус)
    jump_to: f32,          // целевая верхняя точка окна при прыжке
    fall_slow: bool,       // спуск на парашюте (после поимки)
    fall_hold: bool,       // держать украденный курсор во время падения
    rng: u32,
}

impl Default for Mascot {
    fn default() -> Self {
        Self {
            state: State::Boot,
            entered: 0.0,
            walk_at: None,
            sleep_at: None,
            cry_until: 0.0,
            pos_x: 200.0,
            pos_y: 200.0,
            walk_dir: -1.0,
            dragging: false,
            vel_y: 0.0,
            fall_switch: 0.0,
            fall_switched: false,
            work_secs: 0.0,
            jump_to: 0.0,
            fall_slow: false,
            fall_hold: false,
            rng: 0x1234_5678,
        }
    }
}

impl Mascot {
    fn rand01(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.rng >> 16) as f32 / 65536.0
    }
}

fn main() {
    // Всё конфигурируется от рабочей области экрана, полученной при старте.
    let cfg = Config::from_work_area();
    // Абсолютный путь к assets (рядом с exe в сборке, либо ./assets при cargo run).
    let assets_path = resource_path(ASSETS_DIR).to_string_lossy().into_owned();
    let render_backends = render_backends_from_args();
    let alpha_mode = alpha_mode_from_args();

    App::new()
        .insert_resource(ClearColor(Color::NONE))
        .insert_resource(cfg)
        .init_resource::<AnimLibrary>()
        .init_resource::<Mascot>()
        .add_plugins(
            DefaultPlugins
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(WgpuSettings {
                        backends: render_backends,
                        ..default()
                    }),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "mascot".into(),
                        transparent: true,
                        composite_alpha_mode: alpha_mode,
                        decorations: false,
                        window_level: WindowLevel::AlwaysOnTop,
                        resolution: UVec2::new(cfg.win_w as u32, cfg.win_h as u32).into(),
                        // стартовая позиция уточняется в position_window()
                        position: WindowPosition::At(IVec2::new(200, 200)),
                        resizable: false,
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: assets_path.clone(),
                    ..default()
                }),
        )
        .add_plugins((VrmPlugin, VrmaPlugin, MeshPickingPlugin))
        .add_plugins(voice::VoicePlugin)
        .add_systems(
            Startup,
            (
                setup_camera,
                setup_light,
                spawn_vrm,
                position_window,
            ),
        )
        .add_systems(
            Update,
            (
                voice_reaction_system.before(behavior_system),
                behavior_system,
                (
                    walk_movement,
                    drag_follow,
                    falling_system,
                    chaos_system,
                    apply_window_pos,
                )
                    .chain(),
                expression_system,
                facing_system,
                breathing_system,
            ),
        )
        .run();
}

// Стартовая позиция: правый-нижний угол, ноги на таскбаре.
fn position_window(cfg: Res<Config>, mut windows: Query<&mut Window>, mut mascot: ResMut<Mascot>) {
    mascot.pos_x = cfg.max_x();
    mascot.pos_y = cfg.ground_y();
    for mut win in &mut windows {
        win.position = WindowPosition::At(IVec2::new(mascot.pos_x as i32, mascot.pos_y as i32));
    }
}

// Ходьба: двигаем X туда-сюда с отскоком от краёв.
fn walk_movement(time: Res<Time>, cfg: Res<Config>, mut mascot: ResMut<Mascot>) {
    if mascot.state != State::Walking {
        return;
    }
    let (min_x, max_x) = (cfg.min_x(), cfg.max_x());
    let mut x = mascot.pos_x + mascot.walk_dir * cfg.walk_speed * time.delta_secs();
    if x <= min_x {
        x = min_x;
        mascot.walk_dir = 1.0;
    } else if x >= max_x {
        x = max_x;
        mascot.walk_dir = -1.0;
    }
    mascot.pos_x = x;
}

// Пока тащат — окно следует за курсором (захват за верхнюю часть тела).
fn drag_follow(cfg: Res<Config>, mut mascot: ResMut<Mascot>) {
    if !mascot.dragging {
        return;
    }
    let (cx, cy) = cursor_pos();
    // курсор держит персонажа за поднятую руку (макушку силуэта)
    mascot.pos_x = cx as f32 - cfg.win_w as f32 / 2.0;
    mascot.pos_y = cy as f32 - cfg.hand_top_offset();
}

// Падение после отпускания + приземление.
fn falling_system(
    time: Res<Time>,
    cfg: Res<Config>,
    lib: Res<AnimLibrary>,
    mut mascot: ResMut<Mascot>,
    mut commands: Commands,
) {
    if mascot.state != State::Falling {
        return;
    }
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let ground = cfg.ground_y();

    // За FALL_END_DURATION до земли переключаем анимацию на приземление,
    // чтобы оно доиграло точно к касанию (мгновенно, без кроссфейда).
    if !mascot.fall_switched && now - mascot.entered >= mascot.fall_switch {
        mascot.fall_switched = true;
        mascot.fall_slow = false; // парашютный hanging закончился — дальше обычное падение
        play(
            &mut commands,
            &lib,
            "falling_end",
            Duration::ZERO,
            RepeatAnimation::Never,
        );
    }

    if mascot.fall_slow {
        // после поимки — равномерный медленный спуск «на парашюте» (анимация hanging)
        mascot.pos_y += cfg.parachute_speed * dt;
    } else {
        // обычное свободное падение с ускорением
        mascot.vel_y += cfg.gravity * dt;
        mascot.pos_y += mascot.vel_y * dt;
    }

    // если держим украденный курсор — тянем его за рукой/животом
    if mascot.fall_hold {
        // та же точка, что и при таскании мышью (drag_follow): центр окна по X,
        // hand_top_offset по Y — там в позе hanging находятся руки
        let hx = (mascot.pos_x + cfg.win_w as f32 / 2.0) as i32;
        let hy = (mascot.pos_y + cfg.hand_top_offset()) as i32;
        set_cursor_pos(hx, hy);
    }

    if mascot.pos_y >= ground {
        mascot.pos_y = ground;
        mascot.vel_y = 0.0;
        mascot.fall_hold = false;
        mascot.fall_slow = false;
        enter(&mut mascot, &mut commands, &lib, now, State::Idle);
    }
}

// Хаус: angry → бежит к курсору → прыгает (jumping_up) → поймал «hanging» / промах падение.
fn chaos_system(
    time: Res<Time>,
    cfg: Res<Config>,
    lib: Res<AnimLibrary>,
    mut mascot: ResMut<Mascot>,
    mut commands: Commands,
) {
    if !mascot.state.is_chaos() {
        return;
    }
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let elapsed = now - mascot.entered;
    let (min_x, max_x) = (cfg.min_x(), cfg.max_x());
    let (cx, cy) = cursor_pos();
    let center_x = mascot.pos_x + cfg.win_w as f32 / 2.0;
    // персонаж нарисован по центру окна — точка руки/ловли тоже по центру
    let hand_x = center_x;

    match mascot.state {
        // предупреждение, затем бежим
        State::Angry if elapsed >= ANGRY_DURATION => {
            enter(&mut mascot, &mut commands, &lib, now, State::ChaseRun);
        }
        // бежим к курсору по X
        State::ChaseRun => {
            let dir = (cx as f32 - center_x).signum();
            mascot.walk_dir = dir;
            mascot.pos_x = (mascot.pos_x + dir * cfg.run_speed * dt).clamp(min_x, max_x);

            let hand_now = mascot.pos_x + cfg.win_w as f32 / 2.0;
            let near = (cx as f32 - hand_now).abs() <= cfg.catch_range;
            if near {
                // добежали — прыгаем к курсору (цель: рука к курсору).
                // Над персонажем в окне есть пустое место (hand_top_offset), поэтому
                // верху окна разрешаем уйти выше края экрана — иначе рука не дотянется
                // до курсора у самой верхней кромки.
                let top_limit = cfg.work_top as f32 - cfg.hand_top_offset();
                mascot.jump_to =
                    (cy as f32 - cfg.hand_top_offset()).clamp(top_limit, cfg.ground_y());
                mascot.vel_y = 0.0;
                enter(&mut mascot, &mut commands, &lib, now, State::JumpingUp);
            } else if elapsed >= CHASE_TIMEOUT {
                // не догнали — сдаёмся
                enter(&mut mascot, &mut commands, &lib, now, State::Idle);
            }
        }
        // jumping_up: окно летит к цели за время = дистанция / jump_speed.
        // Клип jumping_up играет один раз и держит последний кадр оставшийся полёт.
        // Долетели → проверка (X и Y): поймал → hanging+медленное падение,
        // промах → on_falling; оба приземляются в falling_end.
        State::JumpingUp => {
            let ground = cfg.ground_y();
            let distance = (ground - mascot.jump_to).max(1.0);
            let travel = distance / cfg.jump_speed; // время полёта до цели
                                                    // короткий замах: первые JUMP_DELAY секунд стоим на земле, потом взлёт
            let airborne = (elapsed - JUMP_DELAY).max(0.0);
            let p = (airborne / travel).min(1.0);
            mascot.pos_y = ground + (mascot.jump_to - ground) * p; // лерп земля→цель
            if p >= 1.0 {
                // долетели до цели — проверяем ловлю
                let hand_y = mascot.pos_y + cfg.hand_top_offset();
                let dx = (cx as f32 - hand_x).abs();
                let dy = (cy as f32 - hand_y).abs();
                if dx <= cfg.catch_range && dy <= cfg.catch_range {
                    info!(
                        "jump ✓ поймал: курсор=({},{}) рука=({:.0},{:.0}) dx={:.0} dy={:.0}",
                        cx, cy, hand_x, hand_y, dx, dy
                    );
                    // hanging + медленное падение, держа курсор
                    start_fall(&mut mascot, &mut commands, &lib, &cfg, now, true);
                } else {
                    info!(
                        "jump ✗ промах: курсор=({},{}) рука=({:.0},{:.0}) dx={:.0} dy={:.0}",
                        cx, cy, hand_x, hand_y, dx, dy
                    );
                    start_fall(&mut mascot, &mut commands, &lib, &cfg, now, false);
                }
            }
        }
        _ => {}
    }
}

// Применяем позицию маскота к окну (единая точка управления окном).
// Двигаем окно только при реальном изменении позиции — в статичных
// состояниях (idle/typing/sleeping) это экономит SetWindowPos каждый кадр.
fn apply_window_pos(
    mascot: Res<Mascot>,
    mut last: Local<Option<IVec2>>,
    mut windows: Query<&mut Window>,
) {
    let pos = IVec2::new(mascot.pos_x as i32, mascot.pos_y as i32);
    if *last == Some(pos) {
        return;
    }
    *last = Some(pos);
    for mut win in &mut windows {
        win.position = WindowPosition::At(pos);
    }
}

fn setup_camera(mut commands: Commands) {
    // Кадрируем так, чтобы ступни (y=0) были у нижнего края окна.
    // При вертикальном FOV=45°: нижняя граница кадра = cam_y - cam_z*tan(22.5°).
    // Приравниваем к 0 → cam_y = cam_z*0.4142. Больше CAM_Z — мельче персонаж.
    let cam_z = CAM_Z;
    let cam_y = cam_z * 0.4142; // ступни внизу кадра
    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        Transform::from_xyz(0.0, cam_y, cam_z).looking_at(Vec3::new(0.0, cam_y, 0.0), Vec3::Y),
    ));
}

fn setup_light(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 6000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(2.0, 4.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn spawn_vrm(mut commands: Commands, asset_server: Res<AssetServer>) {
    let anim_dir = resource_path(ASSETS_DIR).join(ANIM_SUBDIR);
    let mut files: Vec<String> = fs::read_dir(&anim_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.to_lowercase().ends_with(".vrma"))
        .collect();
    files.sort();
    info!("Анимации: {:?}", files);

    commands
        .spawn((VrmHandle(asset_server.load(MODEL)), VrmRoot))
        // клик мышью по персонажу = «его ударили» → плач
        .observe(on_hit)
        // перетаскивание мышью: схватили → висит, отпустили → падает
        .observe(on_drag_start)
        .observe(on_drag_end)
        .with_children(|cmd| {
            for f in &files {
                let path = format!("{ANIM_SUBDIR}/{f}");
                cmd.spawn((VrmaHandle(asset_server.load(path)), AnimName(f.clone())))
                    .observe(on_vrma_loaded);
            }
        });
}

// VRMA догрузилась → регистрируем в библиотеке по имени без расширения.
fn on_vrma_loaded(trigger: On<LoadedVrma>, names: Query<&AnimName>, mut lib: ResMut<AnimLibrary>) {
    let vrma = trigger.vrma;
    if let Ok(name) = names.get(vrma) {
        let key = name.0.trim_end_matches(".vrma").to_string();
        lib.map.insert(key, vrma);
    }
}

// Клик по персонажу → плач на CRY_DURATION секунд.
fn on_hit(_trigger: On<Pointer<Click>>, time: Res<Time>, mut mascot: ResMut<Mascot>) {
    mascot.cry_until = time.elapsed_secs() + CRY_DURATION;
    info!("Ай! По мне кликнули → crying");
}

// Начали тащить мышью → висит.
fn on_drag_start(
    _trigger: On<Pointer<DragStart>>,
    time: Res<Time>,
    lib: Res<AnimLibrary>,
    mut mascot: ResMut<Mascot>,
    mut commands: Commands,
) {
    mascot.dragging = true;
    let now = time.elapsed_secs();
    enter(&mut mascot, &mut commands, &lib, now, State::Hanging);
}

// Отпустили → падает.
fn on_drag_end(
    _trigger: On<Pointer<DragEnd>>,
    time: Res<Time>,
    cfg: Res<Config>,
    lib: Res<AnimLibrary>,
    mut mascot: ResMut<Mascot>,
    mut commands: Commands,
) {
    mascot.dragging = false;
    let now = time.elapsed_secs();
    start_fall(&mut mascot, &mut commands, &lib, &cfg, now, false);
}

// Перейти в новое состояние: обновить таймеры и запустить анимацию.
fn enter(mascot: &mut Mascot, commands: &mut Commands, lib: &AnimLibrary, now: f32, new: State) {
    if mascot.state == new {
        return;
    }
    info!("Состояние: {:?} → {:?}", mascot.state, new);
    mascot.state = new;
    mascot.entered = now;
    match new {
        State::Idle => {
            // взводим таймер прогулки (1–5 мин)
            let delay = WALK_DELAY_MIN + mascot.rand01() * (WALK_DELAY_MAX - WALK_DELAY_MIN);
            mascot.walk_at = Some(now + delay);
            // таймер сна — только если ещё не идёт (idle↔walk его не сбрасывают)
            if mascot.sleep_at.is_none() {
                mascot.sleep_at = Some(now + SLEEP_AFTER);
            }
            info!(
                "idle: прогулка через {:.0} c, сон через {:.0} c",
                delay,
                mascot.sleep_at.unwrap() - now
            );
        }
        // прогулка — часть безделья: таймер сна не трогаем
        State::Walking => mascot.walk_at = None,
        // любое прочее состояние сбрасывает оба таймера
        _ => {
            mascot.walk_at = None;
            mascot.sleep_at = None;
        }
    }
    // у динамичных состояний переход мгновенный (плавность мешает),
    // у остальных — кроссфейд 300 мс
    let transition = if new.instant() {
        Duration::ZERO
    } else {
        Duration::from_millis(300)
    };
    let repeat = if new.one_shot() {
        RepeatAnimation::Never
    } else {
        RepeatAnimation::Forever
    };
    play(commands, lib, new.anim(), transition, repeat);
}

// Запустить падение. `caught` различает два сценария:
//   true  — персонаж поймал курсор: висит (hanging), падает медленно, тянет курсор;
//   false — обычное падение (промах/отпустили мышью): on_falling, обычная скорость.
// Рассчитываем момент включения falling_end, чтобы он доиграл ровно к земле.
fn start_fall(
    mascot: &mut Mascot,
    commands: &mut Commands,
    lib: &AnimLibrary,
    cfg: &Config,
    now: f32,
    caught: bool,
) {
    mascot.fall_slow = caught;
    mascot.fall_hold = caught;
    mascot.vel_y = 0.0;
    let distance = (cfg.ground_y() - mascot.pos_y).max(0.0);
    // время спуска: при поимке — равномерный парашют, иначе — свободное падение
    let total = if caught {
        distance / cfg.parachute_speed
    } else {
        (2.0 * distance / cfg.gravity).sqrt()
    };
    mascot.fall_switch = (total - FALL_END_DURATION).max(0.0);
    mascot.fall_switched = false;
    enter(mascot, commands, lib, now, State::Falling); // играет on_falling
                                                       // при поимке в воздухе нужна анимация удержания (hanging)
    if caught {
        play(
            commands,
            lib,
            "hanging",
            Duration::ZERO,
            RepeatAnimation::Forever,
        );
    }
}

// Запустить анимацию состояния с заданным режимом повтора и временем перехода.
fn play(
    commands: &mut Commands,
    lib: &AnimLibrary,
    name: &str,
    transition: Duration,
    repeat: RepeatAnimation,
) {
    if let Some(&vrma) = lib.map.get(name) {
        commands.trigger(PlayVrma {
            vrma,
            repeat,
            transition_duration: transition,
            reset_spring_bones: false,
        });
    } else {
        warn!("Анимация '{name}' не найдена");
    }
}

// Реакция на принятую голосовую команду: проигрываем приветствие (salute) один раз.
fn voice_reaction_system(
    mut messages: MessageReader<voice::VoiceCommandAccepted>,
    time: Res<Time>,
    lib: Res<AnimLibrary>,
    mut mascot: ResMut<Mascot>,
    mut commands: Commands,
) {
    // дренируем все накопленные сообщения, реагируем если было хоть одно
    let mut any = false;
    for _ in messages.read() {
        any = true;
    }
    if !any {
        return;
    }
    // нет клипа или маскот занят таскать/падать/хаусить — не реагируем
    if !lib.map.contains_key("salute")
        || mascot.state.is_drag()
        || mascot.state.is_chaos()
        || mascot.state == State::Saluting
    {
        return;
    }
    let now = time.elapsed_secs();
    enter(&mut mascot, &mut commands, &lib, now, State::Saluting);
}

// Главная машина состояний: смотрит на активность и переключает состояние/анимацию.
fn behavior_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    lib: Res<AnimLibrary>,
    mut editor_cache: Local<(f32, bool)>, // (время следующей проверки, закэшированный «редактор активен»)
    mut mascot: ResMut<Mascot>,
    mut commands: Commands,
) {
    // ждём, пока загрузится хотя бы idle
    if !lib.map.contains_key("idle") {
        return;
    }
    // во время таскания/падения/хауса поведением управляют другие системы
    if mascot.state.is_drag() || mascot.state.is_chaos() {
        return;
    }
    let now = time.elapsed_secs();
    // пока играет приветствие (реакция на команду) — не перебиваем
    if mascot.state == State::Saluting && now < mascot.entered + SALUTE_DURATION {
        return;
    }
    let dt = time.delta_secs();
    let idle = idle_seconds();
    // foreground_exe() открывает хэндл процесса — дорого делать каждый кадр.
    // Опрашиваем ~3 раза в секунду, между опросами берём кэш.
    let (next_check, cached) = &mut *editor_cache;
    if now >= *next_check {
        *cached = foreground_exe().as_deref().map(is_editor).unwrap_or(false);
        *next_check = now + 0.3;
    }
    let editor = *cached;

    // Таймер переработки: копим время кодинга, перерыв (долгое бездействие) сбрасывает.
    let coding = editor && idle < TYPING_GRACE;
    if coding {
        mascot.work_secs += dt;
    } else if idle >= 60.0 {
        mascot.work_secs = 0.0;
    }
    // Переработал или нажал C (тест) → начинаем хаус с гнева.
    if mascot.work_secs >= WORK_LIMIT || keys.just_pressed(KeyCode::KeyC) {
        mascot.work_secs = 0.0;
        info!("ХАУС! Переработка — иду за курсором");
        enter(&mut mascot, &mut commands, &lib, now, State::Angry);
        return;
    }

    // Определяем желаемое состояние (по приоритету).
    let desired = if now < mascot.cry_until {
        State::Crying
    } else if editor && idle < TYPING_GRACE {
        State::Typing
    } else {
        match mascot.state {
            // начатую прогулку доводим до конца (8 c), потом вернёмся в idle
            State::Walking if now < mascot.entered + WALK_DURATION => State::Walking,
            // проснуться при свежей активности пользователя
            State::Sleeping if idle < 2.0 => State::Idle,
            State::Sleeping => State::Sleeping,
            _ => {
                if mascot.sleep_at.is_some_and(|t| now >= t) {
                    State::Sleeping
                } else if mascot.state == State::Idle && mascot.walk_at.is_some_and(|t| now >= t) {
                    State::Walking
                } else {
                    State::Idle
                }
            }
        }
    };

    enter(&mut mascot, &mut commands, &lib, now, desired);
}

// Мимика лица под текущее состояние. Mixamo-клипы мимику не несут — ставим сами.
fn expression_system(
    time: Res<Time>,
    mascot: Res<Mascot>,
    vrms: Query<Entity, With<Vrm>>,
    mut last: Local<Option<State>>,
    mut commands: Commands,
) {
    let Ok(vrm) = vrms.single() else { return };
    let t = time.elapsed_secs();

    // Crying/Sleeping анимируют моргание по времени — их шлём каждый кадр.
    // Остальные мимики статичны — обновляем только при смене состояния,
    // чтобы не аллоцировать Vec и не дёргать событие на каждом тике.
    let animated = matches!(mascot.state, State::Crying | State::Sleeping);
    if !animated && *last == Some(mascot.state) {
        return;
    }
    *last = Some(mascot.state);

    let mut weights: Vec<(&str, f32)> = Vec::new();
    match mascot.state {
        State::Crying => {
            weights.push(("sad", 1.0));
            let blink = (((t * 1.6).sin() + 1.0) * 0.5).powf(1.5);
            weights.push(("blink", blink));
        }
        State::Sleeping => {
            weights.push(("relaxed", 0.6));
            let open = (t * 0.4).sin().max(0.0) * 0.25;
            weights.push(("blink", (1.0 - open).clamp(0.0, 1.0)));
        }
        // злое лицо на всё время хауса
        State::Angry | State::ChaseRun | State::JumpingUp => {
            weights.push(("angry", 1.0));
        }
        // радостное лицо при приветствии-подтверждении команды
        State::Saluting => {
            weights.push(("happy", 1.0));
        }
        _ => {} // нейтральное лицо
    }
    commands.trigger(SetExpressions::from_iter(vrm, weights));
}

// Разворот модели: при ходьбе — лицом в сторону движения (боком к зрителю),
// иначе — лицом к камере. Поворот плавный (slerp), без щелчка.
fn facing_system(
    time: Res<Time>,
    mascot: Res<Mascot>,
    mut query: Query<&mut Transform, With<VrmRoot>>,
) {
    // VRM 1.0 при yaw=0 смотрит в +Z (на камеру).
    // -90° → лицом влево (-X), +90° → лицом вправо (+X).
    let target_yaw = if mascot.state.faces_sideways() {
        if mascot.walk_dir < 0.0 {
            -FRAC_PI_2
        } else {
            FRAC_PI_2
        }
    } else {
        0.0
    };
    let target = Quat::from_rotation_y(target_yaw);
    let s = (time.delta_secs() * 8.0).min(1.0); // скорость доворота
    for mut tf in &mut query {
        tf.rotation = tf.rotation.slerp(target, s);
    }
}

// Дыхание: лёгкое покачивание модели по Y.
fn breathing_system(time: Res<Time>, mut query: Query<&mut Transform, With<VrmRoot>>) {
    let t = time.elapsed_secs();
    for mut tf in &mut query {
        tf.translation.y = (t * 1.7).sin() * 0.005;
    }
}
