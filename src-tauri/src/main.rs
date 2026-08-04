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

    // 設定ファイルパス: --config があればそれ、無ければ %APPDATA% のデフォルト
    let config_path = args
        .iter()
        .position(|a| a == "--config")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("APPDATA").ok().map(|d| {
                std::path::PathBuf::from(d)
                    .join("com.myquickfeed.app")
                    .join("my-quick-feed.yaml")
            })
        });

    // 設定ファイルの `cli:` セクション（起動オプションのデフォルト）。
    // コマンドライン引数が明示されていれば引数が優先され、無ければこの値を使う。
    let cli_cfg = config_path
        .as_ref()
        .and_then(|p| my_quick_feed_lib::load_config_cli(p).ok())
        .unwrap_or_default();

    let cli = my_quick_feed_lib::CliArgs {
        // --no-post: 記事生成はするが Discord 投稿・DB 保存をしない（ドライラン用）
        // --post: YAML で no_post: true でも明示的に投稿を有効化
        post_enabled: if args.iter().any(|a| a == "--no-post") {
            false
        } else if args.iter().any(|a| a == "--post") {
            true
        } else {
            !cli_cfg.no_post
        },
        // --console: ログをコンソールにも出力する
        console: if args.iter().any(|a| a == "--console") {
            true
        } else if args.iter().any(|a| a == "--no-console") {
            false
        } else {
            cli_cfg.console
        },
        // --run-once: パイプラインを 1 回実行して終了（--topic <name> で対象を限定）
        // --scheduler: YAML で run_once: true でも通常起動（スケジューラ）に戻す
        run_once: if args.iter().any(|a| a == "--run-once") {
            true
        } else if args.iter().any(|a| a == "--scheduler") {
            false
        } else {
            cli_cfg.run_once
        },
        topic_filter: args
            .iter()
            .position(|a| a == "--topic")
            .and_then(|i| args.get(i + 1))
            .cloned()
            .or_else(|| cli_cfg.topic.filter(|t| !t.is_empty())),
        // --no-run: 起動時の即実行をスキップ（スケジューラは動く。定期実行から始める）
        no_initial_run: if args.iter().any(|a| a == "--no-run") {
            true
        } else {
            cli_cfg.no_run
        },
        // --config <path>: 設定ファイルパスを指定
        config_path: config_path,
        // --verbose / --debug: ログレベルを debug に
        verbose: if args.iter().any(|a| a == "--verbose" || a == "--debug") {
            true
        } else {
            cli_cfg.verbose
        },
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
