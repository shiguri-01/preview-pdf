use crate::app::App;
use crate::config::{AppOptions, CacheOptions, Config};
use crate::presenter::PresenterKind;

#[test]
fn new_with_config_applies_l1_cache_limits() {
    let mut config = Config::default();
    config.cache.l1_max_entries = 7;
    config.cache.l1_memory_budget_mb = 2;

    let app = App::new_with_config(PresenterKind::RatatuiImage, config.clone()).expect("app init");

    assert_eq!(app.render.runtime.l1_cache.max_entries(), 7);
    assert_eq!(
        app.render.runtime.l1_cache.memory_budget_bytes(),
        config.cache.l1_memory_budget_bytes()
    );
}

#[test]
fn new_with_options_applies_l1_cache_limits_without_file_config() {
    let options = AppOptions {
        cache: CacheOptions {
            l1_max_entries: Some(9),
            l1_memory_budget_mb: Some(3),
            ..CacheOptions::default()
        },
        ..AppOptions::default()
    };

    let app = App::new_with_options(PresenterKind::RatatuiImage, options).expect("app init");

    assert_eq!(app.render.runtime.l1_cache.max_entries(), 9);
    assert_eq!(
        app.render.runtime.l1_cache.memory_budget_bytes(),
        3 * 1024 * 1024
    );
}
