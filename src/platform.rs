// Детекция активности пользователя через WinAPI:
//   - сколько секунд назад был последний ввод (мышь/клавиатура)
//   - имя exe активного окна (для распознавания «кодит»)
//
// Детект музыки:
//   Windows  — WASAPI loopback через cpal: измеряем RMS системного аудиовыхода.
//              Работает с любым источником (браузер, Spotify, Yandex Music и т.д.).
//   macOS    — TODO: использовать приватный фреймворк MediaRemote через objc-крейт.
//              MediaRemote виден в виджете «Сейчас играет» macOS и охватывает все плееры.
//              Ссылки: https://github.com/nickcoutsos/mediaremote-bindings
//              Функция start_music_detector() возвращает заглушку (false) до реализации.

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{CloseHandle, RECT};
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, SetCursorPos,
        SystemParametersInfoW, SPI_GETWORKAREA,
    };

    /// Позиция курсора в экранных координатах (пиксели).
    pub fn cursor_pos() -> (i32, i32) {
        unsafe {
            let mut p = POINT { x: 0, y: 0 };
            if GetCursorPos(&mut p) == 0 {
                (0, 0)
            } else {
                (p.x, p.y)
            }
        }
    }

    /// Переместить курсор (кража курсора в режиме хауса).
    pub fn set_cursor_pos(x: i32, y: i32) {
        unsafe {
            SetCursorPos(x, y);
        }
    }

    /// Рабочая область экрана (без панели задач): (left, top, right, bottom) в пикселях.
    /// bottom — это верхняя граница таскбара (если он снизу).
    pub fn work_area() -> (i32, i32, i32, i32) {
        unsafe {
            let mut r = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            let ok = SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                (&mut r) as *mut RECT as *mut core::ffi::c_void,
                0,
            );
            if ok == 0 {
                (0, 0, 1920, 1040)
            } else {
                (r.left, r.top, r.right, r.bottom)
            }
        }
    }

    /// Сколько секунд прошло с последнего ввода пользователя.
    pub fn idle_seconds() -> f32 {
        unsafe {
            let mut lii = LASTINPUTINFO {
                cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
                dwTime: 0,
            };
            if GetLastInputInfo(&mut lii) == 0 {
                return 0.0;
            }
            let now = GetTickCount();
            now.wrapping_sub(lii.dwTime) as f32 / 1000.0
        }
    }

    /// Имя exe активного окна в нижнем регистре (например, "zed.exe"). None если не удалось.
    pub fn foreground_exe() -> Option<String> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 {
                return None;
            }
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buf = [0u16; 260];
            let mut size = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            let full = String::from_utf16_lossy(&buf[..size as usize]);
            let name = full
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or("")
                .to_lowercase();
            if name.is_empty() {
                None
            } else {
                Some(name)
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn idle_seconds() -> f32 {
        0.0
    }
    pub fn foreground_exe() -> Option<String> {
        None
    }
    pub fn work_area() -> (i32, i32, i32, i32) {
        (0, 0, 1920, 1040)
    }
    pub fn cursor_pos() -> (i32, i32) {
        (0, 0)
    }
    pub fn set_cursor_pos(_x: i32, _y: i32) {}
}

pub use imp::{cursor_pos, foreground_exe, idle_seconds, set_cursor_pos, work_area};

/// Считается ли активное окно редактором кода / IDE / терминалом.
pub fn is_editor(exe: &str) -> bool {
    const EDITORS: &[&str] = &[
        "zed.exe",
        "code.exe",
        "cursor.exe",
        "devenv.exe",
        "rustrover64.exe",
        "idea64.exe",
        "clion64.exe",
        "pycharm64.exe",
        "sublime_text.exe",
        "notepad++.exe",
        "windowsterminal.exe",
        "wezterm-gui.exe",
        "alacritty.exe",
        "powershell.exe",
        "pwsh.exe",
        "cmd.exe",
    ];
    EDITORS.contains(&exe)
}

/// Запускает фоновый поток детекта музыки.
/// Возвращает `Arc<AtomicBool>` — `true` пока играет звук выше порога.
///
/// Windows: WASAPI loopback (cpal) — захватывает системный аудиовыход,
///          работает с любым плеером (браузер, Spotify, Yandex Music).
/// macOS:   заглушка (всегда false).
///          TODO: реализовать через MediaRemote (objc-крейт).
///          Фреймворк приватный, но стабильный — используется виджетом
///          «Сейчас играет» macOS. Охватывает браузер, Spotify, Yandex Music.
///          Пример биндингов: https://github.com/nickcoutsos/mediaremote-bindings
pub fn start_music_detector() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    #[cfg(windows)]
    {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let playing = Arc::new(AtomicBool::new(false));
        let flag = playing.clone();

        std::thread::spawn(move || {
            let host = cpal::default_host();
            let device = match host.default_output_device() {
                Some(d) => d,
                None => return,
            };
            let config = match device.default_output_config() {
                Ok(c) => c,
                Err(_) => return,
            };

            // Сглаженный RMS — экспоненциальное скользящее среднее по блокам.
            // Порог 0.008 ≈ тихая музыка/речь; ниже — фоновый шум карты.
            const THRESHOLD: f32 = 0.008;
            const ALPHA: f32 = 0.05; // скорость сглаживания (меньше = медленнее)

            let smooth = std::sync::Arc::new(std::sync::Mutex::new(0.0f32));
            let smooth2 = smooth.clone();

            let stream = device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    let rms = (data.iter().map(|s| s * s).sum::<f32>() / data.len() as f32).sqrt();
                    let mut s = smooth2.lock().unwrap();
                    *s = *s * (1.0 - ALPHA) + rms * ALPHA;
                    flag.store(*s > THRESHOLD, Ordering::Relaxed);
                },
                |err| eprintln!("music detector error: {err}"),
                None,
            );

            if let Ok(stream) = stream {
                stream.play().ok();
                // держим поток живым вечно
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(60));
                }
            }
        });

        playing
    }

    #[cfg(not(windows))]
    {
        // macOS / Linux: заглушка
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))
    }
}
