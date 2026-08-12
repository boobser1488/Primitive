// Announces sunset, once per night.
//
// Demonstrates per-plugin persistent state: `store` survives between
// hook calls, so the plugin can remember it has already spoken.

fn on_tick(tick) {
    let t = time_of_day();
    let is_night = t < 0.25 || t > 0.75;

    let announced = false;
    if store.contains("announced") {
        announced = store.announced == "yes";
    }

    if is_night && !announced {
        broadcast("Night is falling -- watch your step.");
        store.announced = "yes";
    }
    if !is_night && announced {
        store.announced = "no";
    }
}
