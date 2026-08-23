mod app;
mod codec;
mod highlight;
mod identicon;
mod identities;
mod keys;
mod ops;
mod proto;
mod qr;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    app::boot();
}
