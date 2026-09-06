// 正式 build 不要開主控台視窗。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    open_pos::init_tracing(None);
    println!("open-pos {} — GUI 進入點（M0 骨架）", open_pos::VERSION);
}
