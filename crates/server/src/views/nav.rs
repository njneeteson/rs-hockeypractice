//! Navigation: which page is active and the side menu.
//!
//! To add a page to the menu: add a [`Nav`] variant and an entry in [`SECTIONS`].

use crate::extract::Ctx;
use maud::{Markup, html};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Home,
    Account,
    Users,
}

struct Item {
    nav: Nav,
    label: &'static str,
    path: &'static str,
    /// Shown only to admins.
    admin_only: bool,
}

const fn item(nav: Nav, label: &'static str, path: &'static str, admin_only: bool) -> Item {
    Item {
        nav,
        label,
        path,
        admin_only,
    }
}

/// Side-menu groups: `(section title, items)`.
const SECTIONS: &[(&str, &[Item])] = &[
    ("App", &[item(Nav::Home, "Home", "/", false)]),
    (
        "Settings",
        &[
            item(Nav::Account, "My account", "/account", false),
            item(Nav::Users, "Users", "/admin/users", true),
        ],
    ),
];

pub fn side_menu(cx: &Ctx, active: Nav) -> Markup {
    html! {
        nav class="sidenav" aria-label="Main" {
            @for (title, items) in SECTIONS {
                @let visible: Vec<&Item> = items
                    .iter()
                    .filter(|it| cx.user.is_admin() || !it.admin_only)
                    .collect();
                @if !visible.is_empty() {
                    div class="section" { (title) }
                    @for it in visible {
                        a href=(cx.url(it.path)) aria-current=[(it.nav == active).then_some("page")] {
                            (it.label)
                        }
                    }
                }
            }
        }
    }
}
