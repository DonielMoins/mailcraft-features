// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The Reminders window in a window of the system, while the main window has no passes (#771).

use std::sync::{Arc, Mutex};

use egui::{RawInput, ViewportCommand, pos2, vec2};
use mailcraft_engine::Session;
use serde_json::json;

use super::*;

// Wednesday 2026-10-14 16:00 UTC: the sample has reminders due then.
const NOW: i64 = 1_791_993_600;

fn app() -> MailApp {
    let mut app = MailApp::new(Session::at(mailcraft_engine::sample::mailbox(NOW, 0), NOW, 0), crate::Services::default());
    app.autosave = false;
    app
}

fn screen() -> Option<egui::Rect> {
    Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0)))
}

/// A pass of the main window, logic and layout, as eframe runs it while the window is on screen;
/// answers the Reminders window's own pass, when it has one.
fn main_pass(ctx: &egui::Context, app: &mut MailApp, time: &mut f64) -> Option<std::sync::Arc<egui::DeferredViewportUiCallback>> {
    *time += 1.0 / 60.0;
    let output = ctx.run_ui(RawInput { screen_rect: screen(), time: Some(*time), ..Default::default() }, |ui| {
        let ctx = ui.ctx().clone();
        app.logic(&ctx);
        app.ui(ui);
    });
    let paint = output.viewport_output.get(&viewport()).and_then(|v| v.viewport_ui_cb.clone());
    output.drop_without_applying_deltas();
    paint
}

/// The main window's logic alone, as eframe runs it while the window is minimized and the
/// compositor gives it no frames; answers the commands it sent to the Reminders window.
fn logic_only(ctx: &egui::Context, app: &mut MailApp) -> Vec<ViewportCommand> {
    let mut output = ctx.run_logic(&RawInput::default(), |ctx| app.logic(ctx));
    output.viewport_commands.remove(&viewport()).unwrap_or_default()
}

/// A pass of the Reminders window of its own, with `events`.
fn reminders_pass(ctx: &egui::Context, paint: &egui::DeferredViewportUiCallback, time: &mut f64, events: Vec<egui::Event>) {
    *time += 1.0 / 60.0;
    let mut input = RawInput {
        viewport_id: viewport(),
        screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(420.0, 300.0))),
        time: Some(*time),
        events,
        ..Default::default()
    };
    input.viewports.insert(viewport(), egui::ViewportInfo::default());
    ctx.run_ui(input, |ui| paint(ui)).drop_without_applying_deltas();
}

fn click(ctx: &egui::Context, paint: &egui::DeferredViewportUiCallback, time: &mut f64, pos: egui::Pos2) {
    reminders_pass(ctx, paint, time, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        let button = egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        reminders_pass(ctx, paint, time, vec![egui::Event::PointerMoved(pos), button]);
    }
}

/// Dismiss in the Reminders window is carried out by the main window's logic alone, with no
/// pass of the main window; and a window no longer wanted is put out of sight the same way.
#[test]
fn a_reminder_is_dismissed_while_the_main_window_has_no_passes() {
    let mut app = app();
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut time = 0.0;
    let mut paint = None;
    for _ in 0..6 {
        paint = main_pass(&ctx, &mut app, &mut time).or(paint);
    }
    let paint = paint.expect("the Reminders window has passes of its own");
    let before = app.reminders.showing.clone();
    assert!(before.len() >= 2, "the sample has reminders due: {before:?}");
    let first = before[0].clone();

    // The main window is minimized from here on: its logic runs, its layout doesn't.
    reminders_pass(&ctx, paint.as_ref(), &mut time, Vec::new());
    let row = lock(&app.reminders.shared).hits.iter().find(|(kind, id, _)| *kind == "reminder" && *id == first.event).map(|h| h.2);
    let row = row.expect("the reminder is laid out in its window");
    // Dismiss is at the row's right end.
    click(&ctx, paint.as_ref(), &mut time, pos2(row.max.x - 20.0, row.center().y));
    assert_eq!(lock(&app.reminders.shared).actions, [Action::Dismiss(first.event, first.start.iso())]);
    assert!(logic_only(&ctx, &mut app).is_empty());
    assert!(app.ui.dismissed_reminders.contains(&key_of(&first)));
    assert!(!app.reminders.showing.contains(&first));
    // What the window shows follows at once, for its next pass.
    assert_eq!(lock(&app.reminders.shared).rows.len(), before.len() - 1);
    assert!(lock(&app.reminders.shared).actions.is_empty(), "carried out once");

    // Dismiss All: nothing is left to show, and with no pass of the main window to take the
    // window away it is minimized, once.
    lock(&app.reminders.shared).actions.push(Action::DismissAll);
    assert!(logic_only(&ctx, &mut app).is_empty(), "the main window's next pass may yet take it away");
    assert!(app.reminders.showing.is_empty());
    assert_eq!(logic_only(&ctx, &mut app), [ViewportCommand::Minimized(true)]);
    assert!(logic_only(&ctx, &mut app).is_empty());

    // Back on screen, the main window lets it go.
    assert!(main_pass(&ctx, &mut app, &mut time).is_none());
    assert!(!app.reminders.minimized && !app.reminders.on_screen);
}

/// On screen, the main window's pass takes a window no longer wanted away itself: nothing is
/// minimized. And one that was minimized is made afresh when it is wanted again.
#[test]
fn a_window_on_screen_closes_without_being_minimized() {
    let mut app = app();
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut time = 0.0;
    for _ in 0..6 {
        main_pass(&ctx, &mut app, &mut time);
    }
    assert!(app.reminders.on_screen);
    lock(&app.reminders.shared).actions.push(Action::Close);
    // Closed with reminders showing: they are put off.
    assert!(main_pass(&ctx, &mut app, &mut time).is_none());
    assert!(app.reminders.showing.is_empty() && !app.reminders.snoozed.is_empty());
    for _ in 0..3 {
        assert!(logic_only(&ctx, &mut app).is_empty());
        main_pass(&ctx, &mut app, &mut time);
    }
    assert!(!app.reminders.minimized);

    // Asked for again after it was minimized: let go for a pass, then made afresh.
    app.reminders.minimized = true;
    app.reminders.asked_for = true;
    assert!(main_pass(&ctx, &mut app, &mut time).is_none());
    assert!(main_pass(&ctx, &mut app, &mut time).is_some());
    // Its Close with nothing due takes the request back.
    lock(&app.reminders.shared).actions.push(Action::Close);
    assert!(main_pass(&ctx, &mut app, &mut time).is_none());
    assert!(!app.reminders.asked_for);
}

/// The system's notifications, recorded: the reminder switch must keep them quiet.
#[derive(Clone, Default)]
struct Toasts(Arc<Mutex<Vec<crate::alerts::DesktopAlert>>>);

impl crate::alerts::AlertHost for Toasts {
    fn show(&self, alert: &crate::alerts::DesktopAlert) -> Result<(), String> {
        self.0.lock().unwrap().push(alert.clone());
        Ok(())
    }
}

fn due_keys(app: &MailApp) -> Vec<String> {
    due_reminders(&app.session, app.session.local_now()).iter().map(key_of).collect()
}

/// An app whose calendar has only: a daily series with yesterday's and today's occurrence
/// due, and one overdue appointment. Wednesday 2026-10-14 16:00 UTC.
fn due_app() -> (MailApp, u64, u64) {
    let mut app = app();
    let ids: Vec<u64> = app.session.mailbox.events.keys().copied().collect();
    for id in ids {
        app.execute("calendar.delete", json!({"id": id})).unwrap();
    }
    let daily = app
        .execute(
            "calendar.new",
            json!({"title": "Daily sync", "start": "2026-10-12T16:10", "end": "2026-10-12T16:30", "recurrence": {"freq": "daily"}, "reminder": 15}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let overdue = app
        .execute("calendar.new", json!({"title": "Overdue review", "start": "2026-10-14T13:00", "end": "2026-10-14T13:30", "reminder": 15}))
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    (app, daily, overdue)
}

fn attention(commands: &[ViewportCommand]) -> usize {
    commands.iter().filter(|c| matches!(c, ViewportCommand::RequestUserAttention(_))).count()
}

/// A pass of the main window, logic and layout; answers the Reminders window's own pass, when
/// it has one, and the commands sent to the main window.
fn main_pass_commands(
    ctx: &egui::Context,
    app: &mut MailApp,
    time: &mut f64,
) -> (Option<std::sync::Arc<egui::DeferredViewportUiCallback>>, Vec<ViewportCommand>) {
    *time += 1.0 / 60.0;
    let output = ctx.run_ui(RawInput { screen_rect: screen(), time: Some(*time), focused: false, ..Default::default() }, |ui| {
        let ctx = ui.ctx().clone();
        app.logic(&ctx);
        app.ui(ui);
    });
    let paint = output.viewport_output.get(&viewport()).and_then(|v| v.viewport_ui_cb.clone());
    let commands = output.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default();
    output.drop_without_applying_deltas();
    (paint, commands)
}

fn rows(app: &MailApp) -> Vec<u64> {
    lock(&app.reminders.shared).rows.iter().map(|r| r.reminder.event).collect()
}

/// RED tracer: with the switch off, automatic reminders stay quiet while remaining really due.
#[test]
fn switched_off_automatic_reminders_stay_quiet_while_still_being_due() {
    let (mut app, daily, overdue) = due_app();
    let heard = Toasts::default();
    app.services.alert = Some(Box::new(heard.clone()));
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut time = 0.0;
    let before = serde_json::to_value(&app.session.mailbox).unwrap();
    let rev = app.session.rev();

    assert_eq!(app.execute("ui.reminderPopups", json!({"value": false})).unwrap(), json!({"value": false}));
    assert!(!app.ui.show_calendar_reminder_popups);
    assert_eq!(app.ui.suppressed_calendar_reminder_popups, due_keys(&app));
    // What is due is still what is due: a recurring series has two occurrences here, one of
    // them overdue.
    assert_eq!(app.reminders.showing.len(), 3, "{:?}", app.reminders.showing);
    assert_eq!(app.reminders.showing.iter().filter(|r| r.event == daily).count(), 2);
    assert!(app.reminders.showing.iter().any(|r| r.event == overdue && r.minutes < 0));

    let mut paint = None;
    for _ in 0..4 {
        let (p, commands) = main_pass_commands(&ctx, &mut app, &mut time);
        assert_eq!(attention(&commands), 0, "no attention is asked for while switched off");
        paint = p.or(paint);
    }
    assert!(paint.is_none(), "no automatic Reminders window opens");
    assert!(!app.reminders.on_screen);
    assert!(heard.0.lock().unwrap().is_empty(), "no reminder desktop alert");
    assert!(app.ui.alert_new_mail && app.ui.alert_reminders, "mail alerts and their switch are untouched");
    // Nothing of the mailbox, its appointments or the sync state was changed.
    assert_eq!(serde_json::to_value(&app.session.mailbox).unwrap(), before);
    assert_eq!(app.session.rev(), rev);
    assert!(app.ui.dismissed_reminders.is_empty());
}

/// A pass already queued by the automatic window must not paint after the switch goes off, and
/// an action it queued must not run; the visible window closes normally, without a raise.
#[test]
fn a_window_queued_before_the_switch_went_off_paints_nothing_and_queued_actions_do_not_run() {
    let (mut app, _daily, _overdue) = due_app();
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut time = 0.0;
    let mut paint = None;
    for _ in 0..4 {
        paint = main_pass(&ctx, &mut app, &mut time).or(paint);
    }
    let paint = paint.expect("the automatic window opened");
    reminders_pass(&ctx, paint.as_ref(), &mut time, Vec::new());
    assert!(lock(&app.reminders.shared).visible);
    assert!(!lock(&app.reminders.shared).hits.is_empty(), "the window laid its rows out");

    // An action queued by the automatic window, and then the switch goes off.
    lock(&app.reminders.shared).actions.push(Action::DismissAll);
    set_popups(&mut app, false);
    {
        let s = lock(&app.reminders.shared);
        assert!(!s.visible, "a queued pass is no longer wanted");
        assert!(s.rows.is_empty(), "no stale rows are left to paint");
        assert!(s.hits.is_empty(), "no stale hit areas are left");
        assert!(s.actions.is_empty(), "the automatic window's queued action is invalidated");
    }
    // The callback captured before the switch paints nothing of what it captured and adds no
    // action.
    reminders_pass(&ctx, paint.as_ref(), &mut time, Vec::new());
    let s = lock(&app.reminders.shared);
    assert!(s.rows.is_empty() && s.hits.is_empty() && s.actions.is_empty());
    drop(s);
    assert!(app.ui.dismissed_reminders.is_empty(), "the dismissed action did not run");
    assert!(!app.reminders.showing.is_empty(), "the reminders are still really due");

    // And the visible automatic window is closed normally by the next main pass: no raise.
    let (next, commands) = main_pass_commands(&ctx, &mut app, &mut time);
    assert!(next.is_none());
    assert_eq!(attention(&commands), 0);
    assert!(!app.reminders.on_screen);
}

/// Off, then on: the backlog is suppressed before any announcement and does not reopen; a reminder
/// that falls due afterwards opens the window alone, without the suppressed backlog rows.
#[test]
fn switching_off_and_on_does_not_reopen_the_idle_backlog_and_a_new_reminder_still_opens() {
    let (mut app, daily, overdue) = due_app();
    let heard = Toasts::default();
    app.services.alert = Some(Box::new(heard.clone()));
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut time = 0.0;

    set_popups(&mut app, false);
    let backlog = due_keys(&app);
    assert_eq!(backlog.len(), 3);
    // Refreshing while off records the announcements it did not make, and keeps the backlog
    // suppressed.
    for _ in 0..2 {
        app.reminders.looked = 0.0;
        let (p, commands) = main_pass_commands(&ctx, &mut app, &mut time);
        assert!(p.is_none());
        assert_eq!(attention(&commands), 0);
    }
    assert!(heard.0.lock().unwrap().is_empty());
    assert!(backlog.iter().all(|k| app.reminders.announced.contains(k)), "bookkeeping so no repeated attempts");
    assert_eq!(app.ui.suppressed_calendar_reminder_popups, backlog);

    // One falls due while still off, and no tick follows before it is switched on: enabling fills
    // the current due set before any announcement, so it is backlog too.
    let late =
        app.execute("calendar.new", json!({"title": "Late arrival", "start": "2026-10-14T16:05", "reminder": 15})).unwrap()["id"].as_u64().unwrap();
    set_popups(&mut app, true);
    let late_key = app.reminders.showing.iter().find(|r| r.event == late).map(key_of).expect("the late reminder is due");
    assert!(app.ui.suppressed_calendar_reminder_popups.contains(&late_key));
    assert!(backlog.iter().all(|k| app.ui.suppressed_calendar_reminder_popups.contains(k)));
    for _ in 0..2 {
        app.reminders.looked = 0.0;
        let (p, commands) = main_pass_commands(&ctx, &mut app, &mut time);
        assert!(p.is_none(), "the backlog does not reopen by itself");
        assert_eq!(attention(&commands), 0);
    }
    assert!(heard.0.lock().unwrap().is_empty(), "no backlog desktop alerts");
    assert!(app.reminders.showing.iter().any(|r| r.event == daily));
    assert!(app.reminders.showing.iter().any(|r| r.event == overdue));
    assert!(app.reminders.showing.iter().any(|r| r.event == late));

    // A reminder that falls due after the switch went on is new, and opens the window alone: the
    // suppressed backlog is not a row of it.
    let fresh =
        app.execute("calendar.new", json!({"title": "Fresh reminder", "start": "2026-10-14T16:10", "reminder": 15})).unwrap()["id"].as_u64().unwrap();
    app.reminders.looked = 0.0;
    let (p, commands) = main_pass_commands(&ctx, &mut app, &mut time);
    assert!(attention(&commands) >= 1, "the new reminder asks for attention");
    assert!(p.is_some(), "the new reminder opens the automatic window");
    assert_eq!(rows(&app), [fresh], "only the unsuppressed reminder is shown automatically");
    let alerts = heard.0.lock().unwrap();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].kind, crate::alerts::AlertKind::Reminder);
    assert_eq!(alerts[0].opens, crate::alerts::Opens::Reminders);
    drop(alerts);
    assert!(app.reminders.showing.len() >= 5, "every reminder that is really due is kept: {:?}", app.reminders.showing);

    // Asking for the Reminders window by hand shows them all, backlog included.
    app.execute("view.reminders", json!({})).unwrap();
    assert!(app.reminders.asked_for);
    let (p, _) = main_pass_commands(&ctx, &mut app, &mut time);
    assert!(p.is_some());
    let shown: Vec<u64> = app.reminders.showing.iter().map(|r| r.event).collect();
    assert_eq!(rows(&app), shown, "the manual window shows all that is due");
}

/// The switch and the suppressed occurrences survive a restart through the saved UI settings, and
/// the backlog still does not reopen.
#[test]
fn the_switch_and_suppression_are_kept_across_a_restart_without_reopening() {
    let (mut first, _daily, _overdue) = due_app();
    set_popups(&mut first, false);
    let keys = first.ui.suppressed_calendar_reminder_popups.clone();
    assert_eq!(keys.len(), 3);
    let saved = serde_json::to_value(first.prefs()).unwrap();
    assert_eq!(saved["showCalendarReminderPopups"], false);
    assert_eq!(saved["suppressedCalendarReminderPopups"].as_array().map(Vec::len), Some(3));

    let heard = Toasts::default();
    let mut restarted = MailApp::new(Session::at(first.session.mailbox.clone(), NOW, 0), crate::Services::default());
    restarted.autosave = false;
    restarted.services.alert = Some(Box::new(heard.clone()));
    restarted.apply_prefs(serde_json::from_value(saved).unwrap());
    assert!(!restarted.ui.show_calendar_reminder_popups);
    assert_eq!(restarted.ui.suppressed_calendar_reminder_popups, keys);
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut time = 0.0;
    let mut paint = None;
    for _ in 0..4 {
        let (p, commands) = main_pass_commands(&ctx, &mut restarted, &mut time);
        assert_eq!(attention(&commands), 0);
        paint = p.or(paint);
    }
    assert!(paint.is_none(), "a suppressed reminder stays suppressed over a restart");
    assert!(heard.0.lock().unwrap().is_empty());
    assert!(restarted.reminders.showing.len() >= 3, "the occurrences are still due");
    assert_eq!(restarted.ui.suppressed_calendar_reminder_popups, keys, "retained while due");
}

#[test]
fn automatic_window_bulk_actions_leave_the_hidden_backlog_alone() {
    for action in [Action::DismissAll, Action::Snooze(5), Action::Close] {
        let (mut app, _daily, _overdue) = due_app();
        set_popups(&mut app, false);
        set_popups(&mut app, true);
        let earlier_backlog = due_keys(&app);
        let future = app
            .execute(
                "calendar.new",
                json!({
                    "title": "Future appointment", "start": "2026-10-14T16:45", "end": "2026-10-14T17:00", "reminder": 15
                }),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        app.session.clock.now = Some(NOW + 30 * 60);
        let ctx = egui::Context::default();
        ctx.set_embed_viewports(false);
        let mut time = 0.0;
        for _ in 0..4 {
            main_pass(&ctx, &mut app, &mut time);
        }
        // Advancing to 16:30 expires yesterday's occurrence at the engine's 24-hour boundary.
        // Bulk buttons must leave every still-due hidden row alone, not keep expired keys alive.
        let due_now = due_keys(&app);
        let backlog: Vec<String> = earlier_backlog.into_iter().filter(|key| due_now.contains(key)).collect();
        assert_eq!(backlog.len(), 2);
        assert_eq!(rows(&app), [future], "only the future appointment is on screen");
        lock(&app.reminders.shared).actions.push(action.clone());
        logic_only(&ctx, &mut app);
        for key in &backlog {
            assert!(!app.ui.dismissed_reminders.contains(key), "{action:?} dismissed an invisible reminder");
            assert!(!app.reminders.snoozed.contains_key(key), "{action:?} snoozed an invisible reminder");
        }
        assert_eq!(app.ui.dismissed_reminders.len() + app.reminders.snoozed.len(), 1);
        assert!(
            backlog.iter().all(|key| app.ui.suppressed_calendar_reminder_popups.contains(key)),
            "{action:?}: earlier {backlog:?}; due now {:?}; suppressed {:?}",
            due_keys(&app),
            app.ui.suppressed_calendar_reminder_popups
        );
    }
}

/// Suppression is pruned to what the engine still reports due, bounded to the engine's 100, kept
/// across a snooze, and dropped once none of the occurrences is due any more.
#[test]
fn suppression_is_pruned_to_what_is_due_and_bounded() {
    let (mut app, daily, _overdue) = due_app();
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut time = 0.0;
    set_popups(&mut app, false);
    let due = due_keys(&app);
    assert_eq!(due.len(), 3);
    // Keys that are no longer due, and far more than the engine's cap, are let go.
    for n in 0..150 {
        app.ui.suppressed_calendar_reminder_popups.push(format!("999999@2099-01-01T00:00:{n:02}"));
    }
    assert!(app.ui.suppressed_calendar_reminder_popups.len() > 100);
    app.reminders.looked = 0.0;
    let (p, _) = main_pass_commands(&ctx, &mut app, &mut time);
    assert!(p.is_none());
    assert_eq!(app.ui.suppressed_calendar_reminder_popups, due);
    assert!(app.ui.suppressed_calendar_reminder_popups.len() <= 100);

    // A snooze keeps the occurrence suppressed: it is still due, only not shown.
    crate::reminders::snooze(&mut app, None, 5);
    assert!(app.reminders.showing.is_empty());
    assert_eq!(app.ui.suppressed_calendar_reminder_popups, due, "retained across a snooze");
    app.reminders.snoozed.values_mut().for_each(|until| *until = 0.0);
    app.reminders.looked = 0.0;
    let (p, commands) = main_pass_commands(&ctx, &mut app, &mut time);
    assert!(p.is_none(), "the snoozed backlog does not reopen");
    assert_eq!(attention(&commands), 0);
    assert!(!app.reminders.showing.is_empty());
    assert_eq!(app.ui.suppressed_calendar_reminder_popups, due);

    // Once no occurrence of them is due any more, the keys are pruned. (The series would keep
    // producing occurrences, so it is deleted before the clock moves past every appointment.)
    app.execute("calendar.delete", json!({"id": daily})).unwrap();
    app.session.clock.now = Some(NOW + 40 * 86_400);
    app.reminders.looked = 0.0;
    let (p, _) = main_pass_commands(&ctx, &mut app, &mut time);
    assert!(p.is_none());
    assert!(app.ui.suppressed_calendar_reminder_popups.is_empty(), "stale keys are pruned");
    assert!(app.reminders.showing.is_empty());
}

/// View › Reminders and a click on an old alert still open the center while the switch is off, and
/// do not unsuppress the automatic backlog.
#[test]
fn a_manual_request_and_an_old_alert_click_still_open_the_center_with_the_switch_off() {
    let (mut app, _daily, _overdue) = due_app();
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut time = 0.0;
    set_popups(&mut app, false);
    assert_eq!(app.reminders.showing.len(), 3);
    // Let the fonts install, as the first passes of a run do, before the manual window is asked
    // for (with the switch off, none opens by itself).
    for _ in 0..3 {
        let (p, _) = main_pass_commands(&ctx, &mut app, &mut time);
        assert!(p.is_none());
    }

    // An alert shown before the switch went off is still clicked: it opens the center.
    let _ = ctx.run_logic(&RawInput::default(), |ctx| crate::alerts::open(&mut app, ctx, crate::alerts::Opens::Reminders));
    assert!(app.reminders.asked_for);
    let (p, commands) = main_pass_commands(&ctx, &mut app, &mut time);
    assert!(p.is_some());
    assert_eq!(attention(&commands), 0);
    let shown: Vec<u64> = app.reminders.showing.iter().map(|r| r.event).collect();
    assert_eq!(rows(&app), shown);

    // View › Reminders likewise.
    app.reminders.asked_for = false;
    let (p, _) = main_pass_commands(&ctx, &mut app, &mut time);
    assert!(p.is_none());
    app.execute("view.reminders", json!({})).unwrap();
    assert!(app.reminders.asked_for);
    let (p, _) = main_pass_commands(&ctx, &mut app, &mut time);
    assert!(p.is_some());
    assert_eq!(rows(&app), shown);
    assert_eq!(app.ui.suppressed_calendar_reminder_popups.len(), 3, "a manual look does not unsuppress the backlog");
}
