use crate::app::App;
use crate::config::Config;
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
