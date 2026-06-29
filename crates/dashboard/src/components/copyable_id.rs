use leptos::prelude::*;

/// Truncates an id to `head…tail` (UUID-style: `prefix…suffix`). `head == 0`
/// returns the full value; a value already within the `head + tail` budget is
/// returned unchanged. Char-based so multi-byte values never split a codepoint.
pub(crate) fn format_id(value: &str, head: usize, tail: usize) -> String {
    if head == 0 {
        return value.to_string();
    }
    let len = value.chars().count();
    if len <= head + tail + 1 {
        return value.to_string();
    }
    let prefix: String = value.chars().take(head).collect();
    if tail > 0 {
        let suffix: String = value.chars().skip(len - tail).collect();
        format!("{prefix}\u{2026}{suffix}")
    } else {
        format!("{prefix}\u{2026}")
    }
}

/// A monospaced id with a click-to-copy button (writes the full value to the
/// clipboard). `truncate = 0` (the default) shows the entire value; set it to a
/// positive number for `prefix…suffix` display. Mirrors the aarokya `CopyableId`.
///
/// The copy button calls `stop_propagation`, so it is safe to embed inside a
/// clickable row without triggering the row's own handler.
#[component]
pub fn CopyableId(
    #[prop(into)] value: String,
    /// Leading chars before the ellipsis; `0` (default) shows the full value.
    #[prop(default = 0)] truncate: usize,
    /// Trailing chars after the ellipsis (UUID-style). Default `4`.
    #[prop(default = 4)] truncate_tail: usize,
) -> impl IntoView {
    let (copied, set_copied) = signal(false);
    let display = format_id(&value, truncate, truncate_tail);
    let title = value.clone();

    let on_copy = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        ev.prevent_default();
        // Browser-only; on the server `window()` is `None` so this is skipped.
        if let Some(clipboard) = web_sys::window().map(|w| w.navigator().clipboard()) {
            let _ = clipboard.write_text(&value);
        }
        set_copied.set(true);
        set_timeout(move || set_copied.set(false), std::time::Duration::from_millis(1500));
    };

    view! {
        <span class="inline-flex items-center gap-1.5 font-mono text-xs" title=title>
            <span>{display}</span>
            <button type="button"
                on:click=on_copy
                aria-label=move || if copied.get() { "Copied" } else { "Copy ID" }
                class="inline-flex h-5 w-5 items-center justify-center rounded text-gray-400 hover:bg-gray-100 hover:text-gray-700 transition-colors">
                {move || if copied.get() {
                    view! { <span class="text-green-600">"\u{2713}"</span> }.into_any()
                } else {
                    view! { <span>"\u{29C9}"</span> }.into_any()
                }}
            </button>
        </span>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_id_zero_head_returns_full() {
        assert_eq!(format_id("abcdef-1234-5678", 0, 4), "abcdef-1234-5678");
    }

    #[test]
    fn format_id_truncates_prefix_and_suffix() {
        let id = "81724436-aaaa-bbbb-cccc-1234567890ef";
        assert_eq!(format_id(id, 8, 4), "81724436\u{2026}90ef");
    }

    #[test]
    fn format_id_no_tail_is_prefix_ellipsis() {
        assert_eq!(format_id("81724436abcdef", 8, 0), "81724436\u{2026}");
    }

    #[test]
    fn format_id_short_value_unchanged() {
        // Within the head + tail + 1 budget: returned as-is.
        assert_eq!(format_id("abc", 8, 4), "abc");
    }
}
