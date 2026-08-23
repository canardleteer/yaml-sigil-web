//! DOM wiring for the playground shell.

use std::cell::{Cell, RefCell};

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{
    Document, HtmlButtonElement, HtmlElement, HtmlInputElement, HtmlSelectElement,
    HtmlTextAreaElement, KeyboardEvent,
};

use crate::identicon;
use crate::identities::{Roster, short_alg, trunc_hex};
use crate::ops::{self, ED25519_NAME, OpResult};
use crate::proto::{self, ArtifactFields, SignatureFields};

const SAMPLE_YAML: &str = r#"# A supply-wagon manifest
claim: ridge-line cache
season: 2026
stores:
  - pemmican
  - lamp oil
"#;

const WINDOWS: [&str; 6] = [
    "validate",
    "identity",
    "sign",
    "verify",
    "decompose",
    "compose",
];
const LIVE_DELAY_MS: i32 = 90;
const DEFAULT_IDENTITY: &str = "alice";
const NEW_IDENTITY_ID: &str = "__new__";

thread_local! {
    static VERIFY_TIMER: Cell<i32> = const { Cell::new(-1) };
    static DECOMPOSE_TIMER: Cell<i32> = const { Cell::new(-1) };
    static VALIDATE_TIMER: Cell<i32> = const { Cell::new(-1) };
    static COMPOSE_CARRIER_TIMER: Cell<i32> = const { Cell::new(-1) };
    static PROTO_SYNCING: Cell<bool> = const { Cell::new(false) };
    static IDENTITY_SYNCING: Cell<bool> = const { Cell::new(false) };
    static ROSTER: RefCell<Roster> = RefCell::new(Roster::default());
    static SELECTED_ID: RefCell<String> = RefCell::new(NEW_IDENTITY_ID.to_string());
    static CURRENT_ID: RefCell<String> = RefCell::new(DEFAULT_IDENTITY.to_string());
    static LAST_COMPOSE_FORM: RefCell<String> = const { RefCell::new(String::new()) };
    static LAST_DECOMPOSE_FORM: RefCell<String> = const { RefCell::new(String::new()) };
}

pub fn boot() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };

    bind_windows(&document);
    bind_identity(&document);
    bind_validate(&document);
    bind_sign(&document);
    bind_send(&document);
    bind_verify(&document);
    bind_compose(&document);
    bind_decompose(&document);
    bind_form_toggles(&document);
    bind_current_id_menu(&document);

    if let Some(yaml) = textarea(&document, "validate-yaml") {
        yaml.set_value(SAMPLE_YAML);
    }
    if let Some(payload) = textarea(&document, "sign-payload") {
        payload.set_value(SAMPLE_YAML);
    }

    seed_roster(&document);
    open_from_hash(&document);
}

fn bind_windows(document: &Document) {
    let Ok(openers) = document.query_selector_all("[data-open-window]") else {
        return;
    };
    for index in 0..openers.length() {
        let Some(node) = openers.item(index) else {
            continue;
        };
        let Ok(button) = node.dyn_into::<HtmlButtonElement>() else {
            continue;
        };
        let name = button.get_attribute("data-open-window").unwrap_or_default();
        let closure = Closure::<dyn FnMut()>::new(move || set_hash(&name));
        let _ = button.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }

    let Ok(closers) = document.query_selector_all("[data-close-window]") else {
        bind_hashchange(document);
        return;
    };
    for index in 0..closers.length() {
        let Some(node) = closers.item(index) else {
            continue;
        };
        let Ok(button) = node.dyn_into::<HtmlButtonElement>() else {
            continue;
        };
        let closure = Closure::<dyn FnMut()>::new(move || set_hash(""));
        let _ = button.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }

    bind_hashchange(document);
}

fn bind_hashchange(document: &Document) {
    let document_hash = document.clone();
    let on_hash = Closure::<dyn FnMut()>::new(move || open_from_hash(&document_hash));
    if let Some(window) = web_sys::window() {
        let _ =
            window.add_event_listener_with_callback("hashchange", on_hash.as_ref().unchecked_ref());
    }
    on_hash.forget();
}

fn set_hash(name: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(location) = window.location().hash() else {
        return;
    };
    let next = if name.is_empty() {
        String::new()
    } else {
        format!("#{name}")
    };
    if location != next {
        let _ = window.location().set_hash(name);
    }
}

fn open_from_hash(document: &Document) {
    let hash = web_sys::window()
        .and_then(|window| window.location().hash().ok())
        .unwrap_or_default();
    let name = hash.trim_start_matches('#').trim();
    if WINDOWS.contains(&name) {
        show_window(document, name);
    } else {
        show_deck(document);
    }
}

fn show_deck(document: &Document) {
    if let Some(body) = document.body() {
        let _ = body.class_list().remove_1("in-window");
    }
    if let Some(deck) = document.get_element_by_id("deck") {
        let _ = deck.remove_attribute("hidden");
    }
    for name in WINDOWS {
        set_window_hidden(document, name, true);
    }
}

fn show_window(document: &Document, name: &str) {
    if let Some(body) = document.body() {
        let _ = body.class_list().add_1("in-window");
    }
    if let Some(deck) = document.get_element_by_id("deck") {
        let _ = deck.set_attribute("hidden", "");
    }
    for window_name in WINDOWS {
        set_window_hidden(document, window_name, window_name != name);
    }
    if name == "validate" {
        schedule_validate(document);
    }
    if name == "sign" {
        copy_validate_to_sign(document);
    }
    if name == "verify" {
        schedule_verify(document);
    }
    if name == "compose" {
        update_compose_proto_mode(document);
    }
    if name == "decompose" {
        schedule_decompose(document);
    }
}

fn set_window_hidden(document: &Document, name: &str, hidden: bool) {
    let Ok(nodes) = document.query_selector_all(&format!("[data-window=\"{name}\"]")) else {
        return;
    };
    for index in 0..nodes.length() {
        if let Some(el) = nodes
            .item(index)
            .and_then(|node| node.dyn_into::<web_sys::Element>().ok())
        {
            if hidden {
                let _ = el.set_attribute("hidden", "");
            } else {
                let _ = el.remove_attribute("hidden");
            }
        }
    }
}

fn seed_roster(document: &Document) {
    match Roster::seed() {
        Ok(roster) => {
            with_roster_mut(|slot| *slot = roster);
            set_current_id(DEFAULT_IDENTITY);
            set_selected_id(NEW_IDENTITY_ID);
            refresh_identity_ui(document);
            refresh_current_id_ui(document, false);
            set_status(document, "identity-status", "", "idle");
        }
        Err(error) => set_status(document, "identity-status", "err", &error),
    }
}

fn bind_identity(document: &Document) {
    if let Some(list) = document.get_element_by_id("identity-list") {
        let document = document.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let Some(target) = event.target() else {
                return;
            };
            let Ok(el) = target.dyn_into::<web_sys::Element>() else {
                return;
            };
            let Some(row) = el.closest("[data-identity-id]").ok().flatten() else {
                return;
            };
            let Some(id) = row.get_attribute("data-identity-id") else {
                return;
            };
            if id == NEW_IDENTITY_ID {
                set_selected_id(&id);
                refresh_identity_detail(&document);
                highlight_identity_rows(&document);
                return;
            }
            adopt_working_identity(&document, &id);
        });
        let _ = list.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    for id in ["btn-identity-mint", "btn-identity-add-bar"] {
        bind_click(document, id, mint_identity);
    }
    bind_click(document, "btn-identity-add", add_pasted_identity);
    bind_click(document, "btn-identity-remint", remint_selected);
    bind_click(document, "btn-identity-delete", delete_selected);
    bind_input_event(document, "identity-keyid", "input", persist_selected_keyid);
    for id in ["identity-private", "identity-public"] {
        bind_input_event(document, id, "input", persist_selected_keys);
    }
    bind_input_event(
        document,
        "identity-verify-only",
        "change",
        toggle_verify_only_form,
    );
    bind_input_event(
        document,
        "identity-new-public",
        "input",
        refresh_add_identicon,
    );
    bind_input_event(
        document,
        "identity-new-algorithm",
        "change",
        refresh_add_identicon,
    );
    bind_working_identity_select(document, "sign-identity");
    bind_working_identity_select(document, "verify-identity");
}

fn mint_identity(document: &Document) {
    let label = input_value(document, "identity-new-name");
    let keyid = input_value(document, "identity-new-keyid");
    let algorithm =
        select_value(document, "identity-new-algorithm").unwrap_or_else(|| ED25519_NAME.into());
    let verify_only = checkbox_checked(document, "identity-verify-only");
    let result = if verify_only {
        with_roster_mut(|roster| roster.mint_verify_only(&label, &algorithm, &keyid))
    } else {
        with_roster_mut(|roster| roster.add(&label, &algorithm, &keyid))
    };
    finish_new_identity(
        document,
        result,
        if verify_only {
            "Minted {id} (verify only) for this session."
        } else {
            "Minted {id} for this session."
        },
    );
}

fn add_pasted_identity(document: &Document) {
    let label = input_value(document, "identity-new-name");
    let keyid = input_value(document, "identity-new-keyid");
    let algorithm =
        select_value(document, "identity-new-algorithm").unwrap_or_else(|| ED25519_NAME.into());
    let public = input_value(document, "identity-new-public");
    let result =
        with_roster_mut(|roster| roster.add_verify_only(&label, &algorithm, &public, &keyid));
    finish_new_identity(
        document,
        result,
        "Added {id} (verify only) for this session.",
    );
}

fn finish_new_identity(document: &Document, result: Result<String, String>, ok_template: &str) {
    match result {
        Ok(id) => {
            set_input(document, "identity-new-name", "");
            set_input(document, "identity-new-keyid", "");
            set_input(document, "identity-new-public", "");
            set_checkbox(document, "identity-verify-only", false);
            toggle_verify_only_form(document);
            set_current_id(&id);
            set_selected_id(&id);
            refresh_identity_ui(document);
            refresh_current_id_ui(document, true);
            set_status(
                document,
                "identity-status",
                "ok",
                &ok_template.replace("{id}", &id),
            );
        }
        Err(error) => set_status(document, "identity-status", "err", &error),
    }
}

fn remint_selected(document: &Document) {
    let id = selected_id();
    let algorithm =
        select_value(document, "identity-algorithm").unwrap_or_else(|| ED25519_NAME.into());
    match with_roster_mut(|roster| roster.remint(&id, &algorithm)) {
        Ok(()) => {
            refresh_identity_ui(document);
            refresh_current_id_ui(document, false);
            set_status(
                document,
                "identity-status",
                "ok",
                &format!("Reminted {id}."),
            );
            if is_open(document, "verify") {
                schedule_verify(document);
            }
        }
        Err(error) => set_status(document, "identity-status", "err", &error),
    }
}

fn delete_selected(document: &Document) {
    let id = selected_id();
    match with_roster_mut(|roster| roster.delete(&id)) {
        Ok(()) => {
            let current_removed = current_id() == id;
            if selected_id() == id {
                let next = if current_removed {
                    DEFAULT_IDENTITY.to_string()
                } else {
                    current_id()
                };
                set_selected_id(&next);
            }
            if current_removed {
                set_current_id(DEFAULT_IDENTITY);
            }
            refresh_identity_ui(document);
            refresh_current_id_ui(document, current_removed);
            set_status(document, "identity-status", "ok", &format!("Removed {id}."));
            if is_open(document, "verify") {
                schedule_verify(document);
            }
        }
        Err(error) => set_status(document, "identity-status", "err", &error),
    }
}

fn persist_selected_keyid(document: &Document) {
    if identity_syncing() {
        return;
    }
    let id = selected_id();
    if id == NEW_IDENTITY_ID {
        return;
    }
    let keyid = input_value(document, "identity-keyid");
    if let Err(error) = with_roster_mut(|roster| roster.update_keyid(&id, &keyid)) {
        set_status(document, "identity-status", "err", &error);
        return;
    }
    let sign_id = select_value(document, "sign-identity").unwrap_or_else(current_id);
    if sign_id == id {
        sync_sign_keyid(document);
    }
}

fn persist_selected_keys(document: &Document) {
    if identity_syncing() {
        return;
    }
    let id = selected_id();
    if id == NEW_IDENTITY_ID {
        return;
    }
    let private = input_value(document, "identity-private");
    let public = input_value(document, "identity-public");
    let verify_only =
        with_roster(|roster| roster.get(&id).is_some_and(|identity| identity.verify_only));
    let private = if verify_only { String::new() } else { private };
    if let Err(error) = with_roster_mut(|roster| roster.update_keys(&id, &private, &public)) {
        set_status(document, "identity-status", "err", &error);
        return;
    }
    refresh_identity_list(document);
    refresh_identity_select_icons(document);
    refresh_current_id_ui(document, false);
    let algorithm = with_roster(|roster| {
        roster
            .get(&id)
            .map(|identity| identity.algorithm.clone())
            .unwrap_or_default()
    });
    set_identicon(document, "identity-public-icon", &algorithm, &public);
    set_identicon(document, "identity-selected-icon", &algorithm, &public);
    if is_open(document, "verify") {
        schedule_verify(document);
    }
}

fn refresh_identity_ui(document: &Document) {
    close_current_id_menu(document);
    refresh_identity_list(document);
    fill_identity_select(document, "sign-identity");
    fill_identity_select(document, "verify-identity");
    refresh_identity_select_icons(document);
    refresh_identity_detail(document);
    sync_sign_keyid(document);
}

fn refresh_identity_list(document: &Document) {
    let Some(list) = document.get_element_by_id("identity-list") else {
        return;
    };
    list.set_inner_html("");
    let selected = selected_id();
    let identities = with_roster(|roster| roster.list().to_vec());
    for identity in identities {
        let Ok(row) = document.create_element("button") else {
            continue;
        };
        let _ = row.set_attribute("type", "button");
        let _ = row.set_attribute("data-identity-id", &identity.id);
        row.set_class_name(if identity.id == selected {
            "identity-row selected"
        } else {
            "identity-row"
        });
        append_identicon(document, &row, &identity.algorithm, &identity.public_hex);
        if let Ok(copy) = document.create_element("span") {
            copy.set_class_name("identity-copy");
            if let Ok(name) = document.create_element("span") {
                name.set_class_name("identity-name");
                name.set_text_content(Some(&identity.display_name()));
                let _ = copy.append_child(&name);
            }
            if let Ok(meta) = document.create_element("span") {
                meta.set_class_name("identity-meta");
                meta.set_text_content(Some(&format!(
                    "{} · {}",
                    short_alg(&identity.algorithm),
                    trunc_hex(&identity.public_hex)
                )));
                let _ = copy.append_child(&meta);
            }
            let _ = row.append_child(&copy);
        }
        let _ = list.append_child(&row);
    }
    let Ok(new_row) = document.create_element("button") else {
        return;
    };
    let _ = new_row.set_attribute("type", "button");
    let _ = new_row.set_attribute("data-identity-id", NEW_IDENTITY_ID);
    new_row.set_class_name(if selected == NEW_IDENTITY_ID {
        "identity-row identity-new selected"
    } else {
        "identity-row identity-new"
    });
    if let Ok(name) = document.create_element("span") {
        name.set_class_name("identity-name");
        name.set_text_content(Some("+ New Identity"));
        let _ = new_row.append_child(&name);
    }
    let _ = list.append_child(&new_row);
}

fn highlight_identity_rows(document: &Document) {
    let Some(list) = document.get_element_by_id("identity-list") else {
        return;
    };
    let selected = selected_id();
    let Ok(rows) = list.query_selector_all("[data-identity-id]") else {
        return;
    };
    for index in 0..rows.length() {
        let Some(el) = rows
            .item(index)
            .and_then(|node| node.dyn_into::<web_sys::Element>().ok())
        else {
            continue;
        };
        let id = el.get_attribute("data-identity-id").unwrap_or_default();
        let _ = el.class_list().remove_1("selected");
        if id == selected {
            let _ = el.class_list().add_1("selected");
        }
    }
}

fn refresh_identity_detail(document: &Document) {
    let selected = selected_id();
    let adding = selected == NEW_IDENTITY_ID;
    set_hidden(document, "identity-add-panel", !adding);
    set_hidden(document, "identity-selected-panel", adding);
    set_hidden(document, "btn-identity-add-bar", !adding);
    if adding {
        set_hidden(document, "identity-actions", true);
        return;
    }
    let Some(identity) = with_roster(|roster| roster.get(&selected).cloned()) else {
        set_selected_id(NEW_IDENTITY_ID);
        set_hidden(document, "identity-add-panel", false);
        set_hidden(document, "identity-selected-panel", true);
        set_hidden(document, "btn-identity-add-bar", false);
        set_hidden(document, "identity-actions", true);
        return;
    };
    with_identity_sync(|| {
        if let Some(title) = document.get_element_by_id("identity-selected-title") {
            title.set_text_content(Some(&identity.display_name()));
        }
        set_select(document, "identity-algorithm", &identity.algorithm);
        set_input(document, "identity-keyid", &identity.keyid);
        set_input(document, "identity-private", &identity.private_hex);
        set_input(document, "identity-public", &identity.public_hex);
        set_readonly(
            document,
            "identity-private",
            identity.preset || identity.verify_only,
        );
        set_readonly(document, "identity-public", identity.preset);
        if let Some(algorithm) = select(document, "identity-algorithm") {
            algorithm.set_disabled(identity.preset || identity.verify_only);
        }
        set_identicon(
            document,
            "identity-selected-icon",
            &identity.algorithm,
            &identity.public_hex,
        );
        set_identicon(
            document,
            "identity-public-icon",
            &identity.algorithm,
            &identity.public_hex,
        );
        set_hidden(document, "identity-actions", identity.preset);
        set_hidden(document, "btn-identity-remint", identity.verify_only);
    });
}

fn fill_identity_select(document: &Document, id: &str) {
    let Some(select) = select(document, id) else {
        return;
    };
    let previous = select.value();
    while select.length() > 0 {
        select.remove_with_index(0);
    }
    let identities = with_roster(|roster| roster.list().to_vec());
    for identity in &identities {
        let Ok(option) = document.create_element("option") else {
            continue;
        };
        let _ = option.set_attribute("value", &identity.id);
        option.set_text_content(Some(&format!(
            "{} · {}",
            identity.display_name(),
            short_alg(&identity.algorithm)
        )));
        let _ = select.append_child(&option);
    }
    let current = current_id();
    let fallback = if identities.iter().any(|identity| identity.id == current) {
        current
    } else if identities.iter().any(|identity| identity.id == previous) {
        previous
    } else {
        DEFAULT_IDENTITY.to_string()
    };
    select.set_value(&fallback);
}

fn sync_sign_keyid(document: &Document) {
    let id = select_value(document, "sign-identity").unwrap_or_else(current_id);
    let keyid = with_roster(|roster| {
        roster
            .get(&id)
            .map(|identity| identity.keyid.clone())
            .unwrap_or_default()
    });
    set_input(document, "sign-keyid", &keyid);
}

fn with_roster<T>(f: impl FnOnce(&Roster) -> T) -> T {
    ROSTER.with(|cell| f(&cell.borrow()))
}

fn with_roster_mut<T>(f: impl FnOnce(&mut Roster) -> T) -> T {
    ROSTER.with(|cell| f(&mut cell.borrow_mut()))
}

fn selected_id() -> String {
    SELECTED_ID.with(|cell| cell.borrow().clone())
}

fn set_selected_id(id: &str) {
    SELECTED_ID.with(|cell| *cell.borrow_mut() = id.to_string());
}

fn current_id() -> String {
    CURRENT_ID.with(|cell| cell.borrow().clone())
}

fn set_current_id(id: &str) {
    CURRENT_ID.with(|cell| *cell.borrow_mut() = id.to_string());
}

fn bind_working_identity_select(document: &Document, select_id: &'static str) {
    let Some(el) = document.get_element_by_id(select_id) else {
        return;
    };
    let document = document.clone();
    let closure = Closure::<dyn FnMut()>::new(move || {
        if identity_syncing() {
            return;
        }
        if let Some(id) = select_value(&document, select_id) {
            adopt_working_identity(&document, &id);
        }
    });
    let _ = el.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
    closure.forget();
}

fn bind_current_id_menu(document: &Document) {
    let Ok(chips) = document.query_selector_all(".current-id") else {
        return;
    };
    for index in 0..chips.length() {
        let Some(el) = chips.item(index) else {
            continue;
        };
        let document = document.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            event.stop_propagation();
            let Some(target) = event
                .current_target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            else {
                return;
            };
            toggle_current_id_menu(&document, &target);
        });
        let _ = el.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }

    if let Some(menu) = document.get_element_by_id("current-id-menu") {
        let document = document.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            event.stop_propagation();
            let Some(target) = event
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            else {
                return;
            };
            let Some(row) = target.closest("[data-identity-id]").ok().flatten() else {
                return;
            };
            let Some(id) = row.get_attribute("data-identity-id") else {
                return;
            };
            close_current_id_menu(&document);
            adopt_working_identity(&document, &id);
        });
        let _ = menu.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }

    let document_click = document.clone();
    let on_doc = Closure::<dyn FnMut()>::new(move || close_current_id_menu(&document_click));
    let _ = document.add_event_listener_with_callback("click", on_doc.as_ref().unchecked_ref());
    on_doc.forget();

    let document_key = document.clone();
    let on_key = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
        if event.key() == "Escape" {
            close_current_id_menu(&document_key);
        }
    });
    if let Some(window) = web_sys::window() {
        let _ = window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    }
    on_key.forget();
}

fn toggle_current_id_menu(document: &Document, chip: &web_sys::Element) {
    let expanded = chip.get_attribute("aria-expanded").as_deref() == Some("true");
    if expanded {
        close_current_id_menu(document);
        return;
    }
    open_current_id_menu(document, chip);
}

fn close_current_id_menu(document: &Document) {
    set_hidden(document, "current-id-menu", true);
    let Ok(chips) = document.query_selector_all(".current-id") else {
        return;
    };
    for index in 0..chips.length() {
        if let Some(el) = chips
            .item(index)
            .and_then(|node| node.dyn_into::<web_sys::Element>().ok())
        {
            let _ = el.set_attribute("aria-expanded", "false");
        }
    }
}

fn open_current_id_menu(document: &Document, chip: &web_sys::Element) {
    close_current_id_menu(document);
    fill_current_id_menu(document);
    let _ = chip.set_attribute("aria-expanded", "true");
    set_hidden(document, "current-id-menu", false);
    position_current_id_menu(document, chip);
}

fn fill_current_id_menu(document: &Document) {
    let Some(menu) = document.get_element_by_id("current-id-menu") else {
        return;
    };
    menu.set_inner_html("");
    let current = current_id();
    let identities = with_roster(|roster| roster.list().to_vec());
    for identity in identities {
        let Ok(row) = document.create_element("button") else {
            continue;
        };
        let _ = row.set_attribute("type", "button");
        let _ = row.set_attribute("role", "option");
        let _ = row.set_attribute("data-identity-id", &identity.id);
        row.set_class_name(if identity.id == current {
            "identity-row selected"
        } else {
            "identity-row"
        });
        if identity.id == current {
            let _ = row.set_attribute("aria-selected", "true");
        }
        append_identicon(document, &row, &identity.algorithm, &identity.public_hex);
        if let Ok(copy) = document.create_element("span") {
            copy.set_class_name("identity-copy");
            if let Ok(name) = document.create_element("span") {
                name.set_class_name("identity-name");
                name.set_text_content(Some(&identity.display_name()));
                let _ = copy.append_child(&name);
            }
            if let Ok(meta) = document.create_element("span") {
                meta.set_class_name("identity-meta");
                meta.set_text_content(Some(&format!(
                    "{} · {}",
                    short_alg(&identity.algorithm),
                    trunc_hex(&identity.public_hex)
                )));
                let _ = copy.append_child(&meta);
            }
            let _ = row.append_child(&copy);
        }
        let _ = menu.append_child(&row);
    }
}

fn position_current_id_menu(document: &Document, chip: &web_sys::Element) {
    let Some(menu) = document
        .get_element_by_id("current-id-menu")
        .and_then(|el| el.dyn_into::<HtmlElement>().ok())
    else {
        return;
    };
    let Some(window) = web_sys::window() else {
        return;
    };
    let chip_rect = chip.get_bounding_client_rect();
    let menu_rect = menu.get_bounding_client_rect();
    let vw = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(800.0);
    let vh = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(600.0);
    let mut left = chip_rect.left();
    let width = menu_rect.width().max(12.0);
    if left + width > vw - 8.0 {
        left = (vw - width - 8.0).max(8.0);
    }
    if left < 8.0 {
        left = 8.0;
    }
    let mut top = chip_rect.bottom() + 4.0;
    let height = menu_rect.height();
    if top + height > vh - 8.0 && chip_rect.top() > height + 8.0 {
        top = chip_rect.top() - height - 4.0;
    }
    let _ = menu.style().set_property("left", &format!("{left:.0}px"));
    let _ = menu.style().set_property("top", &format!("{top:.0}px"));
}

fn adopt_working_identity(document: &Document, id: &str) {
    if id.is_empty() || id == NEW_IDENTITY_ID {
        return;
    }
    let Some(identity) = with_roster(|roster| roster.get(id).cloned()) else {
        return;
    };
    let changed = current_id() != identity.id;
    set_current_id(&identity.id);
    set_selected_id(&identity.id);
    with_identity_sync(|| {
        set_select(document, "sign-identity", &identity.id);
        set_select(document, "verify-identity", &identity.id);
    });
    sync_sign_keyid(document);
    refresh_identity_select_icons(document);
    highlight_identity_rows(document);
    refresh_identity_detail(document);
    refresh_current_id_ui(document, changed);
    if is_open(document, "verify") {
        schedule_verify(document);
    }
}

fn refresh_current_id_ui(document: &Document, flash: bool) {
    let id = current_id();
    let identity = with_roster(|roster| roster.get(&id).cloned());
    let name = identity
        .as_ref()
        .map(|identity| identity.display_name())
        .unwrap_or_else(|| DEFAULT_IDENTITY.to_string());
    let algorithm = identity
        .as_ref()
        .map(|identity| identity.algorithm.as_str())
        .unwrap_or("");
    let public = identity
        .as_ref()
        .map(|identity| identity.public_hex.as_str())
        .unwrap_or("");
    let Ok(chips) = document.query_selector_all(".current-id") else {
        return;
    };
    for index in 0..chips.length() {
        let Some(el) = chips
            .item(index)
            .and_then(|node| node.dyn_into::<web_sys::Element>().ok())
        else {
            continue;
        };
        if let Some(name_el) = el.query_selector(".current-id-name").ok().flatten() {
            name_el.set_text_content(Some(&name));
        }
        if let Some(icon) = el.query_selector("[data-current-id-icon]").ok().flatten() {
            fill_identicon(&icon, algorithm, public);
        }
        if flash {
            flash_current_id(&el);
        }
    }
}

fn flash_current_id(el: &web_sys::Element) {
    let _ = el.class_list().remove_1("flash");
    let el = el.clone();
    let closure = Closure::once(move || {
        let _ = el.class_list().add_1("flash");
    });
    if let Some(window) = web_sys::window()
        && window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                20,
            )
            .is_ok()
    {
        closure.forget();
    }
}

fn refresh_identity_select_icons(document: &Document) {
    for (select_id, icon_id) in [
        ("sign-identity", "sign-identity-icon"),
        ("verify-identity", "verify-identity-icon"),
    ] {
        let id = select_value(document, select_id).unwrap_or_else(current_id);
        if let Some(identity) = with_roster(|roster| roster.get(&id).cloned()) {
            set_identicon(document, icon_id, &identity.algorithm, &identity.public_hex);
        } else {
            set_identicon(document, icon_id, "", "");
        }
    }
}

fn toggle_verify_only_form(document: &Document) {
    let verify_only = checkbox_checked(document, "identity-verify-only");
    set_hidden(document, "identity-new-public-wrap", !verify_only);
    set_hidden(document, "btn-identity-add", !verify_only);
    refresh_add_identicon(document);
}

fn refresh_add_identicon(document: &Document) {
    let algorithm =
        select_value(document, "identity-new-algorithm").unwrap_or_else(|| ED25519_NAME.into());
    let public = input_value(document, "identity-new-public");
    if checkbox_checked(document, "identity-verify-only") {
        set_identicon(document, "identity-new-public-icon", &algorithm, &public);
    } else {
        set_identicon(document, "identity-new-public-icon", "", "");
    }
}

fn identity_syncing() -> bool {
    IDENTITY_SYNCING.with(Cell::get)
}

fn with_identity_sync(f: impl FnOnce()) {
    IDENTITY_SYNCING.with(|cell| cell.set(true));
    f();
    IDENTITY_SYNCING.with(|cell| cell.set(false));
}

fn bind_validate(document: &Document) {
    bind_input_event(document, "validate-yaml", "input", schedule_validate);
    bind_click(document, "btn-validate", |document| {
        run_validate(document, true)
    });
}

fn schedule_validate(document: &Document) {
    set_box_state(document, "validate-yaml-box", "pending");
    let document = document.clone();
    debounce(&VALIDATE_TIMER, LIVE_DELAY_MS, move || {
        run_validate(&document, false);
    });
}

fn run_validate(document: &Document, flash: bool) {
    cancel_timer(&VALIDATE_TIMER);
    let yaml = textarea_value(document, "validate-yaml");
    let result = ops::validate_yaml(&yaml);
    let ok = result.status == "success";
    set_box_state(document, "validate-yaml-box", if ok { "ok" } else { "err" });
    show_result(document, "validate-status", &result);
    if flash {
        play_flash(document, "validate-flash", ok);
    }
}

fn copy_validate_to_sign(document: &Document) {
    load_sign_payload(document, &textarea_value(document, "validate-yaml"));
}

fn load_sign_payload(document: &Document, incoming: &str) {
    let current = textarea_value(document, "sign-payload");
    set_textarea(document, "sign-payload", incoming);
    if incoming != current {
        clear_sign_artifact(document);
    }
}

fn clear_sign_artifact(document: &Document) {
    set_textarea(document, "sign-artifact", "");
    set_status(document, "sign-status", "", "idle");
}

fn bind_sign(document: &Document) {
    bind_click(document, "btn-sign", |document| run_sign(document, true));
    bind_input_event(document, "sign-payload", "input", clear_sign_artifact);
    bind_input_event(document, "sign-form", "change", |document| {
        run_sign(document, false);
    });
}

fn run_sign(document: &Document, flash: bool) {
    let payload = textarea_value(document, "sign-payload");
    let id = select_value(document, "sign-identity").unwrap_or_else(|| DEFAULT_IDENTITY.into());
    let Some(identity) = with_roster(|roster| roster.get(&id).cloned()) else {
        set_status(
            document,
            "sign-status",
            "err",
            &format!("unknown identity '{id}'"),
        );
        if flash {
            play_flash(document, "sign-flash", false);
        }
        return;
    };
    let keyid = input_value(document, "sign-keyid");
    let form = select_value(document, "sign-form").unwrap_or_else(|| "yaml".into());
    if identity.verify_only {
        set_textarea(document, "sign-artifact", "");
        show_result(
            document,
            "sign-status",
            &OpResult::invocation_error("verify_only_identity"),
        );
        if flash {
            play_flash(document, "sign-flash", false);
        }
        return;
    }
    let result = ops::sign(
        &payload,
        &identity.algorithm,
        &identity.private_hex,
        if keyid.is_empty() {
            None
        } else {
            Some(keyid.as_str())
        },
        true,
        &form,
    );
    set_textarea(document, "sign-artifact", &result.primary);
    if result.status == "success" {
        copy_artifact_to_verify_and_decompose(document, &result.primary, &form);
    }
    show_result(document, "sign-status", &result);
    if flash {
        play_flash(document, "sign-flash", result.status == "success");
    }
}

fn bind_send(document: &Document) {
    bind_click(document, "btn-send-sign", send_from_validate);
    bind_click(document, "btn-send-verify", |document| {
        send_from_sign(document, "verify")
    });
    bind_click(document, "btn-send-decompose", |document| {
        send_from_sign(document, "decompose")
    });
    bind_click(document, "btn-send-compose", send_from_decompose);
    bind_click(document, "btn-send-compose-verify", |document| {
        send_from_compose(document, "verify")
    });
    bind_click(document, "btn-send-compose-decompose", |document| {
        send_from_compose(document, "decompose")
    });
    bind_click(
        document,
        "btn-send-verify-decompose",
        send_verify_artifact_to_decompose,
    );
    bind_click(
        document,
        "btn-send-verify-sign",
        send_verify_payload_to_sign,
    );
}

fn send_from_validate(document: &Document) {
    copy_validate_to_sign(document);
    set_hash("sign");
}

fn send_from_sign(document: &Document, dest: &str) {
    let artifact = textarea_value(document, "sign-artifact");
    if artifact.trim().is_empty() {
        set_status(
            document,
            "sign-status",
            "warn",
            "Sign first, or paste an artifact into the signed artifact box.",
        );
        return;
    }
    let form = select_value(document, "sign-form").unwrap_or_else(|| "yaml".into());
    copy_artifact_to_verify_and_decompose(document, &artifact, &form);
    set_hash(dest);
}

fn send_from_decompose(document: &Document) {
    let payload = textarea_value(document, "decompose-payload");
    let carrier = textarea_value(document, "decompose-carrier");
    if payload.trim().is_empty() && carrier.trim().is_empty() {
        set_status(
            document,
            "decompose-status",
            "warn",
            "Decompose first, or paste payload and carrier into the result boxes.",
        );
        return;
    }
    let form = select_value(document, "decompose-form").unwrap_or_else(|| "yaml".into());
    copy_parts_to_compose(document, &payload, &carrier, &form);
    set_hash("compose");
}

fn send_from_compose(document: &Document, dest: &str) {
    let artifact = textarea_value(document, "compose-artifact");
    if artifact.trim().is_empty() {
        set_status(
            document,
            "compose-status",
            "warn",
            "Compose first, or paste an artifact into the artifact box.",
        );
        return;
    }
    let form = select_value(document, "compose-form").unwrap_or_else(|| "yaml".into());
    copy_artifact_to_verify_and_decompose(document, &artifact, &form);
    set_hash(dest);
}

fn send_verify_artifact_to_decompose(document: &Document) {
    let artifact = textarea_value(document, "verify-artifact");
    if artifact.trim().is_empty() {
        set_status(
            document,
            "verify-status",
            "warn",
            "Paste an artifact, or send one from Sign.",
        );
        return;
    }
    let form = select_value(document, "verify-form").unwrap_or_else(|| "yaml".into());
    set_textarea(document, "decompose-artifact", &artifact);
    set_decompose_form(document, &form);
    set_hash("decompose");
}

fn send_verify_payload_to_sign(document: &Document) {
    let payload = textarea_value(document, "verify-payload");
    if payload.trim().is_empty() {
        set_status(
            document,
            "verify-status",
            "warn",
            "Verify first, or paste an authenticated payload.",
        );
        return;
    }
    set_hash("sign");
    load_sign_payload(document, &payload);
}

fn bind_verify(document: &Document) {
    bind_click(document, "btn-verify", |document| {
        run_verify(document, true)
    });
    bind_click(document, "btn-verify-convert", convert_verify_artifact);
    bind_input_event(document, "verify-artifact", "input", schedule_verify);
    for id in ["verify-form", "verify-identity"] {
        bind_input_event(document, id, "change", schedule_verify);
    }
}

fn schedule_verify(document: &Document) {
    refresh_verify_convert(document);
    set_box_state(document, "verify-payload-box", "pending");
    let document = document.clone();
    debounce(&VERIFY_TIMER, LIVE_DELAY_MS, move || {
        run_verify(&document, false)
    });
}

fn refresh_verify_convert(document: &Document) {
    let artifact = textarea_value(document, "verify-artifact");
    let form = select_value(document, "verify-form").unwrap_or_else(|| "yaml".into());
    let other = other_form(&form);
    let show = !artifact.trim().is_empty()
        && !signed_envelope_in_form(&artifact, &form)
        && signed_envelope_in_form(&artifact, other);
    set_hidden(document, "btn-verify-convert", !show);
}

fn convert_verify_artifact(document: &Document) {
    let artifact = textarea_value(document, "verify-artifact");
    let form = select_value(document, "verify-form").unwrap_or_else(|| "yaml".into());
    let transcoded = ops::transcode(&artifact, other_form(&form), &form);
    if transcoded.status != "success" {
        show_result(document, "verify-status", &transcoded);
        refresh_verify_convert(document);
        return;
    }
    set_textarea(document, "verify-artifact", &transcoded.primary);
    run_verify(document, false);
}

fn run_verify(document: &Document, flash: bool) {
    cancel_timer(&VERIFY_TIMER);
    refresh_verify_convert(document);
    let artifact = textarea_value(document, "verify-artifact");
    if artifact.trim().is_empty() {
        set_textarea(document, "verify-payload", "");
        if flash {
            set_box_state(document, "verify-payload-box", "err");
            set_status(document, "verify-status", "warn", "document is empty");
            play_flash(document, "verify-flash", false);
        } else {
            set_box_state(document, "verify-payload-box", "");
            set_status(document, "verify-status", "", "idle");
        }
        return;
    }
    let form = select_value(document, "verify-form").unwrap_or_else(|| "yaml".into());
    let id = select_value(document, "verify-identity").unwrap_or_else(|| DEFAULT_IDENTITY.into());
    let Some(identity) = with_roster(|roster| roster.get(&id).cloned()) else {
        set_textarea(document, "verify-payload", "");
        set_box_state(document, "verify-payload-box", "err");
        set_status(
            document,
            "verify-status",
            "err",
            &format!("unknown identity '{id}'"),
        );
        return;
    };
    let result = ops::verify(&artifact, &form, &identity.algorithm, &identity.public_hex);
    set_textarea(document, "verify-payload", &result.primary);
    let ok = result.status == "verified";
    set_box_state(
        document,
        "verify-payload-box",
        if ok { "ok" } else { "err" },
    );
    show_result(document, "verify-status", &result);
    if !ok
        && matches!(
            result.status.as_str(),
            "signed_but_failed_verification" | "signed_but_algorithm_unsupported"
        )
        && let Some(name) = matching_other_identity(&artifact, &form, &id)
        && let Some(el) = document.get_element_by_id("verify-status")
    {
        let current = el.text_content().unwrap_or_default();
        el.set_text_content(Some(&format!(
            "{current} (but this does match {name}'s key)"
        )));
    }
    if flash {
        play_flash(document, "verify-flash", ok);
    }
}

fn matching_other_identity(artifact: &str, form: &str, skip_id: &str) -> Option<String> {
    let identities = with_roster(|roster| roster.list().to_vec());
    for identity in identities {
        if identity.id == skip_id {
            continue;
        }
        let result = ops::verify(artifact, form, &identity.algorithm, &identity.public_hex);
        if result.status == "verified" {
            return Some(identity.label);
        }
    }
    None
}

fn bind_compose(document: &Document) {
    bind_click(document, "btn-compose", |document| {
        run_compose(document, true)
    });
    bind_input_event(
        document,
        "compose-carrier-alg",
        "change",
        schedule_compose_carrier,
    );
    for id in ["compose-carrier-keyid", "compose-carrier-signature"] {
        bind_input_event(document, id, "input", schedule_compose_carrier);
    }
}

fn schedule_compose_carrier(document: &Document) {
    if proto_syncing() {
        return;
    }
    let form = select_value(document, "compose-form").unwrap_or_else(|| "yaml".into());
    if form != "protobuf" {
        set_box_state(document, "compose-carrier-box", "");
        return;
    }
    set_box_state(document, "compose-carrier-box", "pending");
    let document = document.clone();
    debounce(&COMPOSE_CARRIER_TIMER, LIVE_DELAY_MS, move || {
        let _ = sync_compose_carrier_wire(&document);
    });
}

fn run_compose(document: &Document, flash: bool) {
    cancel_timer(&COMPOSE_CARRIER_TIMER);
    let form = select_value(document, "compose-form").unwrap_or_else(|| "yaml".into());
    if form == "protobuf" && !sync_compose_carrier_wire(document) {
        set_status(
            document,
            "compose-status",
            "err",
            "invocation_error / invalid protobuf",
        );
        set_box_state(document, "compose-artifact-box", "err");
        fill_artifact_view(document, "compose-artifact", None);
        if flash {
            play_flash(document, "compose-carrier-flash", false);
            play_flash(document, "compose-artifact-flash", false);
        }
        return;
    }
    let payload = textarea_value(document, "compose-payload");
    let carrier = textarea_value(document, "compose-carrier");
    let result = ops::compose(&payload, &carrier, &form);
    set_textarea(document, "compose-artifact", &result.primary);
    refresh_compose_artifact_view(document, &form, &result.primary);
    show_result(document, "compose-status", &result);
    if flash {
        let ok = result.status == "success";
        play_flash(document, "compose-artifact-flash", ok);
        if form == "protobuf" {
            play_flash(document, "compose-carrier-flash", true);
        }
    }
}

fn sync_compose_carrier_wire(document: &Document) -> bool {
    if proto_syncing() {
        return true;
    }
    let alg = select_value(document, "compose-carrier-alg")
        .unwrap_or_else(|| "ALGORITHM_ED25519_PUREEDDSA_RAW_RS64_CANONICAL".into());
    let keyid = input_value(document, "compose-carrier-keyid");
    let signature = textarea_value(document, "compose-carrier-signature");
    match proto::encode_carrier_text(&alg, &keyid, &signature) {
        Ok(wire) => {
            set_textarea(document, "compose-carrier", &wire);
            set_box_state(document, "compose-carrier-box", "ok");
            true
        }
        Err(_) => {
            set_box_state(document, "compose-carrier-box", "err");
            false
        }
    }
}

fn update_compose_proto_mode(document: &Document) {
    let proto =
        select_value(document, "compose-form").unwrap_or_else(|| "yaml".into()) == "protobuf";
    set_proto_mode(document, "compose-body", proto);
    if proto {
        apply_compose_carrier_from_wire(document);
        refresh_compose_artifact_view(
            document,
            "protobuf",
            &textarea_value(document, "compose-artifact"),
        );
    } else {
        set_box_state(document, "compose-carrier-box", "");
        set_box_state(document, "compose-artifact-box", "");
    }
}

fn apply_compose_carrier_from_wire(document: &Document) {
    let wire = textarea_value(document, "compose-carrier");
    if wire.trim().is_empty() {
        fill_signature_fields(document, "compose-carrier", None, true);
        set_box_state(document, "compose-carrier-box", "");
        return;
    }
    match proto::parse_carrier_text(&wire) {
        Ok(fields) => {
            fill_signature_fields(document, "compose-carrier", Some(&fields), true);
            set_box_state(document, "compose-carrier-box", "ok");
        }
        Err(_) => set_box_state(document, "compose-carrier-box", "err"),
    }
}

fn refresh_compose_artifact_view(document: &Document, form: &str, artifact: &str) {
    if form != "protobuf" {
        set_box_state(document, "compose-artifact-box", "");
        return;
    }
    if artifact.trim().is_empty() {
        fill_artifact_view(document, "compose-artifact", None);
        set_box_state(document, "compose-artifact-box", "");
        return;
    }
    match proto::parse_artifact_text(artifact) {
        Ok(fields) => {
            fill_artifact_view(document, "compose-artifact", Some(&fields));
            set_box_state(document, "compose-artifact-box", "ok");
        }
        Err(_) => {
            fill_artifact_view(document, "compose-artifact", None);
            set_box_state(document, "compose-artifact-box", "err");
        }
    }
}

fn bind_decompose(document: &Document) {
    bind_click(document, "btn-decompose", |document| {
        run_decompose(document, true)
    });
    bind_input_event(document, "decompose-artifact", "input", schedule_decompose);
    bind_input_event(document, "decompose-outer", "change", schedule_decompose);
}

fn schedule_decompose(document: &Document) {
    set_box_state(document, "decompose-payload-box", "pending");
    set_box_state(document, "decompose-carrier-box", "pending");
    if select_value(document, "decompose-form").as_deref() == Some("protobuf") {
        set_box_state(document, "decompose-artifact-box", "pending");
    }
    let document = document.clone();
    debounce(&DECOMPOSE_TIMER, LIVE_DELAY_MS, move || {
        run_decompose(&document, false);
    });
}

fn run_decompose(document: &Document, flash: bool) {
    cancel_timer(&DECOMPOSE_TIMER);
    let artifact = textarea_value(document, "decompose-artifact");
    let form = select_value(document, "decompose-form").unwrap_or_else(|| "yaml".into());
    if artifact.trim().is_empty() {
        set_textarea(document, "decompose-payload", "");
        set_textarea(document, "decompose-carrier", "");
        set_box_state(document, "decompose-payload-box", "");
        set_box_state(document, "decompose-carrier-box", "");
        set_box_state(document, "decompose-artifact-box", "");
        refresh_decompose_proto(document, &form, "", "");
        set_status(document, "decompose-status", "", "idle");
        return;
    }
    let outer = select_value(document, "decompose-outer");
    let outer_ref = outer.as_deref().filter(|s| *s != "omit");
    let result = ops::decompose(&artifact, &form, outer_ref);
    set_textarea(document, "decompose-payload", &result.primary);
    set_textarea(document, "decompose-carrier", &result.extra);
    if result.status == "ok" {
        copy_parts_to_compose(document, &result.primary, &result.extra, &form);
    }
    let ok = result.status == "ok";
    set_box_state(
        document,
        "decompose-payload-box",
        if ok { "ok" } else { "err" },
    );
    set_box_state(
        document,
        "decompose-carrier-box",
        if ok { "ok" } else { "err" },
    );
    refresh_decompose_proto(document, &form, &artifact, &result.extra);
    show_result(document, "decompose-status", &result);
    if flash {
        play_flash(document, "decompose-flash", ok);
    }
}

fn refresh_decompose_proto(document: &Document, form: &str, artifact: &str, carrier: &str) {
    let proto = form == "protobuf";
    set_proto_mode(document, "decompose-body", proto);
    if !proto {
        set_box_state(document, "decompose-artifact-box", "");
        return;
    }
    if artifact.trim().is_empty() {
        fill_signature_fields(document, "decompose-carrier", None, false);
        return;
    }
    match proto::parse_artifact_text(artifact) {
        Ok(_) => set_box_state(document, "decompose-artifact-box", "ok"),
        Err(_) => set_box_state(document, "decompose-artifact-box", "err"),
    }
    if carrier.trim().is_empty() {
        fill_signature_fields(document, "decompose-carrier", None, false);
        return;
    }
    match proto::parse_carrier_text(carrier) {
        Ok(fields) => {
            fill_signature_fields(document, "decompose-carrier", Some(&fields), false);
            set_box_state(document, "decompose-carrier-box", "ok");
        }
        Err(_) => {
            fill_signature_fields(document, "decompose-carrier", None, false);
            set_box_state(document, "decompose-carrier-box", "err");
        }
    }
}

fn bind_form_toggles(document: &Document) {
    remember_decompose_form(
        &select_value(document, "decompose-form").unwrap_or_else(|| "yaml".into()),
    );
    remember_compose_form(&select_value(document, "compose-form").unwrap_or_else(|| "yaml".into()));
    if let Some(select) = select(document, "decompose-form") {
        let document = document.clone();
        let closure = Closure::<dyn FnMut()>::new(move || on_decompose_form_change(&document));
        let _ = select.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    if let Some(select) = select(document, "compose-form") {
        let document = document.clone();
        let closure = Closure::<dyn FnMut()>::new(move || on_compose_form_change(&document));
        let _ = select.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    update_outer_enabled(document);
    update_compose_proto_mode(document);
}

fn last_decompose_form() -> String {
    LAST_DECOMPOSE_FORM.with(|cell| cell.borrow().clone())
}

fn last_compose_form() -> String {
    LAST_COMPOSE_FORM.with(|cell| cell.borrow().clone())
}

fn remember_decompose_form(form: &str) {
    LAST_DECOMPOSE_FORM.with(|cell| *cell.borrow_mut() = form.to_string());
}

fn remember_compose_form(form: &str) {
    LAST_COMPOSE_FORM.with(|cell| *cell.borrow_mut() = form.to_string());
}

fn set_decompose_form(document: &Document, form: &str) {
    set_select(document, "decompose-form", form);
    remember_decompose_form(form);
    update_outer_enabled(document);
}

fn set_compose_form(document: &Document, form: &str) {
    set_select(document, "compose-form", form);
    remember_compose_form(form);
    update_compose_proto_mode(document);
}

fn protobuf_outer_arg(document: &Document) -> Option<String> {
    select_value(document, "decompose-outer").filter(|value| value != "omit")
}

fn on_decompose_form_change(document: &Document) {
    let next = select_value(document, "decompose-form").unwrap_or_else(|| "yaml".into());
    let prev = last_decompose_form();
    update_outer_enabled(document);
    if next == prev {
        schedule_decompose(document);
        return;
    }
    let artifact = textarea_value(document, "decompose-artifact");
    if artifact.trim().is_empty() {
        remember_decompose_form(&next);
        schedule_decompose(document);
        return;
    }
    let outer = protobuf_outer_arg(document);
    let direct = ops::decompose(&artifact, &next, outer.as_deref());
    if direct.status == "ok" {
        remember_decompose_form(&next);
        run_decompose(document, false);
        return;
    }
    let transcoded = ops::transcode(&artifact, &prev, &next);
    if transcoded.status == "success" {
        set_textarea(document, "decompose-artifact", &transcoded.primary);
        remember_decompose_form(&next);
        run_decompose(document, false);
        return;
    }
    remember_decompose_form(&next);
    run_decompose(document, false);
}

fn on_compose_form_change(document: &Document) {
    let next = select_value(document, "compose-form").unwrap_or_else(|| "yaml".into());
    let prev = last_compose_form();
    if next == prev {
        update_compose_proto_mode(document);
        return;
    }
    if prev == "protobuf" {
        let _ = sync_compose_carrier_wire(document);
    }
    let payload = textarea_value(document, "compose-payload");
    let carrier = textarea_value(document, "compose-carrier");
    let artifact = textarea_value(document, "compose-artifact");
    if payload.trim().is_empty() && carrier.trim().is_empty() && artifact.trim().is_empty() {
        remember_compose_form(&next);
        update_compose_proto_mode(document);
        return;
    }

    // YAML compose concatenates payload + `---` + carrier, so a protobuf
    // carrier "succeeds" as a YAML scalar. Transcode the envelope instead.
    if !artifact.trim().is_empty() {
        let transcoded = ops::transcode(&artifact, &prev, &next);
        if transcoded.status == "success" {
            apply_compose_transcode(document, &next, &transcoded);
            return;
        }
    }
    if !payload.trim().is_empty() || !carrier.trim().is_empty() {
        let composed = ops::compose(&payload, &carrier, &prev);
        if composed.status == "success" {
            let transcoded = ops::transcode(&composed.primary, &prev, &next);
            if transcoded.status == "success" {
                apply_compose_transcode(document, &next, &transcoded);
                return;
            }
        }
        let direct = ops::compose(&payload, &carrier, &next);
        if direct.status == "success" && signed_envelope_in_form(&direct.primary, &next) {
            set_textarea(document, "compose-artifact", &direct.primary);
            remember_compose_form(&next);
            update_compose_proto_mode(document);
            refresh_compose_artifact_view(document, &next, &direct.primary);
            show_result(document, "compose-status", &direct);
            return;
        }
    }

    set_select(document, "compose-form", &prev);
    update_compose_proto_mode(document);
    set_status(
        document,
        "compose-status",
        "err",
        "transcode_error / could_not_switch_form",
    );
}

fn apply_compose_transcode(document: &Document, next: &str, transcoded: &OpResult) {
    set_textarea(document, "compose-artifact", &transcoded.primary);
    let outer = (next == "protobuf").then_some("signature_strict");
    let parts = ops::decompose(&transcoded.primary, next, outer);
    if parts.status == "ok" {
        set_textarea(document, "compose-payload", &parts.primary);
        set_textarea(document, "compose-carrier", &parts.extra);
    }
    remember_compose_form(next);
    update_compose_proto_mode(document);
    refresh_compose_artifact_view(document, next, &transcoded.primary);
    show_result(document, "compose-status", transcoded);
}

fn signed_envelope_in_form(artifact: &str, form: &str) -> bool {
    ops::transcode(artifact, form, other_form(form)).status == "success"
}

fn other_form(form: &str) -> &'static str {
    if form == "protobuf" {
        "yaml"
    } else {
        "protobuf"
    }
}

fn update_outer_enabled(document: &Document) {
    let form = select_value(document, "decompose-form").unwrap_or_else(|| "yaml".into());
    if let Some(outer) = select(document, "decompose-outer") {
        let proto = form == "protobuf";
        outer.set_disabled(!proto);
        if proto && outer.value() == "omit" {
            outer.set_value("strict");
        }
        if !proto {
            outer.set_value("omit");
        }
    }
}

fn show_result(document: &Document, status_id: &str, result: &OpResult) {
    let level = match result.status.as_str() {
        "success" | "ok" | "verified" => "ok",
        "unsigned" | "signed_but_algorithm_unsupported" => "warn",
        _ => "err",
    };
    let mut msg = result.status.clone();
    if let Some(code) = &result.code {
        msg.push_str(" / ");
        msg.push_str(code);
    }
    if !result.extra_label.is_empty()
        && !result.extra.is_empty()
        && result.extra_label == "algorithm"
    {
        msg.push_str(" · ");
        msg.push_str(&result.extra);
    }
    set_status(document, status_id, level, &msg);
}

fn set_status(document: &Document, id: &str, level: &str, message: &str) {
    if let Some(el) = document.get_element_by_id(id) {
        el.set_class_name(&if level.is_empty() {
            "status".to_string()
        } else {
            format!("status {level}")
        });
        el.set_text_content(Some(message));
    }
}

fn is_open(document: &Document, name: &str) -> bool {
    document
        .query_selector(&format!("[data-window=\"{name}\"]"))
        .ok()
        .flatten()
        .is_some_and(|el| !el.has_attribute("hidden"))
}

fn bind_input_event(
    document: &Document,
    id: &str,
    event: &str,
    on_event: impl Fn(&Document) + 'static,
) {
    let Some(el) = document.get_element_by_id(id) else {
        return;
    };
    let document = document.clone();
    let closure = Closure::<dyn FnMut()>::new(move || on_event(&document));
    let _ = el.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref());
    closure.forget();
}

fn bind_click(document: &Document, id: &str, on_click: impl Fn(&Document) + 'static) {
    bind_input_event(document, id, "click", on_click);
}

fn copy_artifact_to_verify_and_decompose(document: &Document, artifact: &str, form: &str) {
    set_textarea(document, "verify-artifact", artifact);
    set_select(document, "verify-form", form);
    set_textarea(document, "decompose-artifact", artifact);
    set_decompose_form(document, form);
}

fn copy_parts_to_compose(document: &Document, payload: &str, carrier: &str, form: &str) {
    set_textarea(document, "compose-payload", payload);
    set_textarea(document, "compose-carrier", carrier);
    set_compose_form(document, form);
}

fn set_box_state(document: &Document, id: &str, state: &str) {
    let Some(el) = document.get_element_by_id(id) else {
        return;
    };
    let _ = el.class_list().remove_3("pending", "ok", "err");
    if !state.is_empty() {
        let _ = el.class_list().add_1(state);
    }
}

fn play_flash(document: &Document, id: &str, ok: bool) {
    let Some(el) = document.get_element_by_id(id) else {
        return;
    };
    let _ = el.class_list().remove_3("play", "ok", "err");
    let kind = if ok { "ok" } else { "err" };
    let el = el.clone();
    let closure = Closure::once(move || {
        let _ = el.class_list().add_2("play", kind);
    });
    if let Some(window) = web_sys::window()
        && window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                20,
            )
            .is_ok()
    {
        closure.forget();
    }
}

fn cancel_timer(slot: &'static std::thread::LocalKey<Cell<i32>>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    slot.with(|cell| {
        let prev = cell.get();
        if prev >= 0 {
            window.clear_timeout_with_handle(prev);
            cell.set(-1);
        }
    });
}

fn debounce(
    slot: &'static std::thread::LocalKey<Cell<i32>>,
    delay_ms: i32,
    f: impl FnOnce() + 'static,
) {
    let Some(window) = web_sys::window() else {
        f();
        return;
    };
    cancel_timer(slot);
    let closure = Closure::once(move || {
        slot.with(|cell| cell.set(-1));
        f();
    });
    match window.set_timeout_with_callback_and_timeout_and_arguments_0(
        closure.as_ref().unchecked_ref(),
        delay_ms,
    ) {
        Ok(id) => slot.with(|cell| cell.set(id)),
        Err(_) => slot.with(|cell| cell.set(-1)),
    }
    closure.forget();
}

fn proto_syncing() -> bool {
    PROTO_SYNCING.with(Cell::get)
}

fn with_proto_sync(f: impl FnOnce()) {
    PROTO_SYNCING.with(|cell| cell.set(true));
    f();
    PROTO_SYNCING.with(|cell| cell.set(false));
}

fn set_proto_mode(document: &Document, body_id: &str, proto: bool) {
    let Some(el) = document.get_element_by_id(body_id) else {
        return;
    };
    let _ = el.class_list().remove_1("proto-mode");
    if proto {
        let _ = el.class_list().add_1("proto-mode");
    }
}

fn fill_artifact_view(document: &Document, prefix: &str, fields: Option<&ArtifactFields>) {
    with_proto_sync(|| {
        let payload = fields.map(|f| f.payload.as_str()).unwrap_or("");
        let alg = fields.map(|f| f.signature.alg.as_str()).unwrap_or("");
        let keyid = fields.map(|f| f.signature.keyid.as_str()).unwrap_or("");
        let signature = fields
            .map(|f| f.signature.signature_b64.as_str())
            .unwrap_or("");
        set_textarea(document, &format!("{prefix}-payload"), payload);
        set_input(document, &format!("{prefix}-alg"), alg);
        set_input(document, &format!("{prefix}-keyid"), keyid);
        set_textarea(document, &format!("{prefix}-signature"), signature);
    });
}

fn fill_signature_fields(
    document: &Document,
    prefix: &str,
    fields: Option<&SignatureFields>,
    alg_is_select: bool,
) {
    with_proto_sync(|| {
        let alg = fields.map(|f| f.alg.as_str()).unwrap_or(if alg_is_select {
            "ALGORITHM_ED25519_PUREEDDSA_RAW_RS64_CANONICAL"
        } else {
            ""
        });
        let keyid = fields.map(|f| f.keyid.as_str()).unwrap_or("");
        let signature = fields.map(|f| f.signature_b64.as_str()).unwrap_or("");
        if alg_is_select {
            set_select(document, &format!("{prefix}-alg"), alg);
        } else {
            set_input(document, &format!("{prefix}-alg"), alg);
        }
        set_input(document, &format!("{prefix}-keyid"), keyid);
        set_textarea(document, &format!("{prefix}-signature"), signature);
    });
}

fn textarea(document: &Document, id: &str) -> Option<HtmlTextAreaElement> {
    document
        .get_element_by_id(id)?
        .dyn_into::<HtmlTextAreaElement>()
        .ok()
}

fn textarea_value(document: &Document, id: &str) -> String {
    textarea(document, id)
        .map(|el| el.value())
        .unwrap_or_default()
}

fn set_textarea(document: &Document, id: &str, value: &str) {
    if let Some(el) = textarea(document, id) {
        el.set_value(value);
    }
}

fn input_value(document: &Document, id: &str) -> String {
    document
        .get_element_by_id(id)
        .and_then(|el| el.dyn_into::<HtmlInputElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}

fn set_input(document: &Document, id: &str, value: &str) {
    if let Some(el) = document
        .get_element_by_id(id)
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        el.set_value(value);
    }
}

fn set_readonly(document: &Document, id: &str, readonly: bool) {
    if let Some(el) = document
        .get_element_by_id(id)
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        el.set_read_only(readonly);
    }
}

fn checkbox_checked(document: &Document, id: &str) -> bool {
    document
        .get_element_by_id(id)
        .and_then(|el| el.dyn_into::<HtmlInputElement>().ok())
        .is_some_and(|el| el.checked())
}

fn set_checkbox(document: &Document, id: &str, checked: bool) {
    if let Some(el) = document
        .get_element_by_id(id)
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        el.set_checked(checked);
    }
}

fn set_identicon(document: &Document, id: &str, algorithm: &str, public_key: &str) {
    if let Some(el) = document.get_element_by_id(id) {
        fill_identicon(&el, algorithm, public_key);
    }
}

fn fill_identicon(el: &web_sys::Element, algorithm: &str, public_key: &str) {
    match identicon::svg_for_key(algorithm, public_key) {
        Some(svg) => el.set_inner_html(&svg),
        None => el.set_inner_html(""),
    }
}

fn append_identicon(
    document: &Document,
    parent: &web_sys::Element,
    algorithm: &str,
    public_key: &str,
) {
    let Ok(icon) = document.create_element("span") else {
        return;
    };
    icon.set_class_name("identicon");
    fill_identicon(&icon, algorithm, public_key);
    let _ = parent.append_child(&icon);
}

fn set_hidden(document: &Document, id: &str, hidden: bool) {
    let Some(el) = document.get_element_by_id(id) else {
        return;
    };
    if hidden {
        let _ = el.set_attribute("hidden", "");
    } else {
        let _ = el.remove_attribute("hidden");
    }
}

fn select(document: &Document, id: &str) -> Option<HtmlSelectElement> {
    document
        .get_element_by_id(id)?
        .dyn_into::<HtmlSelectElement>()
        .ok()
}

fn select_value(document: &Document, id: &str) -> Option<String> {
    select(document, id).map(|el| el.value())
}

fn set_select(document: &Document, id: &str, value: &str) {
    if let Some(el) = select(document, id) {
        el.set_value(value);
    }
}
