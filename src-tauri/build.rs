fn main() {
    // dist の変更を cargo に検知させる（2026-08-05 修正）。
    // これが無いと「vite build したのにバイナリに古い UI が埋め込まれる」問題が起きる:
    // tauri-build は frontendDist の変更を rebuild-if-changed に含めないため、
    // Rust ソースが変わらない限り cargo build がスキップされ、古い assets が残る。
    println!("cargo:rerun-if-changed=../dist");
    tauri_build::build()
}
