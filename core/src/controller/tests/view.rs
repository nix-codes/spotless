//! What shows below the player, and the equaliser's presets.

use super::*;

#[test]
fn equaliser_presets_are_saved_by_name_and_deleted() {
    let mut rig = Rig::new("equaliser");
    let mut bands = [0.0; 10];
    bands[0] = 3.0;
    rig.controller.apply(ViewAction::SetEqualiser(Equaliser {
        on: true,
        preamp: 40.0,
        bands,
    }));
    let settings = |rig: &Rig| rig.controller.state().settings.clone();
    assert_eq!(settings(&rig).equaliser.preamp, 12.0);
    rig.controller
        .apply(ViewAction::SaveEqualiserPreset(" Mine ".into()));
    rig.controller
        .apply(ViewAction::SaveEqualiserPreset("".into()));
    bands[0] = -3.0;
    rig.controller.apply(ViewAction::SetEqualiser(Equaliser {
        on: true,
        preamp: 0.0,
        bands,
    }));
    rig.controller
        .apply(ViewAction::SaveEqualiserPreset("Mine".into()));
    let presets = settings(&rig).equaliser_presets;
    assert_eq!(presets.len(), 1);
    assert_eq!(
        (presets[0].name.as_str(), presets[0].bands[0]),
        ("Mine", -3.0)
    );
    rig.controller
        .apply(ViewAction::DeleteEqualiserPreset("Mine".into()));
    assert!(settings(&rig).equaliser_presets.is_empty());
    rig.controller.apply(ViewAction::ToggleEqualiser);
    assert!(settings(&rig).equaliser_shown);
}

#[test]
fn the_deck_shows_and_changes_media() {
    let mut rig = Rig::new("deck");
    let settings = |rig: &Rig| rig.controller.state().settings.clone();
    rig.controller.apply(ViewAction::ToggleDeck);
    assert!(settings(&rig).deck_shown);
    rig.controller
        .apply(ViewAction::SetDeckMedia(crate::store::DeckMedia::Reel));
    assert_eq!(settings(&rig).deck_media, crate::store::DeckMedia::Reel);
    rig.controller
        .apply(ViewAction::SetDeckMeter(crate::store::DeckMeter::Vu));
    assert_eq!(settings(&rig).deck_meter, crate::store::DeckMeter::Vu);
    rig.controller
        .apply(ViewAction::SetVuFace(crate::store::VuFace::Blue));
    assert_eq!(settings(&rig).vu_face, crate::store::VuFace::Blue);
    rig.controller.apply(ViewAction::SetVuCalibration(
        crate::store::VuCalibration::Minus18,
    ));
    assert_eq!(
        settings(&rig).vu_calibration,
        crate::store::VuCalibration::Minus18
    );
    rig.controller.apply(ViewAction::SetCassetteMix(true));
    assert!(settings(&rig).cassette_mix);
    rig.controller.apply(ViewAction::SetVinylLp(true));
    assert!(settings(&rig).vinyl_lp);
    rig.controller.apply(ViewAction::SetReelSlow(true));
    assert!(settings(&rig).reel_slow);
    rig.controller.apply(ViewAction::SetCdMini(true));
    assert!(settings(&rig).cd_mini);
    rig.controller.apply(ViewAction::ToggleDeck);
    assert!(!settings(&rig).deck_shown);
}

#[test]
fn the_time_shows_what_is_left_and_back() {
    let mut rig = Rig::new("time-left");
    let time_left = |rig: &Rig| rig.controller.state().settings.time_left;
    assert!(!time_left(&rig), "what has played to start with");
    rig.controller.apply(ViewAction::ToggleTimeLeft);
    assert!(time_left(&rig));
    rig.controller.apply(ViewAction::ToggleTimeLeft);
    assert!(!time_left(&rig));
}
