//! Голосовое управление маскотом (оффлайн, Vosk + cpal).
//!
//! Конвейер: микрофон → Vosk (фоновый поток) → текст по каналу → парсер грамматики
//! → запуск приложения / сайта. Команда исполняется ТОЛЬКО если фраза содержит
//! wake word «маскот» (микрофон ловит фоновую речь — иначе ложные срабатывания).
//!
//! Две грамматики:
//!   «маскот открой <app>»            — запуск приложения по имени
//!   «маскот открой <site> в браузере» — открыть URL в браузере по умолчанию
//!
//! Пути приложений не хардкодятся: индекс строится из ярлыков Start Menu + PATH.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;

use bevy::prelude::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use serde::Deserialize;
use vosk::{DecodingState, Model, Recognizer};

const DEFAULT_MODEL: &str = "models/vosk-model-small-ru-0.22";
const CONFIG_PATH: &str = "voice.toml";

pub struct VoicePlugin;

/// Сообщение: голосовая команда принята и исполнена (для реакции маскота).
#[derive(Message)]
pub struct VoiceCommandAccepted;

impl Plugin for VoicePlugin {
    fn build(&self, app: &mut App) {
        let cfg = VoiceConfig::load();
        let index = build_app_index();
        let sites = build_site_index();
        info!(
            "voice: индекс приложений — {} записей, сайтов из истории — {}, wake word «{}»",
            index.0.len(),
            sites.0.len(),
            cfg.wake
        );

        let model_path = crate::resource_path(&cfg.model).to_string_lossy().into_owned();
        match start_listener(model_path) {
            Ok(rx) => {
                app.insert_resource(VoiceRx(Mutex::new(rx)))
                    .insert_resource(index)
                    .insert_resource(sites)
                    .insert_resource(cfg)
                    .add_message::<VoiceCommandAccepted>()
                    .add_systems(Update, voice_command_system);
            }
            Err(e) => {
                error!("voice: не удалось запустить распознавание: {e} — голос отключён");
            }
        }
    }
}

// ───────────────────────── конфиг ─────────────────────────

#[derive(Resource, Deserialize)]
struct VoiceConfig {
    /// Путь к Vosk-модели (язык распознавания).
    #[serde(default = "default_model")]
    model: String,
    /// Ключевое слово; команда исполняется только если фраза его содержит.
    wake: String,
    /// Глаголы-триггеры («открой», «запусти», …).
    verbs: Vec<String>,
    /// Слово-маркер браузера (основа): встретилось → цель трактуется как сайт.
    /// Сопоставляется по префиксу/нечётко («браузер»→«браузере»,«браузеры»).
    #[serde(default = "default_browser_word")]
    browser_word: String,
    /// Сказанное → имя для поиска в индексе («зед» → «zed»).
    #[serde(default)]
    alias: HashMap<String, String>,
    /// Сказанное → URL («ютуб» → https://youtube.com). Необязательно — фолбэк ниже.
    #[serde(default)]
    site: HashMap<String, String>,
    /// Шаблон поиска для сайтов вне таблицы. `{q}` заменяется на запрос.
    /// По умолчанию — DuckDuckGo `!ducky` (переход сразу на первый результат).
    #[serde(default = "default_search_url")]
    search_url: String,
}

impl VoiceConfig {
    fn load() -> Self {
        match std::fs::read_to_string(crate::resource_path(CONFIG_PATH)) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                error!("voice: ошибка разбора {CONFIG_PATH}: {e} — беру дефолт");
                Self::default()
            }),
            Err(_) => {
                warn!("voice: {CONFIG_PATH} не найден — беру дефолтную конфигурацию");
                Self::default()
            }
        }
    }
}

fn default_model() -> String {
    DEFAULT_MODEL.into()
}

fn default_browser_word() -> String {
    "браузер".into()
}

fn default_search_url() -> String {
    "https://www.google.com/search?q={q}".into()
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
            wake: "помощник".into(),
            verbs: vec!["открой".into(), "открыть".into(), "запусти".into()],
            browser_word: default_browser_word(),
            alias: HashMap::new(),
            site: HashMap::new(),
            search_url: default_search_url(),
        }
    }
}

// ───────────────────────── индекс приложений ─────────────────────────

/// Как запустить приложение.
#[derive(Clone, Debug)]
enum Launch {
    /// Ярлык Start Menu (.lnk) — открываем через `start`, Windows сам резолвит.
    Shortcut(PathBuf),
    /// Прямой exe (из PATH).
    Exe(PathBuf),
}

/// имя (lowercase) → способ запуска.
#[derive(Resource, Default)]
struct AppIndex(HashMap<String, Launch>);

/// Сканируем Start Menu (.lnk) + PATH. Пути нигде не хардкодим.
fn build_app_index() -> AppIndex {
    let mut map: HashMap<String, Launch> = HashMap::new();

    // 1. Ярлыки Start Menu — главный источник «человеческих» имён.
    for root in start_menu_dirs() {
        collect_shortcuts(&root, &mut map);
    }

    // 2. PATH — фолбэк для CLI/утилит (не перетираем уже найденные ярлыки).
    for name in ["zed", "code", "wt", "pwsh", "powershell", "explorer"] {
        if map.contains_key(name) {
            continue;
        }
        if let Some(path) = which(name) {
            map.insert(name.to_string(), Launch::Exe(path));
        }
    }

    AppIndex(map)
}

fn start_menu_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(pd) = std::env::var("ProgramData") {
        dirs.push(PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(ad) = std::env::var("APPDATA") {
        dirs.push(PathBuf::from(ad).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    dirs
}

/// Рекурсивно собираем *.lnk: ключ — имя файла без расширения в нижнем регистре.
fn collect_shortcuts(dir: &std::path::Path, out: &mut HashMap<String, Launch>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_shortcuts(&path, out);
        } else if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("lnk")).unwrap_or(false) {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                out.entry(stem.to_lowercase())
                    .or_insert_with(|| Launch::Shortcut(path.clone()));
            }
        }
    }
}

/// Поиск exe в PATH (как `where`), добавляя .exe при необходимости.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for cand in [format!("{name}.exe"), name.to_string()] {
            let full = dir.join(&cand);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

// ───────────────────────── индекс сайтов (история браузера) ─────────────────────────

/// метка домена (lowercase, напр. «payme») → полный origin для открытия («https://payme.uz»).
/// Строится из истории браузера: личный список сайтов, обновляется сам по мере сёрфинга.
#[derive(Resource, Default)]
struct SiteIndex(HashMap<String, String>);

/// Читаем историю браузеров (Edge/Chrome), ранжируем хосты по посещаемости.
fn build_site_index() -> SiteIndex {
    let mut hosts: HashMap<String, i64> = HashMap::new(); // host → суммарные визиты
    for db in browser_history_dbs() {
        read_history_hosts(&db, &mut hosts);
    }

    // host → метка (SLD). При коллизии метки берём более посещаемый host.
    let mut ranked: Vec<(String, i64)> = hosts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));

    let mut map: HashMap<String, String> = HashMap::new();
    for (host, _visits) in ranked.into_iter().take(300) {
        let label = domain_label(&host);
        if label.len() >= 2 {
            map.entry(label).or_insert_with(|| format!("https://{host}"));
        }
    }
    SiteIndex(map)
}

/// Пути к файлам истории установленных Chromium-браузеров (профиль Default).
fn browser_history_dbs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(local) = std::env::var("LOCALAPPDATA") else {
        return out;
    };
    let bases = [
        r"Microsoft\Edge\User Data",
        r"Google\Chrome\User Data",
        r"BraveSoftware\Brave-Browser\User Data",
        r"Yandex\YandexBrowser\User Data",
    ];
    for base in bases {
        let hist = PathBuf::from(&local).join(base).join(r"Default\History");
        if hist.is_file() {
            out.push(hist);
        }
    }
    out
}

/// Копируем залоченную БД во временный файл и читаем хосты с числом визитов.
fn read_history_hosts(db: &std::path::Path, hosts: &mut HashMap<String, i64>) {
    // Edge/Chrome держат History залоченной — работаем с копией.
    let tmp = std::env::temp_dir().join(format!(
        "mascot_hist_{}.db",
        db.parent().and_then(|p| p.to_str()).map(|s| s.len()).unwrap_or(0)
    ));
    if std::fs::copy(db, &tmp).is_err() {
        return;
    }
    let conn = match rusqlite::Connection::open_with_flags(
        &tmp,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(c) => c,
        Err(e) => {
            warn!("voice: история {db:?} не открылась: {e}");
            return;
        }
    };
    let mut stmt = match conn.prepare("SELECT url, visit_count FROM urls WHERE visit_count > 0") {
        Ok(s) => s,
        Err(_) => return,
    };
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    });
    if let Ok(rows) = rows {
        for (url, visits) in rows.flatten() {
            if let Some(host) = url_host(&url) {
                *hosts.entry(host).or_insert(0) += visits;
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
}

/// Хост из URL: «https://payme.uz/path» → «payme.uz». Только http(s).
fn url_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.split('@').next_back()?; // отбросить user:pass@
    let host = host.split(':').next()?; // отбросить порт
    let host = host.trim_start_matches("www.").to_lowercase();
    if host.is_empty() || !host.contains('.') {
        return None;
    }
    Some(host)
}

/// Метка домена (SLD) для сопоставления: «payme.uz»→«payme», «mail.google.com»→«google».
fn domain_label(host: &str) -> String {
    let parts: Vec<&str> = host.split('.').collect();
    // отбрасываем последний сегмент (TLD); метка — предпоследний значимый.
    if parts.len() >= 2 {
        parts[parts.len() - 2].to_string()
    } else {
        host.to_string()
    }
}

// ───────────────────────── распознавание ─────────────────────────

// Receiver не Sync — Mutex делает ресурс пригодным для Res.
#[derive(Resource)]
struct VoiceRx(Mutex<Receiver<String>>);

/// Поднимает аудиопоток + Vosk в отдельном потоке, возвращает канал финальных фраз.
fn start_listener(model_path: String) -> Result<Receiver<String>, String> {
    let (text_tx, text_rx) = mpsc::channel::<String>();

    std::thread::Builder::new()
        .name("voice-stt".into())
        .spawn(move || {
            if let Err(e) = run_listener(&model_path, text_tx) {
                eprintln!("voice-stt: {e}");
            }
        })
        .map_err(|e| format!("не удалось создать поток: {e}"))?;

    Ok(text_rx)
}

fn run_listener(model_path: &str, text_tx: mpsc::Sender<String>) -> Result<(), String> {
    eprintln!("voice: загружаю модель {model_path} … (для большой модели это ~10–15 с)");
    let t0 = std::time::Instant::now();
    let model = Model::new(model_path)
        .ok_or_else(|| format!("не удалось загрузить модель из {model_path}"))?;
    eprintln!("voice: модель загружена за {:.1} с", t0.elapsed().as_secs_f32());

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("нет устройства ввода (микрофона)")?;
    let default_cfg = device
        .default_input_config()
        .map_err(|e| format!("нет конфигурации ввода: {e}"))?;
    let sample_rate = default_cfg.sample_rate().0;
    let channels = default_cfg.channels() as usize;
    let sample_format = default_cfg.sample_format();
    let config: cpal::StreamConfig = default_cfg.into();

    let mut recognizer = Recognizer::new(&model, sample_rate as f32)
        .ok_or("не удалось создать Recognizer")?;
    recognizer.set_words(true);

    // callback → loop: сырые mono-i16
    let (sample_tx, sample_rx) = mpsc::channel::<Vec<i16>>();
    let err_fn = |e| eprintln!("voice-stt: ошибка аудиопотока: {e}");

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &_| {
                let _ = sample_tx.send(downmix_f32(data, channels));
            },
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &_| {
                let _ = sample_tx.send(downmix_i16(data, channels));
            },
            err_fn,
            None,
        ),
        other => return Err(format!("неподдерживаемый формат сэмплов: {other:?}")),
    }
    .map_err(|e| format!("не удалось открыть аудиопоток: {e}"))?;

    stream.play().map_err(|e| format!("не удалось запустить аудиопоток: {e}"))?;
    eprintln!("voice: ✓ готов к командам — говори «помощник …»");

    // Пока крутится цикл — поток (и stream) живы.
    for chunk in sample_rx {
        match recognizer.accept_waveform(&chunk) {
            Ok(DecodingState::Finalized) => {
                if let Some(res) = recognizer.result().single() {
                    let text = res.text.trim().to_string();
                    if !text.is_empty() && text_tx.send(text).is_err() {
                        break; // Bevy закрылся
                    }
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("voice-stt: accept_waveform: {e}"),
        }
    }
    Ok(())
}

fn downmix_f32(data: &[f32], channels: usize) -> Vec<i16> {
    data.chunks(channels)
        .map(|f| {
            let avg = f.iter().copied().sum::<f32>() / channels as f32;
            (avg.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
        })
        .collect()
}

fn downmix_i16(data: &[i16], channels: usize) -> Vec<i16> {
    data.chunks(channels)
        .map(|f| (f.iter().map(|&s| s as i32).sum::<i32>() / channels as i32) as i16)
        .collect()
}

// ───────────────────────── Bevy-система ─────────────────────────

fn voice_command_system(
    rx: Res<VoiceRx>,
    index: Res<AppIndex>,
    sites: Res<SiteIndex>,
    cfg: Res<VoiceConfig>,
    mut accepted: MessageWriter<VoiceCommandAccepted>,
) {
    let rx = rx.0.lock().expect("voice channel poisoned");
    while let Ok(text) = rx.try_recv() {
        // Полная расшифровка — только в debug (RUST_LOG=debug). По умолчанию тихо.
        debug!("voice heard: «{text}»");
        match parse_command(&text, &cfg, &index, &sites) {
            Some(Target::App(name, launch)) => {
                info!("voice: «{text}» → запускаю {name}");
                launch_app(&launch);
                accepted.write(VoiceCommandAccepted);
            }
            Some(Target::Url(url)) => {
                info!("voice: «{text}» → открываю {url}");
                open_url(&url);
                accepted.write(VoiceCommandAccepted);
            }
            None => {
                // Логируем только если был wake word — иначе это фоновая речь.
                if text.contains(&cfg.wake) {
                    info!("voice: «{text}» → команда не распознана");
                }
            }
        }
    }
}

enum Target {
    App(String, Launch),
    Url(String),
}

/// Разбор фразы по грамматике. None, если нет wake word или цель не найдена.
fn parse_command(
    text: &str,
    cfg: &VoiceConfig,
    index: &AppIndex,
    sites: &SiteIndex,
) -> Option<Target> {
    let lower = text.to_lowercase();
    // 1. wake word обязателен (нечёткая ловля: «маскот»/«москот»/«мас кот»…)
    let after_wake = strip_wake(&lower, &cfg.wake)?;

    // 2. глагол: срезаем, если есть. Необязателен в режиме браузера (см. ниже).
    let (rest, had_verb) = match strip_leading_verb(after_wake, &cfg.verbs) {
        Some(r) => (r.trim(), true),
        None => (after_wake.trim(), false),
    };
    if rest.is_empty() {
        return None;
    }

    // 3. имя цели = слова после глагола, без слова-браузера и связок (в/на/in/the).
    //    Если во фразе звучит «браузер» — это сайт (открываем дефолтным браузером),
    //    иначе приложение.
    const FILLERS: &[&str] = &["в", "во", "на", "и", "in", "the", "on", "a", "to"];
    let mut has_browser = false;
    let mut name_words = Vec::new();
    for w in rest.split_whitespace() {
        if is_browser_word(w, &cfg.browser_word) {
            has_browser = true;
        } else if !FILLERS.contains(&w) {
            name_words.push(w);
        }
    }
    let name = name_words.join(" ");
    if name.is_empty() {
        return None;
    }

    // browser → сайт (глагол не нужен: «помощник <название> в браузере» = веб-намерение).
    // Порядок: ручная таблица → история браузера (по скелету) → Google-поиск.
    if has_browser {
        let url = resolve_site(&name, cfg)
            .or_else(|| resolve_history_site(&name, sites))
            .unwrap_or_else(|| search_url(&name, cfg));
        return Some(Target::Url(url));
    }
    // Запуск приложения требует глагол — иначе фоновая речь с «помощник» спамила бы запуски.
    if !had_verb {
        return None;
    }
    // иначе приложение; фолбэк — только точный/нечёткий сайт из таблицы (без поиска,
    // чтобы мис-распознанное имя приложения не улетало в случайный веб-поиск).
    if let Some((app_name, launch)) = resolve_app(&name, cfg, index) {
        return Some(Target::App(app_name, launch));
    }
    resolve_site(&name, cfg).map(Target::Url)
}

/// URL поиска для сайта вне таблицы: подставляет URL-кодированный запрос в шаблон.
fn search_url(query: &str, cfg: &VoiceConfig) -> String {
    cfg.search_url.replace("{q}", &urlencode(query))
}

/// Перкодирование строки для URL-запроса: пробел → '+', не-ASCII-буквенно-цифровое → %XX.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b' ' => out.push('+'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Ищет wake word (нечётко) и возвращает текст после него.
/// Vosk может выдать «москот», «маскат», «мас кот» — принимаем близкие варианты.
fn strip_wake<'a>(s: &'a str, wake: &str) -> Option<&'a str> {
    // точное вхождение
    if let Some((_, after)) = s.split_once(wake) {
        return Some(after);
    }
    // по словам: первое слово в пределах Левенштейна ≤ 2 от wake
    let thr = if wake.chars().count() <= 4 { 1 } else { 2 };
    for word in s.split_whitespace() {
        if levenshtein(word, wake) <= thr {
            let end = word.as_ptr() as usize - s.as_ptr() as usize + word.len();
            return Some(&s[end..]);
        }
    }
    None
}

/// Слово — это «браузер»? По префиксу основы или нечётко («браузере»,«браузеры»).
fn is_browser_word(word: &str, base: &str) -> bool {
    word == base || word.starts_with(base) || levenshtein(word, base) <= 2
}

/// Возвращает текст после первого глагола-триггера (по словам).
fn strip_leading_verb<'a>(s: &'a str, verbs: &[String]) -> Option<&'a str> {
    for word in s.split_whitespace() {
        if verbs.iter().any(|v| v == word) {
            // позиция конца слова в исходной строке
            let word_end = word.as_ptr() as usize - s.as_ptr() as usize + word.len();
            return Some(&s[word_end..]);
        }
    }
    None
}

/// Сайт по фразе: alias-таблица site (с транслит/fuzzy фолбэком на ключи).
fn resolve_site(phrase: &str, cfg: &VoiceConfig) -> Option<String> {
    // Только точное совпадение: таблица — это ярлыки. Всё остальное уходит в
    // поиск-фолбэк (иначе нечёткость даёт ложные хиты: «почту»→«чат»).
    cfg.site.get(phrase).cloned()
}

/// Сайт из истории браузера по звуковому скелету (личный список → нечёткость надёжна).
fn resolve_history_site(phrase: &str, sites: &SiteIndex) -> Option<String> {
    let key = best_match(phrase, sites.0.keys().map(|k| k.as_str()))?;
    sites.0.get(key).cloned()
}

/// Приложение по фразе: alias → транслит → fuzzy по ключам индекса.
fn resolve_app(phrase: &str, cfg: &VoiceConfig, index: &AppIndex) -> Option<(String, Launch)> {
    // кандидаты имени: alias, транслит, сырое
    let mut candidates = Vec::new();
    if let Some(a) = cfg.alias.get(phrase) {
        candidates.push(a.clone());
    }
    candidates.push(translit(phrase));
    candidates.push(phrase.to_string());

    let keys: Vec<&str> = index.0.keys().map(|k| k.as_str()).collect();
    for cand in &candidates {
        if let Some(launch) = index.0.get(cand) {
            return Some((cand.clone(), launch.clone()));
        }
        if let Some(key) = best_match(cand, keys.iter().copied()) {
            return Some((key.to_string(), index.0[key].clone()));
        }
    }
    None
}

/// Лучшее совпадение из набора ключей: точное / подстрока / по расстоянию.
/// None, если ничего разумно близкого нет.
fn best_match<'a>(query: &str, keys: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let qs = skeleton(q);
    let mut best: Option<(&str, f32)> = None; // (key, score 0..1, выше — лучше)
    for key in keys {
        // канал 1: прямое сравнение (точное / подстрока / Левенштейн).
        let direct = if key == q {
            1.0
        } else if key.contains(q) || q.contains(key) {
            let (a, b) = (key.len().min(q.len()) as f32, key.len().max(q.len()) as f32);
            0.6 + 0.3 * (a / b)
        } else {
            let dist = levenshtein(q, key) as f32;
            let maxlen = q.chars().count().max(key.chars().count()) as f32;
            1.0 - dist / maxlen.max(1.0)
        };
        // канал 2: звуковой скелет (транслит + без гласных) — чинит гласные/окончания.
        let ks = skeleton(key);
        let skel = if qs.len() >= 3 && ks.len() >= 3 {
            if qs == ks {
                0.85
            } else {
                let d = levenshtein(&qs, &ks) as f32;
                let m = qs.chars().count().max(ks.chars().count()) as f32;
                (1.0 - d / m.max(1.0)) * 0.8
            }
        } else {
            0.0
        };
        let score = direct.max(skel);
        if best.map(|(_, s)| score > s).unwrap_or(true) {
            best = Some((key, score));
        }
    }
    best.filter(|&(_, s)| s >= 0.6).map(|(k, _)| k)
}

/// Звуковой «скелет»: транслит в латиницу, убрать гласные и схлопнуть повторы.
/// «телеграм»→«tlgrm», «steam»→«stm». Сводит варианты с разными гласными/окончаниями.
fn skeleton(s: &str) -> String {
    let t = translit(&s.to_lowercase());
    let mut out = String::new();
    let mut last = '\0';
    for c in t.chars() {
        if !c.is_ascii_alphanumeric() || "aeiouy".contains(c) {
            continue;
        }
        if c != last {
            out.push(c);
            last = c;
        }
    }
    out
}

/// Грубая транслитерация кириллица → латиница для матчинга («зед»→«zed»).
fn translit(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let lower = c.to_lowercase().next().unwrap_or(c);
        let mapped = match lower {
            'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d",
            'е' | 'ё' | 'э' => "e", 'ж' => "zh", 'з' => "z", 'и' => "i",
            'й' | 'ы' => "y", 'к' => "k", 'л' => "l", 'м' => "m", 'н' => "n",
            'о' => "o", 'п' => "p", 'р' => "r", 'с' => "s", 'т' => "t",
            'у' => "u", 'ф' => "f", 'х' => "h", 'ц' => "ts", 'ч' => "ch",
            'ш' => "sh", 'щ' => "sch", 'ъ' | 'ь' => "", 'ю' => "yu", 'я' => "ya",
            other => {
                out.push(other);
                continue;
            }
        };
        out.push_str(mapped);
    }
    out
}

/// Расстояние Левенштейна (по символам).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

// ───────────────────────── запуск ─────────────────────────

fn launch_app(launch: &Launch) {
    let res = match launch {
        // .lnk запускаем через `start` — Windows резолвит ярлык (рабочая папка, аргументы).
        Launch::Shortcut(p) => Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(p)
            .spawn(),
        Launch::Exe(p) => Command::new(p).spawn(),
    };
    if let Err(e) = res {
        error!("voice: не удалось запустить {launch:?}: {e}");
    }
}

fn open_url(url: &str) {
    // start с URL → браузер по умолчанию.
    if let Err(e) = Command::new("cmd").args(["/C", "start", "", url]).spawn() {
        error!("voice: не удалось открыть {url}: {e}");
    }
}
