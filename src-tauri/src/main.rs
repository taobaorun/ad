// ad — entry point. Real work lives in ad_lib.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ad_lib::run();
}
