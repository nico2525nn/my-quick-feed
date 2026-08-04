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
    let cli = my_quick_feed_lib::CliArgs {
        // --no-post: 記事生成はするが Discord 投稿・DB 保存をしない（ドライラン用）
        post_enabled: !args.iter().any(|a| a == "--no-post"),
        // --console: ログをコンソールにも出力する
        console: args.iter().any(|a| a == "--console"),
        // --run-once: パイプラインを 1 回実行して終了（--topic <name> で対象を限定）
        run_once: args.iter().any(|a| a == "--run-once"),
        topic_filter: args
            .iter()
            .position(|a| a == "--topic")
            .and_then(|i| args.get(i + 1))
            .cloned(),
        // --config <path>: 設定ファイルパスを指定
        config_path: args
            .iter()
            .position(|a| a == "--config")
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from),
        // --verbose / --debug: ログレベルを debug に
        verbose: args.iter().any(|a| a == "--verbose" || a == "--debug"),
    };

    #[cfg(target_os = "windows")]
    if cli.console {
        unsafe {
            // 親コンソールがあればアタッチ、無ければ新しいコンソールを作る
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                AllocConsole();
            }
        }
    }
    my_quick_feed_lib::run(cli);
}
