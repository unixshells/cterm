//! Tab templates dialog for managing sticky tab configurations
//!
//! Provides a dialog to create, edit, and delete tab templates with support
//! for general settings, Docker configuration, and SSH configuration.

use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr;

use winapi::shared::basetsd::INT_PTR;
use winapi::shared::minwindef::{LPARAM, UINT, WPARAM};
use winapi::shared::windef::HWND;
use winapi::um::commctrl::*;
use winapi::um::winuser::*;

use crate::dialog_utils::*;
use cterm_app::config::{DockerMode, StickyTabConfig};

// Control IDs - Top bar
const IDC_TEMPLATE_LIST: i32 = 1001;
const IDC_ADD_TEMPLATE: i32 = 1002;
const IDC_REMOVE_TEMPLATE: i32 = 1003;
const IDC_PRESETS: i32 = 1004;

// Control IDs - Tabs
const IDC_TABS: i32 = 1010;

// Control IDs - General tab
const IDC_TMPL_NAME: i32 = 1020;
const IDC_TMPL_COMMAND: i32 = 1021;
const IDC_TMPL_ARGS: i32 = 1022;
const IDC_TMPL_WORKDIR: i32 = 1023;
const IDC_TMPL_GIT_REMOTE: i32 = 1029;
const IDC_TMPL_COLOR: i32 = 1024;
const IDC_TMPL_COLOR_BTN: i32 = 1025;
const IDC_TMPL_UNIQUE: i32 = 1026;
const IDC_TMPL_KEEPOPEN: i32 = 1028;
const IDC_TMPL_REMOTE: i32 = 1027;

// Control IDs - Docker tab
const IDC_DOCKER_MODE: i32 = 1030;
const IDC_DOCKER_CONTAINER: i32 = 1031;
const IDC_DOCKER_IMAGE: i32 = 1032;
const IDC_DOCKER_SHELL: i32 = 1033;
const IDC_DOCKER_AUTOREMOVE: i32 = 1034;
const IDC_DOCKER_PROJECT: i32 = 1035;
const IDC_DOCKER_ENABLE: i32 = 1036;

// Control IDs - SSH tab
const IDC_SSH_ENABLE: i32 = 1040;
const IDC_SSH_HOST: i32 = 1041;
const IDC_SSH_PORT: i32 = 1042;
const IDC_SSH_USER: i32 = 1043;
const IDC_SSH_IDENTITY: i32 = 1044;
const IDC_SSH_JUMP: i32 = 1045;
const IDC_SSH_X11: i32 = 1046;
const IDC_SSH_AGENT: i32 = 1047;

// Tab indices
const TAB_GENERAL: i32 = 0;
const TAB_DOCKER: i32 = 1;
const TAB_SSH: i32 = 2;

/// Preset templates
#[allow(clippy::type_complexity)]
const PRESETS: &[(&str, fn() -> StickyTabConfig)] = &[
    ("Claude Code", preset_claude),
    ("Claude DevContainer", preset_claude_devcontainer),
    ("Ubuntu Container", preset_ubuntu),
    ("Alpine Container", preset_alpine),
    ("Node.js Container", preset_nodejs),
    ("Python Container", preset_python),
    ("SSH Server", preset_ssh),
    ("SSH with Agent", preset_ssh_agent),
];

fn preset_claude() -> StickyTabConfig {
    StickyTabConfig::claude()
}

fn preset_claude_devcontainer() -> StickyTabConfig {
    StickyTabConfig::claude_devcontainer(None)
}

fn preset_ubuntu() -> StickyTabConfig {
    StickyTabConfig::ubuntu()
}

fn preset_alpine() -> StickyTabConfig {
    StickyTabConfig::alpine()
}

fn preset_nodejs() -> StickyTabConfig {
    StickyTabConfig::nodejs()
}

fn preset_python() -> StickyTabConfig {
    StickyTabConfig::python()
}

fn preset_ssh() -> StickyTabConfig {
    StickyTabConfig::ssh("SSH Server", "example.com", None)
}

fn preset_ssh_agent() -> StickyTabConfig {
    StickyTabConfig::ssh_with_agent("SSH with Agent", "example.com", None)
}

/// Dialog state
struct DialogState {
    templates: Vec<StickyTabConfig>,
    current_template: usize,
    current_tab: i32,
    // Control handles for each tab
    general_controls: Vec<HWND>,
    docker_controls: Vec<HWND>,
    ssh_controls: Vec<HWND>,
    /// Remote names loaded from config (for remote combo)
    remote_names: Vec<String>,
}

// Thread-local storage for dialog state
thread_local! {
    static DIALOG_STATE: RefCell<Option<DialogState>> = const { RefCell::new(None) };
}

/// Show the tab templates dialog
///
/// Returns true if templates were saved, false if cancelled.
pub fn show_templates_dialog(parent: HWND) -> bool {
    // Load current templates and config
    let templates = cterm_app::load_sticky_tabs().unwrap_or_default();
    let remote_names: Vec<String> = cterm_app::config::load_config()
        .unwrap_or_default()
        .remotes
        .iter()
        .map(|r| r.name.clone())
        .collect();

    DIALOG_STATE.with(|s| {
        *s.borrow_mut() = Some(DialogState {
            templates,
            current_template: 0,
            current_tab: TAB_GENERAL,
            general_controls: Vec::new(),
            docker_controls: Vec::new(),
            ssh_controls: Vec::new(),
            remote_names,
        });
    });

    // Build and show dialog
    let template = build_dialog_template();
    let ret = unsafe {
        DialogBoxIndirectParamW(
            ptr::null_mut(),
            template.as_ptr() as *const DLGTEMPLATE,
            parent,
            Some(dialog_proc),
            0,
        )
    };

    // Clean up state
    DIALOG_STATE.with(|s| {
        *s.borrow_mut() = None;
    });

    ret == IDOK as isize
}

/// Build the dialog template
fn build_dialog_template() -> Vec<u8> {
    let mut template = Vec::new();

    // Dialog dimensions
    let width: i16 = 370; // ~550 pixels
    let height: i16 = 340; // ~520 pixels

    let style = DS_MODALFRAME | DS_CENTER | WS_POPUP | WS_CAPTION | WS_SYSMENU | DS_SETFONT;
    let ex_style = 0u32;
    let c_dit = 0u16;
    let x = 0i16;
    let y = 0i16;

    template.extend_from_slice(&style.to_le_bytes());
    template.extend_from_slice(&ex_style.to_le_bytes());
    template.extend_from_slice(&c_dit.to_le_bytes());
    template.extend_from_slice(&x.to_le_bytes());
    template.extend_from_slice(&y.to_le_bytes());
    template.extend_from_slice(&width.to_le_bytes());
    template.extend_from_slice(&height.to_le_bytes());

    // Menu (none)
    template.extend_from_slice(&[0u8, 0]);
    // Class (use default)
    template.extend_from_slice(&[0u8, 0]);
    // Title
    let title = to_wide("Tab Templates");
    for c in &title {
        template.extend_from_slice(&c.to_le_bytes());
    }

    // Font
    align_to_word(&mut template);
    template.extend_from_slice(&9u16.to_le_bytes());
    let font = to_wide("Segoe UI");
    for c in &font {
        template.extend_from_slice(&c.to_le_bytes());
    }

    template
}

fn align_to_word(v: &mut Vec<u8>) {
    while !v.len().is_multiple_of(2) {
        v.push(0);
    }
}

/// Dialog procedure
unsafe extern "system" fn dialog_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> INT_PTR {
    match msg {
        WM_INITDIALOG => {
            init_dialog(hwnd);
            1
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as i32;
            let code = ((wparam >> 16) & 0xFFFF) as u16;
            handle_command(hwnd, id, code);
            1
        }
        WM_NOTIFY => {
            let nmhdr = lparam as *const NMHDR;
            if !nmhdr.is_null() {
                handle_notify(hwnd, &*nmhdr);
            }
            0
        }
        WM_CLOSE => {
            EndDialog(hwnd, IDCANCEL as isize);
            1
        }
        _ => 0,
    }
}

/// Initialize the dialog
unsafe fn init_dialog(hwnd: HWND) {
    let mut rect = std::mem::zeroed();
    GetClientRect(hwnd, &mut rect);
    let dlg_width = rect.right - rect.left;
    let dlg_height = rect.bottom - rect.top;

    let margin = 10;
    let button_height = 25;
    let button_width = 75;
    let small_btn_width = 30;
    let combo_height = 22;
    let tab_height = 28;

    // Top bar: Template selector, add/remove buttons, presets
    let mut x = margin;
    create_label(hwnd, -1, "Template:", x, margin + 3, 60, 18);
    x += 65;

    let template_combo = create_combobox(hwnd, IDC_TEMPLATE_LIST, x, margin, 150, combo_height);
    x += 155;

    create_button(
        hwnd,
        IDC_ADD_TEMPLATE,
        "+",
        x,
        margin,
        small_btn_width,
        button_height,
    );
    x += small_btn_width + 5;

    create_button(
        hwnd,
        IDC_REMOVE_TEMPLATE,
        "-",
        x,
        margin,
        small_btn_width,
        button_height,
    );
    x += small_btn_width + 15;

    create_label(hwnd, -1, "Presets:", x, margin + 3, 50, 18);
    x += 55;

    let presets_combo = create_combobox(hwnd, IDC_PRESETS, x, margin, 120, combo_height);
    add_combobox_item(presets_combo, "Add preset...");
    for (name, _) in PRESETS {
        add_combobox_item(presets_combo, name);
    }
    set_combobox_selection(presets_combo, 0);

    // Tab control
    let tab_top = margin + combo_height + 10;
    let tab_ctrl = create_tab_control(
        hwnd,
        IDC_TABS,
        margin,
        tab_top,
        dlg_width - margin * 2,
        tab_height,
    );
    add_tab(tab_ctrl, TAB_GENERAL, "General");
    add_tab(tab_ctrl, TAB_DOCKER, "Docker");
    add_tab(tab_ctrl, TAB_SSH, "SSH");

    // Content area
    let content_top = tab_top + tab_height + 5;
    let content_height = dlg_height - content_top - button_height - margin * 2 - 5;

    // Create controls for each tab
    create_general_controls(
        hwnd,
        margin,
        content_top,
        dlg_width - margin * 2,
        content_height,
    );
    create_docker_controls(
        hwnd,
        margin,
        content_top,
        dlg_width - margin * 2,
        content_height,
    );
    create_ssh_controls(
        hwnd,
        margin,
        content_top,
        dlg_width - margin * 2,
        content_height,
    );

    // Show only General tab initially
    show_tab(TAB_GENERAL);

    // Create buttons at bottom
    let btn_y = dlg_height - button_height - margin;
    create_button(
        hwnd,
        IDCANCEL,
        "Cancel",
        dlg_width - margin - button_width * 2 - 10,
        btn_y,
        button_width,
        button_height,
    );
    create_default_button(
        hwnd,
        IDOK,
        "Save",
        dlg_width - margin - button_width,
        btn_y,
        button_width,
        button_height,
    );

    // Populate template list
    populate_template_list(template_combo);

    // Load first template if exists
    DIALOG_STATE.with(|s| {
        if let Some(ref state) = *s.borrow() {
            if !state.templates.is_empty() {
                set_combobox_selection(template_combo, 0);
            }
        }
    });
    load_current_template();
    update_remove_button(hwnd);
}

/// Create controls for the General tab
unsafe fn create_general_controls(hwnd: HWND, x: i32, y: i32, w: i32, _h: i32) {
    let mut controls = Vec::new();
    let row_height = 26;
    let label_width = 100;
    let control_width = w - label_width - 20;

    // Name
    let mut cy = y;
    controls.push(create_label(hwnd, -1, "Name:", x, cy + 3, label_width, 18));
    controls.push(create_edit(
        hwnd,
        IDC_TMPL_NAME,
        x + label_width + 10,
        cy,
        control_width,
        22,
    ));

    // Command
    cy += row_height + 5;
    controls.push(create_label(
        hwnd,
        -1,
        "Command:",
        x,
        cy + 3,
        label_width,
        18,
    ));
    controls.push(create_edit(
        hwnd,
        IDC_TMPL_COMMAND,
        x + label_width + 10,
        cy,
        control_width,
        22,
    ));

    // Arguments
    cy += row_height + 5;
    controls.push(create_label(
        hwnd,
        -1,
        "Arguments:",
        x,
        cy + 3,
        label_width,
        18,
    ));
    controls.push(create_edit(
        hwnd,
        IDC_TMPL_ARGS,
        x + label_width + 10,
        cy,
        control_width,
        22,
    ));

    // Working directory
    cy += row_height + 5;
    controls.push(create_label(
        hwnd,
        -1,
        "Working dir:",
        x,
        cy + 3,
        label_width,
        18,
    ));
    controls.push(create_edit(
        hwnd,
        IDC_TMPL_WORKDIR,
        x + label_width + 10,
        cy,
        control_width,
        22,
    ));

    // Git remote
    cy += row_height + 5;
    controls.push(create_label(
        hwnd,
        -1,
        "Git remote:",
        x,
        cy + 3,
        label_width,
        18,
    ));
    controls.push(create_edit(
        hwnd,
        IDC_TMPL_GIT_REMOTE,
        x + label_width + 10,
        cy,
        control_width,
        22,
    ));

    // Color
    cy += row_height + 5;
    controls.push(create_label(hwnd, -1, "Color:", x, cy + 3, label_width, 18));
    controls.push(create_edit(
        hwnd,
        IDC_TMPL_COLOR,
        x + label_width + 10,
        cy,
        100,
        22,
    ));
    controls.push(create_button(
        hwnd,
        IDC_TMPL_COLOR_BTN,
        "...",
        x + label_width + 115,
        cy,
        30,
        22,
    ));

    // Remote
    cy += row_height + 5;
    controls.push(create_label(
        hwnd,
        -1,
        "Remote:",
        x,
        cy + 3,
        label_width,
        18,
    ));
    let remote_combo = create_combobox(hwnd, IDC_TMPL_REMOTE, x + label_width + 10, cy, 200, 22);
    add_combobox_item(remote_combo, "Local");
    DIALOG_STATE.with(|s| {
        if let Some(ref state) = *s.borrow() {
            for name in &state.remote_names {
                add_combobox_item(remote_combo, name);
            }
        }
    });
    set_combobox_selection(remote_combo, 0);
    controls.push(remote_combo);

    // Checkboxes
    cy += row_height + 10;
    controls.push(create_checkbox(
        hwnd,
        IDC_TMPL_UNIQUE,
        "Unique (reuse existing tab)",
        x,
        cy,
        250,
        20,
    ));

    cy += row_height;
    controls.push(create_checkbox(
        hwnd,
        IDC_TMPL_KEEPOPEN,
        "Keep open after exit",
        x,
        cy,
        200,
        20,
    ));

    DIALOG_STATE.with(|s| {
        if let Some(ref mut state) = *s.borrow_mut() {
            state.general_controls = controls;
        }
    });
}

/// Create controls for the Docker tab
unsafe fn create_docker_controls(hwnd: HWND, x: i32, y: i32, w: i32, _h: i32) {
    let mut controls = Vec::new();
    let row_height = 26;
    let label_width = 100;
    let control_width = w - label_width - 20;

    // Enable Docker checkbox
    let mut cy = y;
    controls.push(create_checkbox(
        hwnd,
        IDC_DOCKER_ENABLE,
        "Enable Docker",
        x,
        cy,
        150,
        20,
    ));

    // Mode
    cy += row_height + 5;
    controls.push(create_label(hwnd, -1, "Mode:", x, cy + 3, label_width, 18));
    let mode_combo = create_combobox(hwnd, IDC_DOCKER_MODE, x + label_width + 10, cy, 150, 22);
    add_combobox_item(mode_combo, "Exec (existing container)");
    add_combobox_item(mode_combo, "Run (new container)");
    add_combobox_item(mode_combo, "DevContainer");
    controls.push(mode_combo);

    // Container (for exec mode)
    cy += row_height + 5;
    controls.push(create_label(
        hwnd,
        -1,
        "Container:",
        x,
        cy + 3,
        label_width,
        18,
    ));
    controls.push(create_edit(
        hwnd,
        IDC_DOCKER_CONTAINER,
        x + label_width + 10,
        cy,
        control_width,
        22,
    ));

    // Image (for run/devcontainer mode)
    cy += row_height + 5;
    controls.push(create_label(hwnd, -1, "Image:", x, cy + 3, label_width, 18));
    controls.push(create_edit(
        hwnd,
        IDC_DOCKER_IMAGE,
        x + label_width + 10,
        cy,
        control_width,
        22,
    ));

    // Shell
    cy += row_height + 5;
    controls.push(create_label(hwnd, -1, "Shell:", x, cy + 3, label_width, 18));
    controls.push(create_edit_with_text(
        hwnd,
        IDC_DOCKER_SHELL,
        "/bin/sh",
        x + label_width + 10,
        cy,
        150,
        22,
    ));

    // Auto-remove
    cy += row_height + 5;
    controls.push(create_checkbox(
        hwnd,
        IDC_DOCKER_AUTOREMOVE,
        "Auto-remove container on exit",
        x,
        cy,
        250,
        20,
    ));

    // Project directory (for devcontainer)
    cy += row_height + 5;
    controls.push(create_label(
        hwnd,
        -1,
        "Project dir:",
        x,
        cy + 3,
        label_width,
        18,
    ));
    controls.push(create_edit(
        hwnd,
        IDC_DOCKER_PROJECT,
        x + label_width + 10,
        cy,
        control_width,
        22,
    ));

    DIALOG_STATE.with(|s| {
        if let Some(ref mut state) = *s.borrow_mut() {
            state.docker_controls = controls;
        }
    });
}

/// Create controls for the SSH tab
unsafe fn create_ssh_controls(hwnd: HWND, x: i32, y: i32, w: i32, _h: i32) {
    let mut controls = Vec::new();
    let row_height = 26;
    let label_width = 100;
    let control_width = w - label_width - 20;

    // Enable SSH checkbox
    let mut cy = y;
    controls.push(create_checkbox(
        hwnd,
        IDC_SSH_ENABLE,
        "Enable SSH",
        x,
        cy,
        150,
        20,
    ));

    // Host
    cy += row_height + 5;
    controls.push(create_label(hwnd, -1, "Host:", x, cy + 3, label_width, 18));
    controls.push(create_edit(
        hwnd,
        IDC_SSH_HOST,
        x + label_width + 10,
        cy,
        control_width,
        22,
    ));

    // Port
    cy += row_height + 5;
    controls.push(create_label(hwnd, -1, "Port:", x, cy + 3, label_width, 18));
    controls.push(create_edit_with_text(
        hwnd,
        IDC_SSH_PORT,
        "22",
        x + label_width + 10,
        cy,
        80,
        22,
    ));

    // Username
    cy += row_height + 5;
    controls.push(create_label(
        hwnd,
        -1,
        "Username:",
        x,
        cy + 3,
        label_width,
        18,
    ));
    controls.push(create_edit(
        hwnd,
        IDC_SSH_USER,
        x + label_width + 10,
        cy,
        150,
        22,
    ));

    // Identity file
    cy += row_height + 5;
    controls.push(create_label(
        hwnd,
        -1,
        "Identity file:",
        x,
        cy + 3,
        label_width,
        18,
    ));
    controls.push(create_edit(
        hwnd,
        IDC_SSH_IDENTITY,
        x + label_width + 10,
        cy,
        control_width,
        22,
    ));

    // Jump host
    cy += row_height + 5;
    controls.push(create_label(
        hwnd,
        -1,
        "Jump host:",
        x,
        cy + 3,
        label_width,
        18,
    ));
    controls.push(create_edit(
        hwnd,
        IDC_SSH_JUMP,
        x + label_width + 10,
        cy,
        control_width,
        22,
    ));

    // Checkboxes
    cy += row_height + 10;
    controls.push(create_checkbox(
        hwnd,
        IDC_SSH_X11,
        "X11 forwarding",
        x,
        cy,
        150,
        20,
    ));
    controls.push(create_checkbox(
        hwnd,
        IDC_SSH_AGENT,
        "Agent forwarding",
        x + 160,
        cy,
        150,
        20,
    ));

    DIALOG_STATE.with(|s| {
        if let Some(ref mut state) = *s.borrow_mut() {
            state.ssh_controls = controls;
        }
    });
}

/// Show controls for a specific tab, hide others
fn show_tab(tab_index: i32) {
    DIALOG_STATE.with(|s| {
        if let Some(ref state) = *s.borrow() {
            // Hide all
            for hwnd in &state.general_controls {
                show_control(*hwnd, false);
            }
            for hwnd in &state.docker_controls {
                show_control(*hwnd, false);
            }
            for hwnd in &state.ssh_controls {
                show_control(*hwnd, false);
            }

            // Show the selected tab's controls
            let controls = match tab_index {
                TAB_GENERAL => &state.general_controls,
                TAB_DOCKER => &state.docker_controls,
                TAB_SSH => &state.ssh_controls,
                _ => &state.general_controls,
            };
            for hwnd in controls {
                show_control(*hwnd, true);
            }
        }
    });
}

/// Populate the template list combobox
fn populate_template_list(combo: HWND) {
    clear_combobox(combo);
    DIALOG_STATE.with(|s| {
        if let Some(ref state) = *s.borrow() {
            for template in &state.templates {
                add_combobox_item(combo, &template.name);
            }
        }
    });
}

/// Load the current template into controls
fn load_current_template() {
    DIALOG_STATE.with(|s| {
        if let Some(ref state) = *s.borrow() {
            if state.current_template >= state.templates.len() {
                return;
            }
            let template = &state.templates[state.current_template];

            // General tab
            if let Some(&edit) = state.general_controls.get(1) {
                set_edit_text(edit, &template.name);
            }
            if let Some(&edit) = state.general_controls.get(3) {
                set_edit_text(edit, template.command.as_deref().unwrap_or(""));
            }
            if let Some(&edit) = state.general_controls.get(5) {
                set_edit_text(edit, &template.args.join(" "));
            }
            if let Some(&edit) = state.general_controls.get(7) {
                set_edit_text(
                    edit,
                    template
                        .working_directory
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                        .as_deref()
                        .unwrap_or(""),
                );
            }
            if let Some(&edit) = state.general_controls.get(9) {
                set_edit_text(edit, template.git_remote.as_deref().unwrap_or(""));
            }
            if let Some(&edit) = state.general_controls.get(11) {
                set_edit_text(edit, template.color.as_deref().unwrap_or(""));
            }
            // Remote combo
            if let Some(&combo) = state.general_controls.get(14) {
                let idx = match &template.remote {
                    Some(remote_name) => state
                        .remote_names
                        .iter()
                        .position(|n| n == remote_name)
                        .map(|i| (i + 1) as i32)
                        .unwrap_or(0),
                    None => 0,
                };
                set_combobox_selection(combo, idx);
            }

            if let Some(&checkbox) = state.general_controls.get(15) {
                set_checkbox_state(checkbox, template.unique);
            }
            if let Some(&checkbox) = state.general_controls.get(16) {
                set_checkbox_state(checkbox, template.keep_open);
            }

            // Docker tab
            let has_docker = template.docker.is_some();
            if let Some(&checkbox) = state.docker_controls.first() {
                set_checkbox_state(checkbox, has_docker);
            }
            if let Some(ref docker) = template.docker {
                if let Some(&combo) = state.docker_controls.get(2) {
                    let idx = match docker.mode {
                        DockerMode::Exec => 0,
                        DockerMode::Run => 1,
                        DockerMode::DevContainer => 2,
                    };
                    set_combobox_selection(combo, idx);
                }
                if let Some(&edit) = state.docker_controls.get(4) {
                    set_edit_text(edit, docker.container.as_deref().unwrap_or(""));
                }
                if let Some(&edit) = state.docker_controls.get(6) {
                    set_edit_text(edit, docker.image.as_deref().unwrap_or(""));
                }
                if let Some(&edit) = state.docker_controls.get(8) {
                    set_edit_text(edit, docker.shell.as_deref().unwrap_or("/bin/sh"));
                }
                if let Some(&checkbox) = state.docker_controls.get(9) {
                    set_checkbox_state(checkbox, docker.auto_remove);
                }
                if let Some(&edit) = state.docker_controls.get(11) {
                    set_edit_text(
                        edit,
                        docker
                            .project_dir
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string())
                            .as_deref()
                            .unwrap_or(""),
                    );
                }
            } else {
                // Clear Docker fields
                if let Some(&combo) = state.docker_controls.get(2) {
                    set_combobox_selection(combo, 0);
                }
                for &edit in state.docker_controls.get(4..=8).into_iter().flatten() {
                    set_edit_text(edit, "");
                }
            }

            // SSH tab
            let has_ssh = template.ssh.is_some();
            if let Some(&checkbox) = state.ssh_controls.first() {
                set_checkbox_state(checkbox, has_ssh);
            }
            if let Some(ref ssh) = template.ssh {
                if let Some(&edit) = state.ssh_controls.get(2) {
                    set_edit_text(edit, &ssh.host);
                }
                if let Some(&edit) = state.ssh_controls.get(4) {
                    set_edit_text(edit, &ssh.port.unwrap_or(22).to_string());
                }
                if let Some(&edit) = state.ssh_controls.get(6) {
                    set_edit_text(edit, ssh.username.as_deref().unwrap_or(""));
                }
                if let Some(&edit) = state.ssh_controls.get(8) {
                    set_edit_text(
                        edit,
                        ssh.identity_file
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string())
                            .as_deref()
                            .unwrap_or(""),
                    );
                }
                if let Some(&edit) = state.ssh_controls.get(10) {
                    set_edit_text(edit, ssh.jump_host.as_deref().unwrap_or(""));
                }
                if let Some(&checkbox) = state.ssh_controls.get(11) {
                    set_checkbox_state(checkbox, ssh.x11_forward);
                }
                if let Some(&checkbox) = state.ssh_controls.get(12) {
                    set_checkbox_state(checkbox, ssh.agent_forward);
                }
            } else {
                // Clear SSH fields
                for &edit in state.ssh_controls.get(2..=10).into_iter().flatten() {
                    set_edit_text(edit, "");
                }
            }
        }
    });
}

/// Save the current template from controls
fn save_current_template() {
    DIALOG_STATE.with(|s| {
        if let Some(ref mut state) = *s.borrow_mut() {
            if state.current_template >= state.templates.len() {
                return;
            }

            let template = &mut state.templates[state.current_template];

            // General tab
            if let Some(&edit) = state.general_controls.get(1) {
                template.name = get_edit_text(edit);
            }
            if let Some(&edit) = state.general_controls.get(3) {
                let cmd = get_edit_text(edit);
                template.command = if cmd.is_empty() { None } else { Some(cmd) };
            }
            if let Some(&edit) = state.general_controls.get(5) {
                let args = get_edit_text(edit);
                template.args = if args.is_empty() {
                    Vec::new()
                } else {
                    args.split_whitespace().map(|s| s.to_string()).collect()
                };
            }
            if let Some(&edit) = state.general_controls.get(7) {
                let path = get_edit_text(edit);
                template.working_directory = if path.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(path))
                };
            }
            if let Some(&edit) = state.general_controls.get(9) {
                let git_remote = get_edit_text(edit);
                template.git_remote = if git_remote.is_empty() {
                    None
                } else {
                    Some(git_remote)
                };
            }
            if let Some(&edit) = state.general_controls.get(11) {
                let color = get_edit_text(edit);
                template.color = if color.is_empty() { None } else { Some(color) };
            }
            // Remote combo
            if let Some(&combo) = state.general_controls.get(14) {
                template.remote = match get_combobox_selection(combo) {
                    Some(idx) if idx > 0 => state.remote_names.get((idx - 1) as usize).cloned(),
                    _ => None,
                };
            }

            if let Some(&checkbox) = state.general_controls.get(15) {
                template.unique = get_checkbox_state(checkbox);
            }
            if let Some(&checkbox) = state.general_controls.get(16) {
                template.keep_open = get_checkbox_state(checkbox);
            }

            // Docker tab
            let docker_enabled = state
                .docker_controls
                .first()
                .map(|&c| get_checkbox_state(c))
                .unwrap_or(false);

            if docker_enabled {
                let mut docker = template.docker.take().unwrap_or_default();

                if let Some(&combo) = state.docker_controls.get(2) {
                    docker.mode = match get_combobox_selection(combo) {
                        Some(0) => DockerMode::Exec,
                        Some(1) => DockerMode::Run,
                        Some(2) => DockerMode::DevContainer,
                        _ => DockerMode::Exec,
                    };
                }
                if let Some(&edit) = state.docker_controls.get(4) {
                    let val = get_edit_text(edit);
                    docker.container = if val.is_empty() { None } else { Some(val) };
                }
                if let Some(&edit) = state.docker_controls.get(6) {
                    let val = get_edit_text(edit);
                    docker.image = if val.is_empty() { None } else { Some(val) };
                }
                if let Some(&edit) = state.docker_controls.get(8) {
                    let val = get_edit_text(edit);
                    docker.shell = if val.is_empty() { None } else { Some(val) };
                }
                if let Some(&checkbox) = state.docker_controls.get(9) {
                    docker.auto_remove = get_checkbox_state(checkbox);
                }
                if let Some(&edit) = state.docker_controls.get(11) {
                    let val = get_edit_text(edit);
                    docker.project_dir = if val.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(val))
                    };
                }

                template.docker = Some(docker);
            } else {
                template.docker = None;
            }

            // SSH tab
            let ssh_enabled = state
                .ssh_controls
                .first()
                .map(|&c| get_checkbox_state(c))
                .unwrap_or(false);

            if ssh_enabled {
                let mut ssh = template.ssh.take().unwrap_or_default();

                if let Some(&edit) = state.ssh_controls.get(2) {
                    ssh.host = get_edit_text(edit);
                }
                if let Some(&edit) = state.ssh_controls.get(4) {
                    let port_str = get_edit_text(edit);
                    ssh.port = port_str.parse().ok();
                }
                if let Some(&edit) = state.ssh_controls.get(6) {
                    let val = get_edit_text(edit);
                    ssh.username = if val.is_empty() { None } else { Some(val) };
                }
                if let Some(&edit) = state.ssh_controls.get(8) {
                    let val = get_edit_text(edit);
                    ssh.identity_file = if val.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(val))
                    };
                }
                if let Some(&edit) = state.ssh_controls.get(10) {
                    let val = get_edit_text(edit);
                    ssh.jump_host = if val.is_empty() { None } else { Some(val) };
                }
                if let Some(&checkbox) = state.ssh_controls.get(11) {
                    ssh.x11_forward = get_checkbox_state(checkbox);
                }
                if let Some(&checkbox) = state.ssh_controls.get(12) {
                    ssh.agent_forward = get_checkbox_state(checkbox);
                }

                template.ssh = Some(ssh);
            } else {
                template.ssh = None;
            }
        }
    });
}

/// Handle WM_COMMAND
fn handle_command(hwnd: HWND, id: i32, code: u16) {
    match id {
        IDOK => {
            save_current_template();
            let result = DIALOG_STATE.with(|s| {
                if let Some(ref state) = *s.borrow() {
                    cterm_app::save_sticky_tabs(&state.templates)
                } else {
                    Ok(())
                }
            });

            if result.is_ok() {
                unsafe { EndDialog(hwnd, IDOK as isize) };
            } else {
                crate::dialogs::show_error(hwnd, "Error", "Failed to save templates");
            }
        }
        IDCANCEL => {
            unsafe { EndDialog(hwnd, IDCANCEL as isize) };
        }
        IDC_TEMPLATE_LIST if code == CBN_SELCHANGE => {
            // Save current, switch to selected
            save_current_template();
            let combo = get_dialog_item(hwnd, IDC_TEMPLATE_LIST);
            if let Some(idx) = get_combobox_selection(combo) {
                DIALOG_STATE.with(|s| {
                    if let Some(ref mut state) = *s.borrow_mut() {
                        state.current_template = idx as usize;
                    }
                });
                load_current_template();
            }
            // Update the combobox items to reflect name changes
            let combo = get_dialog_item(hwnd, IDC_TEMPLATE_LIST);
            populate_template_list(combo);
            DIALOG_STATE.with(|s| {
                if let Some(ref state) = *s.borrow() {
                    set_combobox_selection(combo, state.current_template as i32);
                }
            });
        }
        IDC_ADD_TEMPLATE => {
            save_current_template();
            DIALOG_STATE.with(|s| {
                if let Some(ref mut state) = *s.borrow_mut() {
                    let new_template = StickyTabConfig {
                        name: format!("New Template {}", state.templates.len() + 1),
                        ..Default::default()
                    };
                    state.templates.push(new_template);
                    state.current_template = state.templates.len() - 1;
                }
            });
            let combo = get_dialog_item(hwnd, IDC_TEMPLATE_LIST);
            populate_template_list(combo);
            DIALOG_STATE.with(|s| {
                if let Some(ref state) = *s.borrow() {
                    set_combobox_selection(combo, state.current_template as i32);
                }
            });
            load_current_template();
            update_remove_button(hwnd);
        }
        IDC_REMOVE_TEMPLATE => {
            DIALOG_STATE.with(|s| {
                if let Some(ref mut state) = *s.borrow_mut() {
                    if !state.templates.is_empty() {
                        state.templates.remove(state.current_template);
                        if state.current_template >= state.templates.len()
                            && !state.templates.is_empty()
                        {
                            state.current_template = state.templates.len() - 1;
                        } else if state.templates.is_empty() {
                            state.current_template = 0;
                        }
                    }
                }
            });
            let combo = get_dialog_item(hwnd, IDC_TEMPLATE_LIST);
            populate_template_list(combo);
            DIALOG_STATE.with(|s| {
                if let Some(ref state) = *s.borrow() {
                    if !state.templates.is_empty() {
                        set_combobox_selection(combo, state.current_template as i32);
                    }
                }
            });
            load_current_template();
            update_remove_button(hwnd);
        }
        IDC_PRESETS if code == CBN_SELCHANGE => {
            let combo = get_dialog_item(hwnd, IDC_PRESETS);
            if let Some(idx) = get_combobox_selection(combo) {
                if idx > 0 {
                    // idx 0 is "Add preset...", actual presets start at 1
                    let preset_idx = (idx - 1) as usize;
                    if preset_idx < PRESETS.len() {
                        save_current_template();
                        let new_template = (PRESETS[preset_idx].1)();
                        DIALOG_STATE.with(|s| {
                            if let Some(ref mut state) = *s.borrow_mut() {
                                state.templates.push(new_template);
                                state.current_template = state.templates.len() - 1;
                            }
                        });
                        let template_combo = get_dialog_item(hwnd, IDC_TEMPLATE_LIST);
                        populate_template_list(template_combo);
                        DIALOG_STATE.with(|s| {
                            if let Some(ref state) = *s.borrow() {
                                set_combobox_selection(
                                    template_combo,
                                    state.current_template as i32,
                                );
                            }
                        });
                        load_current_template();
                        update_remove_button(hwnd);
                    }
                }
                // Reset presets combo to "Add preset..."
                set_combobox_selection(combo, 0);
            }
        }
        IDC_TMPL_COLOR_BTN => {
            // Show color picker
            if let Some(color) = crate::dialogs::show_color_picker(hwnd) {
                let color_str = format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b);
                let edit = get_dialog_item(hwnd, IDC_TMPL_COLOR);
                set_edit_text(edit, &color_str);
            }
        }
        _ => {}
    }
}

/// Handle WM_NOTIFY
fn handle_notify(hwnd: HWND, nmhdr: &NMHDR) {
    match nmhdr.code {
        TCN_SELCHANGE if nmhdr.idFrom == IDC_TABS as usize => {
            let tab_ctrl = get_dialog_item(hwnd, IDC_TABS);
            let new_tab = get_selected_tab(tab_ctrl);

            DIALOG_STATE.with(|s| {
                if let Some(ref mut state) = *s.borrow_mut() {
                    state.current_tab = new_tab;
                }
            });

            show_tab(new_tab);
        }
        _ => {}
    }
}

/// Update the remove button state
fn update_remove_button(hwnd: HWND) {
    let remove_btn = get_dialog_item(hwnd, IDC_REMOVE_TEMPLATE);
    let has_templates = DIALOG_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| !state.templates.is_empty())
            .unwrap_or(false)
    });
    enable_control(remove_btn, has_templates);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_presets_defined() {
        assert!(!PRESETS.is_empty());
        // Verify all presets can be created
        for (name, factory) in PRESETS {
            let template = factory();
            assert!(
                !template.name.is_empty(),
                "Preset '{}' has empty name",
                name
            );
        }
    }
}
