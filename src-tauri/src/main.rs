// 正式 build 不要開主控台視窗。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    open_pos::init_tracing(None);
    open_pos::run();
}
