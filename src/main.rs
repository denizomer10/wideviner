// SPDX-License-Identifier: MIT
//! wideviner - Widevine CDM installer with a Slint user interface.

slint::include_modules!();

mod browser;
mod chromium;
mod constants;
mod crx;
mod deb;
mod firefox;
mod install;
mod sysinfo;
mod widevine;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;

use slint::{ModelRc, Timer, TimerMode, VecModel};

use browser::Browser;
use constants::{APP_TAGLINE, APP_VERSION};
use sysinfo::SystemInfo;
use widevine::Event;

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Install,
    Update,
    Remove,
}

impl Op {
    fn title(self) -> &'static str {
        match self {
            Op::Install => "Installing Widevine",
            Op::Update => "Updating Widevine",
            Op::Remove => "Removing Widevine",
        }
    }
}

/// Messages sent from the worker threads to the user interface.
enum UiMessage {
    Browsers(Vec<Browser>, String),
    Log(String),
    Detail(String),
    Progress { done: u64, total: u64 },
    Finished { ok: bool, message: String },
}

#[derive(Default)]
struct Shared {
    browsers: Vec<Browser>,
    selected: usize,
    busy: bool,
    log: String,
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    let progress = ProgressWindow::new()?;

    ui.set_app_version(APP_VERSION.into());

    let (tx, rx) = channel::<UiMessage>();
    let shared = Rc::new(RefCell::new(Shared::default()));

    // System information is quick to collect and can be done synchronously.
    let si = sysinfo::detect();
    set_system_rows(&ui, &si);

    ui.set_status(format!("{} — scanning for browsers ...", APP_TAGLINE).into());
    spawn_scan(si.clone(), tx.clone());

    // -- callbacks ----------------------------------------------------------
    {
        let tx = tx.clone();
        let si = si.clone();
        ui.on_refresh(move || {
            spawn_scan(si.clone(), tx.clone());
        });
    }

    {
        let tx = tx.clone();
        let si = si.clone();
        let shared = shared.clone();
        let ui_weak = ui.as_weak();
        let progress_weak = progress.as_weak();
        ui.on_install(move || {
            start_operation(Op::Install, &tx, &si, &shared, &ui_weak, &progress_weak);
        });
    }
    {
        let tx = tx.clone();
        let si = si.clone();
        let shared = shared.clone();
        let ui_weak = ui.as_weak();
        let progress_weak = progress.as_weak();
        ui.on_update(move || {
            start_operation(Op::Update, &tx, &si, &shared, &ui_weak, &progress_weak);
        });
    }
    {
        let tx = tx.clone();
        let si = si.clone();
        let shared = shared.clone();
        let ui_weak = ui.as_weak();
        let progress_weak = progress.as_weak();
        ui.on_remove(move || {
            start_operation(Op::Remove, &tx, &si, &shared, &ui_weak, &progress_weak);
        });
    }

    {
        let weak = progress.as_weak();
        progress.on_dismiss(move || {
            if let Some(window) = weak.upgrade() {
                let _ = window.hide();
            }
        });
    }

    // -- drain worker messages ---------------------------------------------
    let timer = Timer::default();
    {
        let ui_weak = ui.as_weak();
        let progress_weak = progress.as_weak();
        let shared = shared.clone();
        timer.start(TimerMode::Repeated, Duration::from_millis(80), move || {
            while let Ok(message) = rx.try_recv() {
                handle_message(message, &ui_weak, &progress_weak, &shared);
            }
        });
    }

    let result = ui.run();
    // Keep the timer alive until the event loop ends.
    drop(timer);
    result
}

fn handle_message(
    message: UiMessage,
    ui_weak: &slint::Weak<MainWindow>,
    progress_weak: &slint::Weak<ProgressWindow>,
    shared: &Rc<RefCell<Shared>>,
) {
    match message {
        UiMessage::Browsers(browsers, note) => {
            let selected = {
                let mut state = shared.borrow_mut();
                state.browsers = browsers.clone();
                if state.selected >= browsers.len() {
                    state.selected = 0;
                }
                state.selected
            };
            if let Some(ui) = ui_weak.upgrade() {
                set_browser_rows(&ui, &browsers, selected);
                ui.set_status(note.into());
            }
        }
        UiMessage::Log(line) => {
            let text = {
                let mut state = shared.borrow_mut();
                state.log.push_str(&line);
                state.log.push('\n');
                state.log.clone()
            };
            if let Some(window) = progress_weak.upgrade() {
                window.set_log_text(text.into());
            }
        }
        UiMessage::Detail(text) => {
            if let Some(window) = progress_weak.upgrade() {
                window.set_indeterminate(false);
                window.set_detail(text.into());
            }
        }
        UiMessage::Progress { done, total } => {
            if let Some(window) = progress_weak.upgrade() {
                if total > 0 {
                    window.set_indeterminate(false);
                    window.set_progress(done as f32 / total as f32);
                }
            }
        }
        UiMessage::Finished { ok, message } => {
            {
                let mut state = shared.borrow_mut();
                state.busy = false;
                state.log
                    .push_str(if ok { "\nFinished.\n" } else { "\nFailed.\n" });
            }
            if let Some(window) = progress_weak.upgrade() {
                window.set_indeterminate(false);
                if ok {
                    window.set_progress(1.0);
                }
                window.set_finished(true);
                window.set_heading(if ok { "Done" } else { "Failed" }.into());
                window.set_detail(message.clone().into());
            }
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_busy(false);
                ui.set_status(message.into());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
fn set_system_rows(ui: &MainWindow, si: &SystemInfo) {
    let rows: Vec<InfoRow> = sysinfo::report(si)
        .into_iter()
        .map(|(label, value)| InfoRow {
            label: label.into(),
            value: value.into(),
        })
        .collect();
    ui.set_system_rows(ModelRc::new(VecModel::from(rows)));
}

fn set_browser_rows(ui: &MainWindow, browsers: &[Browser], selected: usize) {
    let rows: Vec<BrowserRow> = browsers
        .iter()
        .map(|browser| {
            let (state, kind) = browser.state();
            BrowserRow {
                title: browser.describe().into(),
                subtitle: browser.detail().into(),
                state: state.into(),
                state_kind: kind,
            }
        })
        .collect();
    ui.set_browsers(ModelRc::new(VecModel::from(rows)));
    ui.set_selected_index(selected as i32);
}

// ---------------------------------------------------------------------------
// Background work
// ---------------------------------------------------------------------------
fn spawn_scan(si: SystemInfo, tx: Sender<UiMessage>) {
    std::thread::spawn(move || {
        // `deep` walking is slow; the plain scan finds the usual installations.
        let browsers = browser::discover(&si, false);
        let _ = tx.send(UiMessage::Browsers(browsers, scan_note(&si, &tx)));
    });
}

fn scan_note(_si: &SystemInfo, _tx: &Sender<UiMessage>) -> String {
    "Scan finished.".to_string()
}

fn start_operation(
    op: Op,
    tx: &Sender<UiMessage>,
    si: &SystemInfo,
    shared: &Rc<RefCell<Shared>>,
    ui_weak: &slint::Weak<MainWindow>,
    progress_weak: &slint::Weak<ProgressWindow>,
) {
    let browser = {
        let state = shared.borrow();
        if state.busy {
            return;
        }
        match state.browsers.get(state.selected) {
            Some(browser) => browser.clone(),
            None => return,
        }
    };

    {
        let mut state = shared.borrow_mut();
        state.busy = true;
        state.log = format!("> {}\n", op.title());
    }

    if let Some(ui) = ui_weak.upgrade() {
        ui.set_busy(true);
        ui.set_status(format!("{} ...", op.title()).into());
    }
    if let Some(window) = progress_weak.upgrade() {
        window.set_heading(op.title().into());
        window.set_detail(browser.describe().into());
        window.set_progress(0.0);
        window.set_indeterminate(true);
        window.set_log_text(format!("> {}\n", op.title()).into());
        window.set_finished(false);
        let _ = window.show();
    }

    let tx = tx.clone();
    let si = si.clone();
    std::thread::spawn(move || {
        let reporter_tx = tx.clone();
        let result = run_operation(op, &browser, &si, &reporter_tx);
        let (ok, message) = match result {
            Ok(message) => (true, message),
            Err(err) => (false, err),
        };
        let _ = tx.send(UiMessage::Log(if ok {
            "Done.".to_string()
        } else {
            format!("Error: {}", message)
        }));
        let _ = tx.send(UiMessage::Finished {
            ok,
            message: message.clone(),
        });

        // Refresh the browser list so the UI reflects the new state.
        let browsers = browser::discover(&si, false);
        let note = if ok {
            message
        } else {
            format!("Failed: {}", message)
        };
        let _ = tx.send(UiMessage::Browsers(browsers, note));
    });
}

fn run_operation(
    op: Op,
    browser: &Browser,
    si: &SystemInfo,
    tx: &Sender<UiMessage>,
) -> Result<String, String> {
    let sender = tx.clone();
    let reporter = move |event: Event| match event {
        Event::Info(message) => {
            let _ = sender.send(UiMessage::Detail(message.clone()));
            let _ = sender.send(UiMessage::Log(format!("> {}", message)));
        }
        Event::Warn(message) => {
            let _ = sender.send(UiMessage::Log(format!("! {}", message)));
        }
        Event::Progress { done, total } => {
            let _ = sender.send(UiMessage::Progress { done, total });
            if total > 0 {
                let _ = sender.send(UiMessage::Detail(format!(
                    "{:.1} MiB / {:.1} MiB",
                    done as f64 / 1_048_576.0,
                    total as f64 / 1_048_576.0
                )));
            }
        }
    };

    match op {
        Op::Remove => remove_browser(browser, si, &reporter),
        Op::Install | Op::Update => install_browser(op, browser, si, &reporter),
    }
}

fn install_browser(
    op: Op,
    browser: &Browser,
    si: &SystemInfo,
    reporter: widevine::Reporter,
) -> Result<String, String> {
    let payload = widevine::fetch_cdm(si, None, reporter, false, None)?;
    reporter(Event::Info(format!(
        "Widevine {} obtained from Google ({} download)",
        payload.version, payload.source
    )));

    if op == Op::Update {
        let installed = browser.widevine_version();
        if !installed.is_empty()
            && !payload.version.is_empty()
            && widevine::version_key(&payload.version) <= widevine::version_key(installed)
        {
            return Ok(format!("Widevine is already up to date ({}).", installed));
        }
    }

    if let Browser::Chromium(chromium) = browser {
        if widevine::build_supports_widevine(&chromium.executable()) == Some(false) {
            reporter(Event::Warn(
                "This browser build does not appear to include Widevine support; the CDM \
                 is installed but the browser may not load it."
                    .into(),
            ));
        }
    }

    let result = match browser {
        Browser::Chromium(b) => install::install_chromium(b, &payload, si, reporter)?,
        Browser::Firefox(b) => install::install_firefox(b, &payload, si, reporter)?,
    };

    let mut message = format!(
        "Installed Widevine {} ({}) in {}",
        result.version,
        result.mode,
        result.path.display()
    );
    if let Some(hint) = &result.hint_file {
        message.push_str(&format!("  ·  {}", hint.display()));
    }
    Ok(message)
}

fn remove_browser(
    browser: &Browser,
    si: &SystemInfo,
    reporter: widevine::Reporter,
) -> Result<String, String> {
    let removed = match browser {
        Browser::Chromium(b) => install::remove_chromium(b, si, reporter)?,
        Browser::Firefox(b) => install::remove_firefox(b, reporter)?,
    };
    let list = removed
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!("Removed Widevine ({})", list))
}
