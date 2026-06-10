//! Изолированный STT-прототип: микрофон (cpal) → Vosk → печать распознанного текста.
//!
//! Запуск (PowerShell, из корня проекта):
//!   $env:PATH = "$PWD\vendor\vosk-win64-0.3.45;$env:PATH"
//!   cargo run --example voice_probe
//!
//! Говори по-русски. Партиал печатается в одну строку, финальная фраза — с «=>».
//! Ctrl+C для выхода.

use std::sync::mpsc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use vosk::{DecodingState, Model, Recognizer};

const MODEL_PATH: &str = "models/vosk-model-small-ru-0.22";

fn main() {
    // --- модель ---
    let model = Model::new(MODEL_PATH)
        .unwrap_or_else(|| panic!("не удалось загрузить модель из {MODEL_PATH}"));

    // --- микрофон ---
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .expect("нет устройства ввода (микрофона)");
    println!("микрофон: {}", device.name().unwrap_or_else(|_| "<?>".into()));

    let default_cfg = device
        .default_input_config()
        .expect("нет дефолтной конфигурации ввода");
    let sample_rate = default_cfg.sample_rate().0;
    let channels = default_cfg.channels() as usize;
    let sample_format = default_cfg.sample_format();
    let config: cpal::StreamConfig = default_cfg.into();
    println!("формат: {sample_rate} Hz, {channels} канал(ов), {sample_format:?}");

    // --- распознаватель (на реальном sample rate устройства) ---
    let mut recognizer = Recognizer::new(&model, sample_rate as f32)
        .expect("не удалось создать Recognizer");
    recognizer.set_words(true);

    // Аудио-callback шлёт mono-i16 в главный поток через канал.
    let (tx, rx) = mpsc::channel::<Vec<i16>>();
    let err_fn = |e| eprintln!("ошибка аудиопотока: {e}");

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &_| {
                let mono = downmix_f32(data, channels);
                let _ = tx.send(mono);
            },
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &_| {
                let mono = downmix_i16(data, channels);
                let _ = tx.send(mono);
            },
            err_fn,
            None,
        ),
        other => panic!("неподдерживаемый формат сэмплов: {other:?}"),
    }
    .expect("не удалось открыть аудиопоток");

    stream.play().expect("не удалось запустить аудиопоток");
    println!("\nслушаю… (говори по-русски, Ctrl+C для выхода)\n");

    // --- цикл распознавания в главном потоке ---
    let mut last_partial = String::new();
    for chunk in rx {
        let state = match recognizer.accept_waveform(&chunk) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("accept_waveform: {e}");
                continue;
            }
        };
        match state {
            DecodingState::Finalized => {
                if let Some(res) = recognizer.result().single() {
                    let text = res.text.trim();
                    if !text.is_empty() {
                        // стираем строку партиала и печатаем финал
                        print!("\r{:width$}\r", "", width = last_partial.len() + 12);
                        println!("=> {text}");
                        last_partial.clear();
                    }
                }
            }
            DecodingState::Running => {
                let partial = recognizer.partial_result().partial.trim().to_string();
                if !partial.is_empty() && partial != last_partial {
                    print!("\r… {partial}    ");
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    last_partial = partial;
                }
            }
            DecodingState::Failed => {}
        }
    }
}

/// Усредняем каналы в моно (f32 → i16).
fn downmix_f32(data: &[f32], channels: usize) -> Vec<i16> {
    data.chunks(channels)
        .map(|frame| {
            let avg = frame.iter().copied().sum::<f32>() / channels as f32;
            (avg.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
        })
        .collect()
}

/// Усредняем каналы в моно (i16 → i16).
fn downmix_i16(data: &[i16], channels: usize) -> Vec<i16> {
    data.chunks(channels)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / channels as i32) as i16
        })
        .collect()
}
