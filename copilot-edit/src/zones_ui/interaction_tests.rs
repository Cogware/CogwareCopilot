// SPDX-License-Identifier: GPL-3.0-only
//! Stable band editing and unclipped threshold regressions.

use super::*;

#[test]
fn precise_thresholds_survive_serialisation() {
    for at in [-0.125, 0.0001, 1.25] {
        let z = Zone {
            at,
            color: Color::WHITE,
            cursor: None,
        };
        let text = to_json(&[z]);
        assert!(text.contains(&format!("\"at\": {at},")), "{text}");
    }
}

#[test]
fn sorting_the_scene_does_not_move_the_row_being_dragged() {
    let low = Zone {
        at: 0.8,
        color: Color::rgb(255, 0, 0),
        cursor: None,
    };
    let high = Zone {
        at: 0.7,
        color: Color::rgb(0, 255, 0),
        cursor: None,
    };
    let mut draft = BandDraft::of(&[low, high]);
    let sorted = [high, low];
    draft.source = to_json(&sorted);
    draft.sync(&sorted);
    assert_eq!(draft.edited[0].color, low.color);
    assert_eq!(draft.edited[1].color, high.color);
}

#[test]
fn showing_bands_never_clamps_off_scale_thresholds() {
    let ctx = egui::Context::default();
    let zones = [
        Zone {
            at: -0.1,
            color: Color::WHITE,
            cursor: None,
        },
        Zone {
            at: 1.5,
            color: Color::WHITE,
            cursor: None,
        },
    ];
    for _ in 0..3 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(zone_rows(ui, &zones, Color::WHITE, true).is_none());
            });
        });
    }
}
