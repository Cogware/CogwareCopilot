// SPDX-License-Identifier: GPL-3.0-only
use crate::App;

fn app() -> App {
    let mut app = App::new(None, &[]);
    let rig = copilot::rig::parse(
        r#"{
        "modes":["normal"], "displays":[{
            "name":"Cluster","node":"0x01","gpio":8,"scenes":{"normal":"a.scene"},
            "pins":[
                {"pin":1,"dir":"out","gauge":"RPM","above":5000},
                {"pin":2,"dir":"out","gauge":"AFR_PRI","above":14.7},
                {"pin":3,"dir":"out","gauge":"AFR_TARGET","above":14.7},
                {"pin":4,"dir":"out","gauge":"CLNT","above":100}
            ]
        }]
    }"#,
    )
    .unwrap();
    app.driver.sync(
        &copilot::widget::Tree::new(copilot::Rect::new(0, 0, 64, 64)),
        &[],
        &[],
        &[&rig.displays[0].pins],
    );
    for channel in app.driver.gauges.values_mut() {
        channel.min = 0.0;
        channel.max = 1.0;
    }
    app
}

fn frame(
    app: &mut App,
    ctx: &egui::Context,
    options: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if options {
                    app.playback_options(ui);
                } else {
                    app.values_pane(ui);
                }
            });
        },
    )
}

fn click(app: &mut App, ctx: &egui::Context, options: bool, label: &str) {
    let output = frame(app, ctx, options, Vec::new());
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing control {label}"));
    for pressed in [true, false] {
        let _ = frame(
            app,
            ctx,
            options,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn fill_button_populates_paused_values_without_clamping_or_editing_the_document() {
    let ctx = egui::Context::default();
    let mut app = app();
    let text = app.text.clone();
    click(&mut app, &ctx, false, "Fill preview values");
    assert!(app.driver.enabled);
    assert!(!app.driver.driving && !app.driver.sweep && !app.driver.to_car);
    assert!(app.driver.gauges["RPM"].value > 500.0);
    assert!(app.driver.gauges["AFR_PRI"].value > 10.0);
    let values = app.driver.gauges.clone();
    let _ = frame(&mut app, &ctx, false, Vec::new());
    assert_eq!(
        app.driver.gauges, values,
        "rendering must not clamp a snapshot to a widget's range"
    );
    assert_eq!(app.text, text);
}

#[test]
fn live_recorded_values_are_read_only_not_clamped_by_preview_sliders() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.driver.enabled = true;
    app.driver.driving = true;
    app.driver.use_log(
        crate::datalog::Log::parse(
            "afr.csv",
            "Time,RPM,AFR_PRI,AFR_TARGET\n0,4321,13.12,12.7\n10,4321,13.12,12.7\n",
        )
        .unwrap(),
    );
    app.driver.tick(40_000);
    let values = app.driver.gauges.clone();
    for _ in 0..3 {
        let _ = frame(&mut app, &ctx, false, Vec::new());
    }
    assert_eq!(app.driver.gauges, values);
    assert_eq!(app.driver.gauges["RPM"].value, 4321.0);
    assert!((app.driver.gauges["AFR_PRI"].value - 13.12).abs() < 0.001);
    assert!((app.driver.gauges["AFR_TARGET"].value - 12.7).abs() < 0.001);
}

#[test]
fn source_controls_and_cold_restart_are_available_without_implicit_sending() {
    let ctx = egui::Context::default();
    let mut app = app();
    let text = app.text.clone();
    click(&mut app, &ctx, true, "Sample drive (US06)");
    assert!(app.driver.log.is_some());
    click(&mut app, &ctx, true, "Generic engine");
    assert!(app.driver.log.is_none());
    app.driver.enabled = true;
    app.driver.driving = true;
    app.driver.tick(1_000_000);
    assert!(app.driver.gauges["CLNT"].value > 60.0);
    click(&mut app, &ctx, true, "Cold engine on restart");
    assert!(app.driver.cold_start);
    click(&mut app, &ctx, true, "Restart drive");
    assert_eq!(app.driver.log_at_s, 0.0);
    assert!(app.driver.gauges["CLNT"].value < 40.0);
    assert!(app.driver.driving);
    assert!(!app.driver.to_car);
    assert_eq!(app.text, text);
}

#[test]
fn restoring_authored_values_stops_the_simulator_from_overwriting_them() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.driver.enabled = true;
    app.driver.driving = true;
    app.driver.tick(1_000_000);
    click(&mut app, &ctx, false, "Reset preview values");
    assert!(!app.driver.driving && !app.driver.sweep);
    let values = app.driver.gauges.clone();
    app.driver.tick(1_000_000);
    assert_eq!(app.driver.gauges, values);
}

#[test]
fn catalogue_search_distinguishes_recorded_afr_from_simulated_channels() {
    let ctx = egui::Context::default();
    ctx.style_mut(|style| style.animation_time = 0.0);
    let mut app = app();
    app.driver.use_log(
        crate::datalog::Log::parse("afr.csv", "Time,AFR_PRI\n0,13.12\n10,13.12\n").unwrap(),
    );
    click(&mut app, &ctx, true, "All generated readings");
    click(&mut app, &ctx, true, "Filter gauge name");
    let output = frame(&mut app, &ctx, true, vec![egui::Event::Text("afr".into())]);
    let labels: Vec<&str> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect();
    for name in ["AFR_PRI", "AFR_SEC", "AFR_TARGET", "Recorded", "Simulated"] {
        assert!(
            labels.contains(&name),
            "missing catalogue label {name}: {labels:?}"
        );
    }
    assert!(
        !labels.contains(&"RPM"),
        "the filter should remove unrelated gauges"
    );
    assert!(!app.driver.to_car);
}
