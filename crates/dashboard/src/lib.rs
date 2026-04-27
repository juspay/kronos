pub mod api;
pub mod app;
pub mod components;
pub mod config;
pub mod pages;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use wasm_bindgen::JsCast;
    // Clear SSR content and mount fresh — avoids hydration mismatch
    // with LocalResource + Suspense streaming chunks
    let body = leptos::prelude::document()
        .body()
        .unwrap()
        .unchecked_into::<web_sys::HtmlElement>();
    body.set_inner_html("");
    leptos::mount::mount_to(body, app::App).forget();
}
