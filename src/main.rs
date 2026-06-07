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

use std::fs;
use std::path::Path;
use std::time::Duration;

use bevy::animation::RepeatAnimation;
use bevy::prelude::*;
use bevy::render::settings::{Backends, RenderCreation, WgpuSettings};
use bevy::render::RenderPlugin;
use bevy::window::{CompositeAlphaMode, Window, WindowLevel, WindowPosition};
use bevy_vrm1::prelude::*;

use platform::{cursor_pos, foreground_exe, idle_seconds, is_editor, work_area};
use std::f32::consts::FRAC_PI_2;

const ASSETS_DIR: &str = "D:/apps/ai_agent/assets";
const MODEL: &str = "vrm1_model.vrm";
const ANIM_SUBDIR: &str = "anim";

// Размер окна (логические пиксели). Меньше окно → меньше персонаж и меньше
// перекрытой площади рабочего стола. Соотношение 2:3 сохраняет кадрирование.
const WIN_W: i32 = 200;
const WIN_H: i32 = 300;

// Пороги поведения (секунды)
const TYPING_GRACE: f32 = 120.0; // редактор активен и ввод был не дольше этого назад
const SLEEP_AFTER: f32 = 900.0; // 15 минут бездействия → сон
const WALK_DURATION: f32 = 8.0; // как долго длится одна прогулка
const WALK_DELAY_MIN: f32 = 60.0; // мин. ожидание в idle до прогулки (1 мин)
const WALK_DELAY_MAX: f32 = 300.0; // макс. ожидание в idle до прогулки (5 мин)
const CRY_DURATION: f32 = 4.0; // как долго плачет после удара
const GRAVITY: f32 = 2600.0; // ускорение падения (пикс/с²)
const FALL_END_DURATION: f32 = 1.05; // длина клипа falling_end (~1.04 с)
// Точка захвата по вертикали: доля высоты окна от верха до поднятой руки.
// Персонаж тянет руку вверх → курсор должен быть у самого верха окна.
const HANG_GRAB: f32 = 0.06;

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
        }
    }

    fn is_drag(self) -> bool {
        matches!(self, State::Hanging | State::Falling)
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
    entered: f32,           // elapsed_secs на момент входа в состояние
    walk_at: Option<f32>,   // когда из idle выйти на прогулку (None = таймер снят)
    sleep_at: Option<f32>,  // когда заснуть; idle/walk его НЕ сбрасывают
    cry_until: f32,         // если now < cry_until — плачет
    pos_x: f32,             // текущий X окна
    pos_y: f32,             // текущий Y окна (меняется при таскании/падении)
    walk_dir: f32,          // направление ходьбы: -1 влево, +1 вправо
    dragging: bool,         // тащат ли мышью прямо сейчас
    vel_y: f32,             // вертикальная скорость при падении
    fall_switch: f32,       // через сколько секунд падения включить falling_end
    fall_switched: bool,    // уже переключились на falling_end?
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
    App::new()
        .insert_resource(ClearColor(Color::NONE))
        .init_resource::<AnimLibrary>()
        .init_resource::<Mascot>()
        .add_plugins(
            DefaultPlugins
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(WgpuSettings {
                        backends: Some(Backends::VULKAN),
                        ..default()
                    }),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "mascot".into(),
                        transparent: true,
                        composite_alpha_mode: CompositeAlphaMode::PreMultiplied,
                        decorations: false,
                        window_level: WindowLevel::AlwaysOnTop,
                        resolution: UVec2::new(WIN_W as u32, WIN_H as u32).into(),
                        // стартовая позиция уточняется в position_window()
                        position: WindowPosition::At(IVec2::new(200, 200)),
                        resizable: false,
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: ASSETS_DIR.into(),
                    ..default()
                }),
        )
        .add_plugins((VrmPlugin, VrmaPlugin, MeshPickingPlugin))
        .add_systems(Startup, (setup_camera, setup_light, spawn_vrm, position_window))
        .add_systems(
            Update,
            (
                behavior_system,
                (walk_movement, drag_follow, falling_system, apply_window_pos).chain(),
                expression_system,
                facing_system,
                breathing_system,
            ),
        )
        .run();
}

// Y-координата «земли» (низ окна на уровне верха таскбара).
fn ground_y() -> f32 {
    let (_, _, _, bottom) = work_area();
    (bottom - WIN_H) as f32
}

// Стартовая позиция: правый-нижний угол, ноги на таскбаре.
fn position_window(mut windows: Query<&mut Window>, mut mascot: ResMut<Mascot>) {
    let (_, _, right, _) = work_area();
    mascot.pos_x = (right - WIN_W) as f32;
    mascot.pos_y = ground_y();
    for mut win in &mut windows {
        win.position = WindowPosition::At(IVec2::new(mascot.pos_x as i32, mascot.pos_y as i32));
    }
}

// Ходьба: двигаем X туда-сюда с отскоком от краёв.
fn walk_movement(time: Res<Time>, mut mascot: ResMut<Mascot>) {
    if mascot.state != State::Walking {
        return;
    }
    let (left, _, right, _) = work_area();
    let min_x = left as f32;
    let max_x = (right - WIN_W) as f32;
    const SPEED: f32 = 90.0; // пикселей в секунду

    let mut x = mascot.pos_x + mascot.walk_dir * SPEED * time.delta_secs();
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
fn drag_follow(mut mascot: ResMut<Mascot>) {
    if !mascot.dragging {
        return;
    }
    let (cx, cy) = cursor_pos();
    // курсор держит персонажа за поднятую руку (у верха окна)
    mascot.pos_x = cx as f32 - WIN_W as f32 / 2.0;
    mascot.pos_y = cy as f32 - WIN_H as f32 * HANG_GRAB;
}

// Падение после отпускания + приземление.
fn falling_system(
    time: Res<Time>,
    lib: Res<AnimLibrary>,
    mut mascot: ResMut<Mascot>,
    mut commands: Commands,
) {
    if mascot.state != State::Falling {
        return;
    }
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let ground = ground_y();

    // За FALL_END_DURATION до земли переключаем анимацию на приземление,
    // чтобы оно доиграло точно к касанию (мгновенно, без кроссфейда).
    if !mascot.fall_switched && now - mascot.entered >= mascot.fall_switch {
        mascot.fall_switched = true;
        play(&mut commands, &lib, "falling_end", Duration::ZERO, RepeatAnimation::Never);
    }

    // свободное падение
    mascot.vel_y += GRAVITY * dt;
    mascot.pos_y += mascot.vel_y * dt;

    if mascot.pos_y >= ground {
        mascot.pos_y = ground;
        mascot.vel_y = 0.0;
        enter(&mut mascot, &mut commands, &lib, now, State::Idle);
    }
}

// Применяем позицию маскота к окну (единая точка управления окном).
fn apply_window_pos(mascot: Res<Mascot>, mut windows: Query<&mut Window>) {
    for mut win in &mut windows {
        win.position = WindowPosition::At(IVec2::new(mascot.pos_x as i32, mascot.pos_y as i32));
    }
}

fn setup_camera(mut commands: Commands) {
    // Кадрируем так, чтобы ступни (y=0) были у нижнего края окна.
    // При вертикальном FOV=45°: нижняя граница кадра на плоскости персонажа =
    // cam_y - cam_z*tan(22.5°). Приравниваем к 0 → cam_y = cam_z*0.4142.
    let cam_z = 2.4;
    let cam_y = cam_z * 0.4142; // ≈ 0.994 — ступни внизу кадра
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
    let anim_dir = Path::new(ASSETS_DIR).join(ANIM_SUBDIR);
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
fn on_vrma_loaded(
    trigger: On<LoadedVrma>,
    names: Query<&AnimName>,
    mut lib: ResMut<AnimLibrary>,
) {
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
    lib: Res<AnimLibrary>,
    mut mascot: ResMut<Mascot>,
    mut commands: Commands,
) {
    mascot.dragging = false;
    mascot.vel_y = 0.0;
    let now = time.elapsed_secs();

    // Считаем полное время падения от текущей высоты до земли (свободное
    // падение: t = sqrt(2·дистанция / g)). falling_end должен доиграть ровно
    // к приземлению, поэтому переключаемся на него за FALL_END_DURATION до конца.
    let distance = (ground_y() - mascot.pos_y).max(0.0);
    let total = (2.0 * distance / GRAVITY).sqrt();
    mascot.fall_switch = (total - FALL_END_DURATION).max(0.0); // «но не -N»
    mascot.fall_switched = false;

    enter(&mut mascot, &mut commands, &lib, now, State::Falling);
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
    // у drag/fall-состояний переход мгновенный (плавность мешает),
    // у остальных — кроссфейд 300 мс
    let transition = if new.is_drag() {
        Duration::ZERO
    } else {
        Duration::from_millis(300)
    };
    play(commands, lib, new.anim(), transition, RepeatAnimation::Forever);
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

// Главная машина состояний: смотрит на активность и переключает состояние/анимацию.
fn behavior_system(
    time: Res<Time>,
    lib: Res<AnimLibrary>,
    mut mascot: ResMut<Mascot>,
    mut commands: Commands,
) {
    // ждём, пока загрузится хотя бы idle
    if !lib.map.contains_key("idle") {
        return;
    }
    // во время таскания/падения поведением управляют drag/falling-системы
    if mascot.state.is_drag() {
        return;
    }
    let now = time.elapsed_secs();
    let idle = idle_seconds();
    let editor = foreground_exe().as_deref().map(is_editor).unwrap_or(false);

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
                } else if mascot.state == State::Idle
                    && mascot.walk_at.is_some_and(|t| now >= t)
                {
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
    mut commands: Commands,
) {
    let Ok(vrm) = vrms.single() else { return };
    let t = time.elapsed_secs();

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
    let target_yaw = if mascot.state == State::Walking {
        if mascot.walk_dir < 0.0 { -FRAC_PI_2 } else { FRAC_PI_2 }
    } else {
        0.0
    };
    let target = Quat::from_rotation_y(target_yaw);
    let s = (time.delta_secs() * 8.0).min(1.0); // скорость доворота
    for mut tf in &mut query {
        tf.rotation = tf.rotation.slerp(target, s);
    }
}

// Едва заметное дыхание: корень покачивается по Y.
fn breathing_system(time: Res<Time>, mut query: Query<&mut Transform, With<VrmRoot>>) {
    let t = time.elapsed_secs();
    for mut tf in &mut query {
        tf.translation.y = (t * 1.7).sin() * 0.005;
    }
}
