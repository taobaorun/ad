// cc-switch — entry point. Real work lives in cc_switch_lib.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    cc_switch_lib::run();
}
