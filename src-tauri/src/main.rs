// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
unsafe extern "system" {
    // Windows API: 親コンソールにアタッチ / 新しいコンソールを割り当て
    fn AttachConsole(dwProcessId: u32) -> i32;
    fn AllocConsole() -> i32;
}

#[cfg(target_os = "windows")]
const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // --no-post: 記事生成はするが Discord 投稿・DB 保存をしない（ドライラン用）
    let no_post = args.iter().any(|a| a == "--no-post");
    // --console: ログをコンソールにも出力する（親コンソールがあればアタッチ、無ければ新規作成）
    let console = args.iter().any(|a| a == "--console");
    #[cfg(target_os = "windows")]
    if console {
        unsafe {
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                AllocConsole();
            }
        }
    }
    my_quick_feed_lib::run(!no_post);
}
