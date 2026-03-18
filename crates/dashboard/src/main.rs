mod api;
mod app;
mod components;
mod pages;

use app::App;
use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Debug);

    mount_to_body(App);
}
