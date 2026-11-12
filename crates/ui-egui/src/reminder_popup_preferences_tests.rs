// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The global reminder switch uses the existing saved UI preferences and owner consent.

use serde_json::json;

use crate::{MailApp, Services, UiState};

fn app() -> MailApp {
    let now = 1_791_993_600;
    let mut app = MailApp::new(mailcraft_engine::Session::at(mailcraft_engine::sample::mailbox(now, 0), now, 0), Services::default());
    app.autosave = false;
    app
}

#[test]
fn old_preferences_keep_automatic_reminder_popups_enabled() {
    for value in [json!({}), json!({"theme": "dark", "alertNewMail": false, "alertReminders": false})] {
        let old: UiState = serde_json::from_value(value).unwrap();
        let saved = serde_json::to_value(old).unwrap();
        assert_eq!(saved["showCalendarReminderPopups"], true);
        assert_eq!(saved["suppressedCalendarReminderPopups"], json!([]));
    }
}

#[test]
fn the_popup_switch_is_saved_without_changing_other_preferences_or_the_mailbox() {
    let mut app = app();
    let before = serde_json::to_value(&app.session.mailbox).unwrap();
    let rev = app.session.rev();
    assert_eq!(app.execute("ui.reminderPopups", json!({"value": false})).unwrap(), json!({"value": false}));
    let saved = serde_json::to_vec(&app.prefs()).unwrap();
    let restored: UiState = serde_json::from_slice(&saved).unwrap();
    let saved = serde_json::to_value(restored).unwrap();
    assert_eq!(saved["showCalendarReminderPopups"], false);
    assert_eq!(saved["alertNewMail"], true);
    assert_eq!(saved["alertReminders"], true, "the desktop-alert sub-switch is not rewritten");
    assert_eq!(serde_json::to_value(&app.session.mailbox).unwrap(), before);
    assert_eq!(app.session.rev(), rev);
}

#[test]
fn invalid_popup_switch_values_are_refused_without_changing_saved_preferences() {
    let mut app = app();
    let before = serde_json::to_value(app.prefs()).unwrap();
    for value in [json!(null), json!("false"), json!(0), json!([]), json!({})] {
        assert!(app.execute("ui.reminderPopups", json!({"value": value})).is_err());
        assert_eq!(serde_json::to_value(app.prefs()).unwrap(), before);
    }
}

#[test]
fn an_agent_needs_settings_permission_to_change_reminder_popups() {
    use mailcraft_engine::agent::{Access, Class, Verdict};
    let app = app();
    assert_eq!(mailcraft_engine::agent::access("ui.reminderPopups"), Some(Access::Owner(Class::Settings)));
    assert!(matches!(
        mailcraft_engine::agent::check(&app.session, "ui.reminderPopups", &json!({"value": false}), &[]),
        Verdict::Ask(Class::Settings, _)
    ));
}

#[test]
fn changing_the_popup_switch_between_frames_wakes_the_main_window() {
    let mut app = app();
    let ctx = egui::Context::default();
    for _ in 0..4 {
        ctx.run_ui(egui::RawInput::default(), |_| {}).drop_without_applying_deltas();
    }
    app.ctx = Some(ctx.clone());
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let heard = requests.clone();
    ctx.set_request_repaint_callback(move |info| heard.lock().unwrap().push((info.viewport_id, info.delay)));
    app.execute("ui.reminderPopups", json!({"value": false})).unwrap();
    assert!(requests.lock().unwrap().iter().any(|(viewport, delay)| *viewport == egui::ViewportId::ROOT && delay.is_zero()));
}

#[test]
fn the_calendar_checkbox_works_without_a_desktop_notification_service_in_both_themes() {
    use egui_kittest::kittest::Queryable;
    for theme in [crate::theme::Appearance::Light, crate::theme::Appearance::Dark] {
        let mut app = app();
        // Keep the sample's accounts (no setup dialog), but no due reminders over Options.
        app.session.mailbox.events = Default::default();
        app.auto_sync = false;
        app.ui.theme = theme;
        app.execute("ui.backstage", json!({"value": true, "page": "options"})).unwrap();
        let mut harness = egui_kittest::Harness::builder()
            .with_accessibility_check(false)
            .allow_missing_glyphs()
            .with_size(egui::vec2(1440.0, 3200.0))
            .build_ui_state(
                |ui, app: &mut MailApp| {
                    let ctx = ui.ctx().clone();
                    app.logic(&ctx);
                    app.ui(ui);
                },
                app,
            );
        harness.run_steps(4);
        assert!(harness.state().ui.show_calendar_reminder_popups);
        harness.get_by_label("Show calendar reminder pop-ups").click();
        harness.run_steps(4);
        assert!(!harness.state().ui.show_calendar_reminder_popups);
        harness.get_by_label("Show calendar reminder pop-ups").click();
        harness.run_steps(4);
        assert!(harness.state().ui.show_calendar_reminder_popups);
    }
}
