//! Create, edit, disable and delete user accounts; reset passwords.

use crate::auth::password;
use crate::auth::store::{Role, User, UserFields};
use crate::error::{AppError, AppResult};
use crate::extract::AdminCtx;
use crate::flash;
use crate::form::{checked, key};
use crate::state::Shared;
use crate::views::components::{Text, card, heading, thead, tip};
use crate::views::forms::{action_button, enum_select, field, text_input};
use crate::views::{Nav, Page, page};
use axum::extract::Path;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Form, Router};
use maud::{Markup, html};
use serde::Deserialize;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/admin/users", get(list).post(create))
        .route("/admin/users/{id}", get(edit_page).post(update))
        .route("/admin/users/{id}/reset-password", post(reset_password))
        .route("/admin/users/{id}/delete", post(delete))
}

const ROLE_TIP: &str = "Admins manage user accounts. Users use the app.";
const USERNAME_TIP: &str = "Used to sign in. Letters, digits, dots, dashes, underscores or @.";

fn status(u: &User) -> &'static str {
    match (u.disabled, u.must_change_password) {
        (true, _) => "Disabled",
        (false, true) => "Awaiting first sign-in",
        (false, false) => "Active",
    }
}

fn users_table(cx: &AdminCtx, users: &[User]) -> Markup {
    html! {
        div class="table-wrap" {
            table {
                (thead(&[
                    ("Username", Text), ("Name", Text), ("Role", Text), ("Status", Text),
                    ("Last sign-in", Text), ("", Text),
                ]))
                tbody {
                    @for u in users {
                        tr {
                            td {
                                (u.username)
                                @if u.id == cx.user.id { span class="muted" { " (you)" } }
                            }
                            td { (u.display_name) }
                            td { (u.role.label()) }
                            td { (status(u)) }
                            td class="muted" { (u.last_login.as_deref().unwrap_or("never")) }
                            td {
                                @let edit = cx.url(&format!("/admin/users/{}", u.id));
                                a class="btn small" href=(edit) { "Edit" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn add_user_form(cx: &AdminCtx) -> Markup {
    html! {
        form method="post" action=(cx.url("/admin/users")) class="form-grid" {
            (field("Username", USERNAME_TIP, html! { input type="text" name="username" required; }))
            (field("Name", "Optional display name.", text_input("display_name", "")))
            (field("Role", ROLE_TIP, enum_select("role", Role::User, false)))
            div { button type="submit" class="primary" { "Add user" } }
        }
        p class="muted" {
            "A temporary password is created for the new user; they choose their own "
            "when they first sign in."
        }
    }
}

async fn list(cx: AdminCtx) -> AppResult<Page> {
    let users = cx.st.auth.users()?;
    let body = html! {
        (heading("Users", "Manage who can sign in.", html! {}))
        (card("Users", "", users_table(&cx, &users)))
        (card("Add a user", "", add_user_form(&cx)))
    };
    Ok(page(&cx, Nav::Users, "Users", body))
}

#[derive(Deserialize)]
pub struct UserForm {
    username: String,
    #[serde(default)]
    display_name: String,
    role: Option<String>,
    disabled: Option<String>,
}

impl UserForm {
    fn fields(&self) -> AppResult<UserFields> {
        let role = self
            .role
            .as_deref()
            .map(|r| key::<Role>(r, "role"))
            .transpose()?;
        Ok(UserFields {
            username: self.username.clone(),
            display_name: self.display_name.clone(),
            role,
            disabled: checked(&self.disabled),
        })
    }
}

fn temp_password_message(username: &str, temp: &str) -> String {
    format!(
        "Temporary password for {username}: {temp}. They'll choose their own when they sign in."
    )
}

async fn create(cx: AdminCtx, Form(f): Form<UserForm>) -> AppResult<Response> {
    let back = cx.url("/admin/users");
    let fields = f.fields()?;
    let temp = password::generate_temporary();
    let result = cx
        .st
        .auth
        .create_user(&fields, &password::hash(&temp)?, true)
        .map(|_| temp_password_message(fields.username.trim(), &temp));
    Ok(flash::report(&cx.st, &back, result))
}

fn load(cx: &AdminCtx, id: i64) -> AppResult<User> {
    cx.st.auth.user(id)?.ok_or(AppError::NotFound)
}

fn edit_form(cx: &AdminCtx, u: &User) -> Markup {
    let is_self = u.id == cx.user.id;
    html! {
        form method="post" action=(cx.url(&format!("/admin/users/{}", u.id))) class="form-grid" {
            (field("Username", USERNAME_TIP, text_input("username", &u.username)))
            (field("Name", "", text_input("display_name", &u.display_name)))
            @if is_self {
                p class="muted" {
                    "You can't change your own role or disable yourself; ask another admin."
                }
            } @else {
                (field("Role", ROLE_TIP, enum_select("role", u.role, false)))
                label class="check" {
                    input type="checkbox" name="disabled" value="1" checked[u.disabled];
                    "Disabled (can't sign in) "
                    (tip("Signs the user out everywhere and blocks sign-in."))
                }
            }
            div { button type="submit" class="primary" { "Save" } }
        }
    }
}

async fn edit_page(cx: AdminCtx, Path(id): Path<i64>) -> AppResult<Page> {
    let u = load(&cx, id)?;
    let base = format!("/admin/users/{id}");
    let body = html! {
        (heading(&format!("Edit {}", u.username), &format!("Created {}.", u.created_at), html! {
            a class="btn" href=(cx.url("/admin/users")) { "← All users" }
        }))
        div class="grid" {
            (card("Account", "", edit_form(&cx, &u)))
            (card("Password", "", html! {
                p { "Create a new temporary password and sign this user out everywhere." }
                (action_button(&cx.url(&format!("{base}/reset-password")), "Reset password", "",
                    Some("Reset this user's password?")))
            }))
            @if u.id != cx.user.id {
                (card("Delete", "", html! {
                    p { "Deletes the account." }
                    (action_button(&cx.url(&format!("{base}/delete")), "Delete user", "danger",
                        Some("Delete this user? This can't be undone.")))
                }))
            }
        }
    };
    Ok(page(&cx, Nav::Users, "Edit user", body))
}

async fn update(cx: AdminCtx, Path(id): Path<i64>, Form(f): Form<UserForm>) -> AppResult<Response> {
    let before = load(&cx, id)?;
    let mut fields = f.fields()?;
    if id == cx.user.id {
        fields.role = Some(before.role);
        fields.disabled = before.disabled;
    }
    let back = cx.url(&format!("/admin/users/{id}"));
    let result = cx.st.auth.update_user(id, &fields).map(|()| "User saved.");
    Ok(flash::report(&cx.st, &back, result))
}

async fn reset_password(cx: AdminCtx, Path(id): Path<i64>) -> AppResult<Response> {
    let u = load(&cx, id)?;
    let temp = password::generate_temporary();
    cx.st.auth.set_password(id, &password::hash(&temp)?, true)?;
    let back = cx.url(&format!("/admin/users/{id}"));
    Ok(flash::info(
        &cx.st,
        &back,
        temp_password_message(&u.username, &temp),
    ))
}

async fn delete(cx: AdminCtx, Path(id): Path<i64>) -> AppResult<Response> {
    if id == cx.user.id {
        return Err(AppError::Forbidden("You can't delete your own account."));
    }
    let u = load(&cx, id)?;
    let result = cx
        .st
        .auth
        .delete_user(id)
        .map(|()| format!("Deleted {}.", u.username));
    let to = if result.is_ok() {
        cx.url("/admin/users")
    } else {
        cx.url(&format!("/admin/users/{id}"))
    };
    Ok(flash::report(&cx.st, &to, result))
}
