//! DOM wiring for the playground shell.

use std::cell::Cell;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{
    Document, HtmlButtonElement, HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement,
};

use crate::keys::generate_keypair;
use crate::ops::{self, ED25519_NAME, OpResult};
use crate::proto::{self, ArtifactFields, SignatureFields};

const SAMPLE_YAML: &str = r#"# A supply-wagon manifest
claim: ridge-line cache
season: 2026
stores:
  - pemmican
  - lamp oil
"#;

const WINDOWS: [&str; 5] = ["validate", "sign", "verify", "compose", "decompose"];
const LIVE_DELAY_MS: i32 = 90;

thread_local! {
    static VERIFY_TIMER: Cell<i32> = const { Cell::new(-1) };
    static DECOMPOSE_TIMER: Cell<i32> = const { Cell::new(-1) };
    static VALIDATE_TIMER: Cell<i32> = const { Cell::new(-1) };
    static COMPOSE_CARRIER_TIMER: Cell<i32> = const { Cell::new(-1) };
    static PROTO_SYNCING: Cell<bool> = const { Cell::new(false) };
}

pub fn boot() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };

    bind_windows(&document);
    bind_generate(&document);
    bind_validate(&document);
    bind_sign(&document);
    bind_send(&document);
    bind_verify(&document);
    bind_compose(&document);
    bind_decompose(&document);
    bind_form_toggles(&document);

    if let Some(yaml) = textarea(&document, "validate-yaml") {
        yaml.set_value(SAMPLE_YAML);
    }
    if let Some(payload) = textarea(&document, "sign-payload") {
        payload.set_value(SAMPLE_YAML);
    }

    mint_keys(&document);
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

fn bind_generate(document: &Document) {
    for id in ["btn-mint-sign", "btn-mint-verify"] {
        if let Some(button) = button(document, id) {
            let document = document.clone();
            let closure = Closure::<dyn FnMut()>::new(move || mint_keys(&document));
            let _ =
                button.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
            closure.forget();
        }
    }
    for id in ["sign-algorithm", "verify-algorithm"] {
        if let Some(algorithm) = select(document, id) {
            let document = document.clone();
            let source = id.to_string();
            let closure = Closure::<dyn FnMut()>::new(move || {
                if let Some(value) = select_value(&document, &source) {
                    set_select(&document, "sign-algorithm", &value);
                    set_select(&document, "verify-algorithm", &value);
                }
                mint_keys(&document);
            });
            let _ = algorithm
                .add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
            closure.forget();
        }
    }
}

fn mint_keys(document: &Document) {
    let algorithm = select_value(document, "sign-algorithm")
        .or_else(|| select_value(document, "verify-algorithm"))
        .unwrap_or_else(|| ED25519_NAME.into());
    match generate_keypair(&algorithm) {
        Ok(pair) => {
            apply_keypair(document, &algorithm, &pair.private_hex, &pair.public_hex);
            set_status(
                document,
                "key-status",
                "ok",
                "Minted a fresh ephemeral pair. Reload or generate again to discard it.",
            );
            if is_open(document, "verify") {
                schedule_verify(document);
            }
        }
        Err(error) => set_status(document, "key-status", "err", &error),
    }
}

fn apply_keypair(document: &Document, algorithm: &str, private_hex: &str, public_hex: &str) {
    set_select(document, "sign-algorithm", algorithm);
    set_select(document, "verify-algorithm", algorithm);
    set_input(document, "key-private", private_hex);
    set_input(document, "verify-private", private_hex);
    set_input(document, "sign-public", public_hex);
    set_input(document, "key-public", public_hex);
}

fn carry_keys(document: &Document) {
    let algorithm = select_value(document, "sign-algorithm")
        .or_else(|| select_value(document, "verify-algorithm"))
        .unwrap_or_else(|| ED25519_NAME.into());
    let private = first_filled_input(document, &["key-private", "verify-private"]);
    let public = first_filled_input(document, &["sign-public", "key-public"]);
    apply_keypair(document, &algorithm, &private, &public);
}

fn first_filled_input(document: &Document, ids: &[&str]) -> String {
    for id in ids {
        let value = input_value(document, id);
        if !value.trim().is_empty() {
            return value;
        }
    }
    String::new()
}

fn bind_validate(document: &Document) {
    bind_input_event(document, "validate-yaml", "input", schedule_validate);
    if let Some(btn) = button(document, "btn-validate") {
        let document = document.clone();
        let closure = Closure::<dyn FnMut()>::new(move || run_validate(&document, true));
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
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
    set_textarea(
        document,
        "sign-payload",
        &textarea_value(document, "validate-yaml"),
    );
}

fn bind_sign(document: &Document) {
    let Some(btn) = button(document, "btn-sign") else {
        return;
    };
    let document = document.clone();
    let closure = Closure::<dyn FnMut()>::new(move || {
        let payload = textarea_value(&document, "sign-payload");
        let algorithm =
            select_value(&document, "sign-algorithm").unwrap_or_else(|| ED25519_NAME.into());
        let key = input_value(&document, "key-private");
        let keyid = input_value(&document, "sign-keyid");
        let form = select_value(&document, "sign-form").unwrap_or_else(|| "yaml".into());
        let result = ops::sign(
            &payload,
            &algorithm,
            &key,
            if keyid.is_empty() {
                None
            } else {
                Some(keyid.as_str())
            },
            true,
            &form,
        );
        set_textarea(&document, "sign-artifact", &result.primary);
        if result.status == "success" {
            set_textarea(&document, "verify-artifact", &result.primary);
            set_select(&document, "verify-form", &form);
            set_textarea(&document, "decompose-artifact", &result.primary);
            set_select(&document, "decompose-form", &form);
        }
        show_result(&document, "sign-status", &result);
    });
    let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
    closure.forget();
}

fn bind_send(document: &Document) {
    if let Some(btn) = button(document, "btn-send-sign") {
        let document = document.clone();
        let closure = Closure::<dyn FnMut()>::new(move || send_from_validate(&document));
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    if let Some(btn) = button(document, "btn-send-verify") {
        let document = document.clone();
        let closure = Closure::<dyn FnMut()>::new(move || send_from_sign(&document, "verify"));
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    if let Some(btn) = button(document, "btn-send-decompose") {
        let document = document.clone();
        let closure = Closure::<dyn FnMut()>::new(move || send_from_sign(&document, "decompose"));
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
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
    carry_keys(document);
    set_textarea(document, "verify-artifact", &artifact);
    set_select(document, "verify-form", &form);
    set_textarea(document, "decompose-artifact", &artifact);
    set_select(document, "decompose-form", &form);
    update_outer_enabled(document);
    set_hash(dest);
}

fn bind_verify(document: &Document) {
    if let Some(btn) = button(document, "btn-verify") {
        let document = document.clone();
        let closure = Closure::<dyn FnMut()>::new(move || run_verify(&document, true));
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    for id in ["verify-artifact", "key-public"] {
        bind_input_event(document, id, "input", schedule_verify);
    }
    for id in ["verify-form", "verify-algorithm"] {
        bind_input_event(document, id, "change", schedule_verify);
    }
}

fn schedule_verify(document: &Document) {
    set_box_state(document, "verify-payload-box", "pending");
    let document = document.clone();
    debounce(&VERIFY_TIMER, LIVE_DELAY_MS, move || {
        run_verify(&document, false)
    });
}

fn run_verify(document: &Document, flash: bool) {
    cancel_timer(&VERIFY_TIMER);
    let artifact = textarea_value(document, "verify-artifact");
    if artifact.trim().is_empty() {
        set_textarea(document, "verify-payload", "");
        set_box_state(document, "verify-payload-box", "");
        set_status(document, "verify-status", "", "idle");
        return;
    }
    let form = select_value(document, "verify-form").unwrap_or_else(|| "yaml".into());
    let algorithm =
        select_value(document, "verify-algorithm").unwrap_or_else(|| ED25519_NAME.into());
    let key = input_value(document, "key-public");
    let result = ops::verify(&artifact, &form, &algorithm, &key);
    set_textarea(document, "verify-payload", &result.primary);
    let ok = result.status == "verified";
    set_box_state(
        document,
        "verify-payload-box",
        if ok { "ok" } else { "err" },
    );
    show_result(document, "verify-status", &result);
    if flash {
        play_flash(document, "verify-flash", ok);
    }
}

fn bind_compose(document: &Document) {
    if let Some(btn) = button(document, "btn-compose") {
        let document = document.clone();
        let closure = Closure::<dyn FnMut()>::new(move || run_compose(&document, true));
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
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
    if let Some(btn) = button(document, "btn-decompose") {
        let document = document.clone();
        let closure = Closure::<dyn FnMut()>::new(move || run_decompose(&document, true));
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
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
        set_textarea(document, "compose-payload", &result.primary);
        set_textarea(document, "compose-carrier", &result.extra);
        set_select(document, "compose-form", &form);
        update_compose_proto_mode(document);
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
        fill_artifact_view(document, "decompose-artifact", None);
        fill_signature_fields(document, "decompose-carrier", None, false);
        return;
    }
    match proto::parse_artifact_text(artifact) {
        Ok(fields) => {
            fill_artifact_view(document, "decompose-artifact", Some(&fields));
            set_box_state(document, "decompose-artifact-box", "ok");
        }
        Err(_) => {
            fill_artifact_view(document, "decompose-artifact", None);
            set_box_state(document, "decompose-artifact-box", "err");
        }
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
    if let Some(select) = select(document, "decompose-form") {
        let document = document.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            update_outer_enabled(&document);
            schedule_decompose(&document);
        });
        let _ = select.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    if let Some(select) = select(document, "compose-form") {
        let document = document.clone();
        let closure = Closure::<dyn FnMut()>::new(move || update_compose_proto_mode(&document));
        let _ = select.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    update_outer_enabled(document);
    update_compose_proto_mode(document);
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

fn button(document: &Document, id: &str) -> Option<HtmlButtonElement> {
    document
        .get_element_by_id(id)?
        .dyn_into::<HtmlButtonElement>()
        .ok()
}
