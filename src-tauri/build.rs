fn main() {
    // db-kit 的 build.rs 無條件呼叫 tauri_build::build()，導致 slim build 仍需要
    // tauri.conf.json 與 icons 齊全。這裡 gate 起來，讓
    // `--no-default-features --features server` 在沒有 WebKit / icons 的容器裡也能編。
    //
    // ⚠️ build script **不會**拿到 `--cfg feature="..."`，所以不能寫 cfg!(feature = "gui")。
    //    Cargo 傳給 build script 的是環境變數 CARGO_FEATURE_<NAME>。
    if std::env::var_os("CARGO_FEATURE_GUI").is_some() {
        tauri_build::build();
    }
}
