slint::include_modules!();
use slint::{Timer, TimerMode, Color};
use std::rc::Rc;
use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::thread;
use std::io::BufReader;
use serde::{Serialize, Deserialize};
use notify_rust::Notification;
use rodio::{Decoder, OutputStream, Sink};

#[derive(Serialize, Deserialize, Clone)]
struct AppConfig {
    work_m: i32,
    short_m: i32,
    long_m: i32,
    alarm_path: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { work_m: 25, short_m: 5, long_m: 15, alarm_path: "alarm.mp3".to_string() }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode { Work, ShortBreak, LongBreak }

#[derive(Clone)]
struct AppState {
    seconds_left: i32,
    mode: Mode,
    sessions_completed: i32,
    config: AppConfig,
}

fn load_config() -> AppConfig {
    fs::read_to_string("config.json")
        .and_then(|data| Ok(serde_json::from_str(&data).unwrap_or_default()))
        .unwrap_or_default()
}

fn save_config(config: &AppConfig) {
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = fs::write("config.json", json);
    }
}

fn play_alarm(path: String) {
    thread::spawn(move || {
        let (_stream, stream_handle) = match OutputStream::try_default() {
            Ok(s) => s,
            Err(_) => return,
        };
        let sink = match Sink::try_new(&stream_handle) {
            Ok(s) => s,
            Err(_) => return,
        };
        if let Ok(file) = fs::File::open(&path) {
            if let Ok(source) = Decoder::new(BufReader::new(file)) {
                sink.append(source);
                sink.sleep_until_end();
            }
        }
    });
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let ui_handle = ui.as_weak();
    let config = load_config();

    ui.set_work_setting(config.work_m.to_string().into());
    ui.set_short_break_setting(config.short_m.to_string().into());
    ui.set_long_break_setting(config.long_m.to_string().into());
    
    let alarm_name = Path::new(&config.alarm_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "alarm.mp3".to_string());
    ui.set_alarm_name(alarm_name.into());
    ui.set_timer_text(format!("{:02}:00", config.work_m).into());

    let state = Rc::new(RefCell::new(AppState {
        seconds_left: config.work_m * 60,
        mode: Mode::Work,
        sessions_completed: 0,
        config,
    }));

    let timer = Timer::default();

    let ui_copy = ui_handle.clone();
    let state_copy = state.clone();
    ui.on_settings_changed(move || {
        let ui = ui_copy.unwrap();
        let mut s = state_copy.borrow_mut();
        s.config.work_m = ui.get_work_setting().parse().unwrap_or(s.config.work_m);
        s.config.short_m = ui.get_short_break_setting().parse().unwrap_or(s.config.short_m);
        s.config.long_m = ui.get_long_break_setting().parse().unwrap_or(s.config.long_m);
        save_config(&s.config);
        
        if !ui.get_is_running() {
            s.seconds_left = match s.mode {
                Mode::Work => s.config.work_m * 60,
                Mode::ShortBreak => s.config.short_m * 60,
                Mode::LongBreak => s.config.long_m * 60,
            };
            ui.set_timer_text(format!("{:02}:00", s.seconds_left / 60).into());
        }
    });

    let ui_copy = ui_handle.clone();
    let state_copy = state.clone();
    ui.on_select_file(move || {
        if let Some(path) = rfd::FileDialog::new().add_filter("Audio", &["mp3", "wav", "ogg"]).pick_file() {
            let mut s = state_copy.borrow_mut();
            s.config.alarm_path = path.display().to_string();
            save_config(&s.config);
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            ui_copy.unwrap().set_alarm_name(name.into());
        }
    });

    ui.on_toggle_timer({
        let ui_copy = ui_handle.clone();
        move || { ui_copy.unwrap().set_is_running(!ui_copy.unwrap().get_is_running()); }
    });

    let ui_copy = ui_handle.clone();
    let state_copy = state.clone();
    ui.on_reset_timer(move || {
        let ui = ui_copy.unwrap();
        let mut s = state_copy.borrow_mut();
        s.mode = Mode::Work;
        s.seconds_left = s.config.work_m * 60;
        ui.set_is_running(false);
        ui.set_timer_text(format!("{:02}:00", s.config.work_m).into());
        ui.set_mode_text("FOCUS PHASE".into());
        ui.set_mode_color(Color::from_rgb_u8(243, 139, 168));
        ui.set_progress(1.0);
    });

    let ui_copy = ui_handle.clone();
    let state_copy = state.clone();
    timer.start(TimerMode::Repeated, std::time::Duration::from_secs(1), move || {
        let ui = match ui_copy.upgrade() { Some(ui) => ui, None => return };
        if !ui.get_is_running() { return; }

        let mut s = state_copy.borrow_mut();
        if s.seconds_left > 0 {
            s.seconds_left -= 1;
            ui.set_timer_text(format!("{:02}:{:02}", s.seconds_left / 60, s.seconds_left % 60).into());
            let total = match s.mode {
                Mode::Work => (s.config.work_m * 60) as f32,
                Mode::ShortBreak => (s.config.short_m * 60) as f32,
                Mode::LongBreak => (s.config.long_m * 60) as f32,
            };
            ui.set_progress(s.seconds_left as f32 / total);
        } else {
            play_alarm(s.config.alarm_path.clone());
            match s.mode {
                Mode::Work => {
                    s.sessions_completed += 1;
                    ui.set_sessions_count(s.sessions_completed);
                    if s.sessions_completed % 4 == 0 {
                        s.mode = Mode::LongBreak; s.seconds_left = s.config.long_m * 60;
                        ui.set_mode_text("LONG BREAK".into()); ui.set_mode_color(Color::from_rgb_u8(125, 207, 255));
                    } else {
                        s.mode = Mode::ShortBreak; s.seconds_left = s.config.short_m * 60;
                        ui.set_mode_text("SHORT BREAK".into()); ui.set_mode_color(Color::from_rgb_u8(158, 206, 106));
                    }
                    let _ = Notification::new().summary("Pomodoro").body("Phase Complete!").show();
                }
                _ => {
                    s.mode = Mode::Work; s.seconds_left = s.config.work_m * 60;
                    ui.set_mode_text("FOCUS PHASE".into()); ui.set_mode_color(Color::from_rgb_u8(243, 139, 168));
                    let _ = Notification::new().summary("Pomodoro").body("Get to Work!").show();
                }
            }
        }
    });

    ui.run()
}