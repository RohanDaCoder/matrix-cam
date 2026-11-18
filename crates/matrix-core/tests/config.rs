//! Integration tests for the settings/config model: presets resolve,
//! clamps hold, and the JSON config round-trips through save/load (the
//! same model localStorage and the config file both serialize).

use matrix_core::config::{Config, Settings, PRESETS};

#[test]
fn all_presets_resolve_with_sane_defaults() {
    for name in PRESETS {
        let s = Settings::preset(name);
        // clamped ranges: sliders' bounds hold on preset output too
        assert!(s.scan <= 100, "{name}: scan");
        assert!(s.noise <= 100, "{name}: noise");
        assert!(s.trails <= 90, "{name}: trails");
        assert!(s.glow <= 24, "{name}: glow");
    }
}

#[test]
fn presets_are_actually_distinct() {
    let a = Settings::preset("ghost");
    let b = Settings::preset("paper");
    assert_ne!(a.ink, b.ink, "ghost and paper must differ in ink color");
}

#[test]
fn config_json_round_trip() {
    let dir = std::env::temp_dir().join(format!("mc-cfg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.json");
    let mut cfg = Config::default();
    let mut set = Settings::preset("vhs");
    set.glow = 9;
    set.trails = 55;
    cfg.set = Some(set);
    cfg.user_presets.push(matrix_core::config::UserPreset {
        name: "my look".into(),
        set: Settings::preset("term"),
    });
    cfg.preview_fps = 24;
    cfg.fps_limit_enabled = true;
    cfg.fps_limit_fps = 10;
    cfg.save(path.to_str()).unwrap();

    let back = Config::load(path.to_str());
    let set = back.set.expect("settings survive the round trip");
    assert_eq!(set.glow, 9);
    assert_eq!(set.trails, 55);
    assert_eq!(set.style, Settings::preset("vhs").style);
    assert_eq!(back.user_presets.len(), 1);
    assert_eq!(back.user_presets[0].name, "my look");
    assert_eq!(back.preview_fps, 24);
    assert!(back.fps_limit_enabled);
    assert_eq!(back.fps_limit_fps, 10);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn loading_garbage_falls_back_to_default() {
    let dir = std::env::temp_dir().join(format!("mc-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("broken.json");
    std::fs::write(&path, "{ not json !!!").unwrap();
    let cfg = Config::load(path.to_str());
    assert!(cfg.set.is_none(), "unparseable file falls back to defaults");
    std::fs::remove_dir_all(&dir).ok();
}
