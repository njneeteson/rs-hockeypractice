//! Form controls shared by many pages.

use super::components::tip;
use localapp_core::enum_macro::KeyEnum;
use maud::{Markup, html};

/// A labelled form field with an optional tooltip.
pub fn field(label: &str, hint: &str, control: Markup) -> Markup {
    html! {
        label class="field" {
            span class="field-label" { (label) " " (tip(hint)) }
            (control)
        }
    }
}

/// How a [`live_form`] reacts.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Live {
    /// Filters: every change (and Apply) refreshes the results and the address bar.
    Filter,
    /// Previews: changes refresh a preview; submit buttons still post the form normally.
    Preview,
}

/// Where a [`live_form`] puts its results.
pub struct LiveTarget<'a> {
    /// Id of the element to refresh.
    pub id: &'a str,
    /// Ids of other elements elsewhere on the page to refresh from the same response.
    pub also: &'a [&'a str],
}

/// A GET form that, with JavaScript, refreshes part of the page in place as its
/// controls change (typing is debounced) instead of reloading the page.
/// Without JavaScript it is an ordinary form. The server just renders the whole
/// page again; htmx picks out the target element.
pub fn live_form(
    action: &str,
    class: &str,
    target: LiveTarget<'_>,
    live: Live,
    body: Markup,
) -> Markup {
    let selector = format!("#{}", target.id);
    let also: Vec<String> = target.also.iter().map(|id| format!("#{id}")).collect();
    let (trigger, push_url) = match live {
        Live::Filter => ("submit, change, input delay:400ms", "true"),
        Live::Preview => ("change, input delay:500ms", "false"),
    };
    html! {
        form method="get" action=(action) class=(class)
            hx-get=(action) hx-trigger=(trigger) hx-target=(selector) hx-select=(selector)
            hx-select-oob=[(!also.is_empty()).then(|| also.join(","))]
            hx-swap="outerHTML" hx-push-url=(push_url) hx-sync="this:replace"
            hx-indicator=(selector) {
            (body)
        }
    }
}

/// A labelled checkbox that submits `value="1"` when ticked.
pub fn checkbox(name: &str, checked: bool, label: Markup) -> Markup {
    html! {
        label class="check" {
            input type="checkbox" name=(name) value="1" checked[checked];
            (label)
        }
    }
}

/// `<input type="text">`.
pub fn text_input(name: &str, value: &str) -> Markup {
    html! { input type="text" name=(name) value=(value); }
}

/// `<select>` over every value of a `str_enum!`; `autosubmit` submits on change.
pub fn enum_select<T: KeyEnum + PartialEq>(name: &str, selected: T, autosubmit: bool) -> Markup {
    html! {
        select name=(name) data-autosubmit[autosubmit] {
            @for v in T::all() {
                option value=(v.key()) selected[*v == selected] { (v.label()) }
            }
        }
    }
}

/// A POST form holding a single button; `confirm` asks before submitting.
pub fn action_button(action: &str, label: &str, class: &str, confirm: Option<&str>) -> Markup {
    html! {
        form method="post" action=(action) class="inline" {
            button type="submit" class={ "btn " (class) } data-confirm=[confirm] { (label) }
        }
    }
}
