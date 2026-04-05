//! Unix Shells sign-in dialog.

use gtk4::prelude::*;
use gtk4::Box as GtkBox;
use gtk4::{glib, Dialog, Entry, Label, Orientation, ResponseType, Spinner, Window};
use std::cell::RefCell;
use std::rc::Rc;

use cterm_app::unixshells::DeviceService;

/// Show the Unix Shells sign-in dialog.
///
/// The dialog has two phases:
/// 1. Username entry
/// 2. Waiting for email approval (with spinner)
pub fn show_signin_dialog(
    parent: &impl IsA<Window>,
    device_service: &std::sync::Arc<DeviceService>,
) {
    let dialog = Dialog::builder()
        .title("Unix Shells - Sign In")
        .transient_for(parent)
        .modal(true)
        .default_width(400)
        .default_height(200)
        .build();

    let content = dialog.content_area();
    content.set_spacing(12);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);

    // Phase 1: Username entry
    let phase1 = GtkBox::new(Orientation::Vertical, 8);

    let title_label = Label::new(Some("Sign in to Unix Shells"));
    title_label.add_css_class("title-3");
    phase1.append(&title_label);

    let desc_label = Label::new(Some("Enter your unixshells.com username"));
    desc_label.add_css_class("dim-label");
    phase1.append(&desc_label);

    let username_entry = Entry::new();
    username_entry.set_placeholder_text(Some("username"));
    username_entry.set_activates_default(true);
    phase1.append(&username_entry);

    content.append(&phase1);

    // Phase 2: Waiting for approval (hidden initially)
    let phase2 = GtkBox::new(Orientation::Vertical, 8);
    phase2.set_visible(false);

    let waiting_label = Label::new(Some("Check your email to approve this device"));
    waiting_label.add_css_class("title-4");
    phase2.append(&waiting_label);

    let spinner = Spinner::new();
    spinner.set_spinning(true);
    phase2.append(&spinner);

    let status_label = Label::new(Some("Waiting for approval..."));
    status_label.add_css_class("dim-label");
    phase2.append(&status_label);

    content.append(&phase2);

    // Buttons
    dialog.add_button("Cancel", ResponseType::Cancel);
    let signin_btn = dialog.add_button("Sign In", ResponseType::Ok);
    signin_btn.add_css_class("suggested-action");

    // Handle response
    let ds = std::sync::Arc::clone(device_service);
    let phase1_ref = phase1.clone();
    let phase2_ref = phase2.clone();
    let status_ref = status_label.clone();
    let entry_ref = username_entry.clone();
    let signin_ref = signin_btn.clone();

    // Track whether we're in approval-waiting phase
    let waiting = Rc::new(RefCell::new(false));

    dialog.connect_response(move |dialog, response| {
        if response == ResponseType::Ok && !*waiting.borrow() {
            let username = entry_ref.text().to_string().trim().to_lowercase();
            if username.is_empty() {
                return;
            }

            // Switch to phase 2
            phase1_ref.set_visible(false);
            phase2_ref.set_visible(true);
            signin_ref.set_sensitive(false);
            *waiting.borrow_mut() = true;

            // Start login in background thread (start_login creates its own tokio runtime)
            let ds_bg: std::sync::Arc<DeviceService> = std::sync::Arc::clone(&ds);
            let username_bg = username.clone();
            std::thread::spawn(move || {
                if let Err(e) = ds_bg.start_login(&username_bg) {
                    log::error!("Login failed: {}", e);
                    // Error will be picked up by the polling timer via ds.last_error()
                }
            });

            // Poll for state changes
            let ds2 = std::sync::Arc::clone(&ds);
            let status = status_ref.clone();
            let dialog_weak2 = dialog.downgrade();
            let last_version = ds2.version.load(std::sync::atomic::Ordering::Relaxed);
            let last_version = Rc::new(RefCell::new(last_version));

            glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                let current = ds2.version.load(std::sync::atomic::Ordering::Relaxed);
                if current != *last_version.borrow() {
                    *last_version.borrow_mut() = current;

                    match ds2.login_state() {
                        cterm_app::unixshells::LoginState::LoggedIn { username } => {
                            status.set_text(&format!("Signed in as {}", username));
                            if let Some(dialog) = dialog_weak2.upgrade() {
                                dialog.close();
                            }
                            return glib::ControlFlow::Break;
                        }
                        cterm_app::unixshells::LoginState::LoggedOut => {
                            // Timed out or error
                            if let Some(err) = ds2.last_error() {
                                status.set_text(&err);
                            }
                            return glib::ControlFlow::Break;
                        }
                        cterm_app::unixshells::LoginState::PendingApproval { .. } => {
                            // Still waiting
                        }
                    }
                }

                // Keep polling if dialog is still open
                if dialog_weak2.upgrade().is_none() {
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            });
        } else {
            dialog.close();
        }
    });

    dialog.present();
}
