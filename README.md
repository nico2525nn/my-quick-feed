# My Quick Feed

複数の RSS 情報源から定期的に情報を取得し、AI エージェントが追加リサーチ・検証・画像取得を行って要約記事を生成、Discord フォーラムのスレッドに自動投稿する Windows デスクトップアプリ。

## 機能

- **RSS / RSSHUB 対応** — トピックごとに複数のフィードを購読し、`interval_min` ごとに自動取得
- **AI 記事生成（Agent モード）** — OMP CLI を子プロセスとして呼び出し、エージェントが記事生成タスクを実行（Web 検索・画像取得含む）
- **Discord フォーラム投稿** — トピック単位でフォーラムスレッドを自動作成・追記。Embed なしの通常 Markdown メッセージ
- **複数記事対応** — 1 回のパイプラインで複数記事を生成して投稿
- **重複防止** — 直近 2 日分の投稿タイトルをプロンプトに埋め込む方式
- **4 画面 UI（ダークモード固定）** — Dashboard / Topics / Logs / Settings
  - Dashboard: 各トピックの稼働状態・記事数・最終投稿時刻・次回実行カウントダウン
  - Logs: 構造化ログのリアルタイム表示・エクスポート
  - Settings: Discord トークン、フォーラム ID、モデル設定、スタートアップ登録
- **自動起動・多重起動防止** — tauri-plugin-autostart / single-instance

## 技術スタック

| 層 | 技術 |
|---|---|
| デスクトップ | Tauri v2（Rust） |
| フロントエンド | React + TypeScript + Vite |
| バックエンド | Rust（tokio / reqwest / rusqlite / serde_yaml） |
| データベース | SQLite |
| AI | OMP CLI（Agent モード）/ OpenAI 互換 API（Direct モード） |

## 動作環境

- Windows 10/11（64bit）
- Rust（stable-x86_64-pc-windows-gnu）+ MinGW-w64
- Node.js

## ビルド・実行

> **重要**: 日本語を含むパスでは Rust リンカ（MinGW/MSVC）が失敗するため、**ビルドは必ず ASCII パス**（例: `D:\quickfeed`）で行うこと。

```bat
:: フロントエンド
npx tsc --noEmit
npx vite build

:: バックエンド（PATH に MinGW を追加）
cd src-tauri
set PATH=C:\msys64\mingw64\bin;%PATH%
cargo build --release

:: 起動（run.bat がビルド→起動まで実行）
run.bat
```

## 設定ファイル

初回起動時に `%APPDATA%\com.myquickfeed.app\my-quick-feed.yaml` が作成されます。

```yaml
discord:
  token: "BOT_TOKEN"              # Discord Bot トークン
  forum_channel_id: "CHANNEL_ID"  # フォーラムチャンネル ID

ai:
  mode: agent                     # agent | direct
  agent_command: omp              # OMP CLI の呼び出し名
  agent_timeout_sec: 120
  model: mimo-v2.5                # 使用モデル
  provider: opencode-go           # プロバイダー
  api_key: "..."                  # Direct モード用 API キー
  base_url: "https://opencode.ai/zen/go/v1/chat/completions"

topics:
  - name: APEX                    # トピック名（フォーラムスレッド名）
    language: ja                  # 記事の言語
    interval_min: 120             # 取得間隔（分）
    sources:
      - type: rss, url: https://example.com/feed
      - type: rsshub, base_url: https://rsshub.example.com, path: /path/to/feed
```

データベースは `%APPDATA%\com.myquickfeed.app\my-quick-feed.db`（SQLite）に保存されます。

## プロジェクト構造

```
my-quick-feed/
├── src/                  # React フロントエンド
│   ├── components/       # Dashboard / Topics / Logs / Settings / Sidebar
│   └── styles/theme.css  # ダークテーマ
├── src-tauri/            # Rust バックエンド
│   └── src/
│       ├── ai/           # OMP エージェント呼び出し / Direct API
│       ├── fetcher/      # RSS / RSSHUB 取得・パース
│       ├── discord.rs    # Discord REST API（スレッド作成・投稿）
│       ├── pipeline.rs   # 記事生成パイプライン
│       ├── scheduler.rs  # 定期実行スケジューラ
│       ├── config.rs     # YAML 設定
│       └── db.rs         # SQLite
├── SPEC.md               # 設計仕様書
└── agents.md             # 開発者向け実装ノウハウ
```

## 開発フロー

- **本拠地**: `D:\学校\app\my-quick-feed`（git 管理）
- **テスト用**: `D:\quickfeed`（ビルド・実行用のコピー。日本語パス問題の回避）

```
1. 本拠地でソース編集 → git commit
2. robocopy でテスト用に同期
3. テスト用ディレクトリでビルド・検証
```

詳細は `agents.md` を参照。

## ライセンス

未定
