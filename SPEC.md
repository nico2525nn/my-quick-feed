# MY QUICK FEED — 設計仕様書

> 複数のRSS情報源（X・Reddit・その他サイト）から定期的に情報を取得し、AIで追加リサーチ・検証・画像取得を行った上で要約記事を生成し、Discordのフォーラムスレッドに自動投稿するWindowsデスクトップアプリ。

---

## 技術スタック

| 層 | 技術 |
|---|---|
| フレームワーク | Tauri v2 |
| バックエンド言語 | Rust（安定版最新） |
| フロントエンド | React + TypeScript（設定GUI） |
| タスクスケジューラ | tokio（Rust asyncランタイム） |
| Discord Bot | serenity + poise |
| RSSパース | quick-xml + serde |
| データベース | SQLite（rusqlite） |
| AI API（Direct モード） | OpenAI互換 Chat Completions（OpenRouter / OpenAI / OpenCodeZen/Go 等） |
| AI API（Agent モード） | OMP（Oh My Pi）CLI / OpenCode CLI を子プロセス呼び出し |
| 画像取得 | Web検索経由 or OMPエージェント経由で関連画像URLを取得 |
| 追加リサーチ | TinyFish API / Web Search API / OMPエージェント |
| 設定ファイル形式 | YAML（serde + serde_yaml） |

---

## AI連携

### Agent モード（v1 初期実装） — 標準

OMP（Oh My Pi）CLI または OpenCode CLI を子プロセスとして呼び出し、エージェントに記事生成タスクを丸ごと委託する。エージェントは内部でツール（Web検索、ブラウザ操作、画像取得等）を自由に使えるため、**追加リサーチ・情報の検証・関連画像の取得**が可能。

- 呼び出し形式:
  ```
  omp task "与えられたニュース記事を元に、追加リサーチを行い、検証し、関連画像を探した上で記事を書いてください。出力はJSONで..."
  ```
  または
  ```
  opencode task "..."
  ```
- エージェントが使用可能なツール（OMPの持つツールセットに依存）:
  - `web_search` — 関連情報の追加調査
  - ブラウジング — 引用元の確認・本文スクレイピング
  - 画像検索 — 記事に関連する画像のURL取得
  - コード実行 — データ分析・変換
- 出力形式: エージェントは構造化JSON（記事タイトル、本文、画像URL、出典リスト）を返す
- OMP/OpenCode がインストールされていることが前提（別途セットアップガイドで案内）

### Direct モード（v2 以降） — 将来対応

OpenAI互換 Chat Completions API を直接呼び出す。追加リサーチや画像取得は行わず、与えられたRSSアイテムをそのまま要約する簡易モード。

- リクエスト先: `{base_url}/v1/chat/completions`
- 対応プロバイダ: OpenRouter、OpenAI、OpenCodeZen/Go、その他OpenAI互換API
- できること: RSS記事の要約・翻訳・再構成
- できないこと: 追加リサーチ、スクレイピング、画像検索、Web検索

---

## 処理フロー（定期実行パイプライン）

```
タイマー発火（interval_minごと / トピックごと）
        │
        ▼
  ① RSS/RSSHUB からフィード取得
        │
        ▼
  ② DBの seen_items と照合（重複除外）
        │
        ▼
  ③ OMP / OpenCode CLI を呼び出し、エージェントに「追加リサーチ＋検証＋画像取得＋記事生成」を依頼
        │
        ├─ ③-a エージェントが Web検索/TinyFish で追加調査・ファクトチェック
        ├─ ③-b エージェントが関連画像のURLを検索・選定
        └─ ③-c エージェントが最終記事（タイトル＋本文＋画像URL）を構造化JSONで返す
        │
        ▼
  ④ Discord フォーラムスレッドに投稿（テキスト + 画像埋め込み）
        │
        ▼
  ⑤ DB更新（seen_items・posts・画像キャッシュ情報）
        │
        ▼
  ⑥ 完了ログ出力
```

---

## プロジェクト構造

```
my-quick-feed/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs                   # Tauriエントリポイント、システムトレイ、IPC commands
│   │   ├── config.rs                 # YAML設定ファイルの読み書き（serde）
│   │   ├── db.rs                     # SQLiteスキーマ定義、CRUD操作
│   │   ├── fetcher/
│   │   │   ├── mod.rs
│   │   │   ├── rss.rs                # 汎用RSS/Atomパーサ
│   │   │   └── rsshub.rs             # RSSHUB（X/Twitter用）ラッパー
│   │   ├── ai/
│   │   │   ├── mod.rs
│   │   │   ├── direct.rs             # Direct モード: OpenAI互換 Chat Completions 呼び出し
│   │   │   └── agent.rs              # Agent モード: OMP / OpenCode CLI 子プロセス呼び出し
│   │   ├── discord.rs                # Discord Bot（フォーラム＋スレッド投稿、画像埋め込み）
│   │   ├── scheduler.rs              # トピックごとの定期実行タイマー
│   │   └── pipeline.rs               # 取得→リサーチ→生成→投稿のパイプライン制御
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                               # React フロントエンド（設定GUI）
│   ├── App.tsx
│   ├── components/
│   │   ├── Sidebar.tsx
│   │   ├── Dashboard.tsx
│   │   ├── TopicsPage.tsx
│   │   ├── SettingsPage.tsx
│   │   └── LogsPage.tsx
│   └── styles/
│       └── theme.css                  # ダークテーマ共通CSS
├── package.json
└── my-quick-feed.yaml                 # 設定ファイル（ユーザーデータディレクトリ）
```

---

## 設定ファイル形式（YAML）

```yaml
discord:
  token: "DISCORD_BOT_TOKEN"
  forum_channel_id: "1234567890123456789"

ai:
  mode: "agent"                       # "agent"(v1) | "direct"(v2)
  # --- Agent モード用 (v1) ---
  agent_command: "omp"                # "omp" | "opencode"
  agent_timeout_sec: 120
  # --- Direct モード用 (v2) ---
  # api_key: "sk-or-xxxxx"
  # model: "openai/gpt-4o-mini"
  # base_url: "https://openrouter.ai/api/v1"

topics:
  - name: "APEXまとめ"
    language: "ja"
    interval_min: 120
    sources:
      - type: "rss"
        url: "https://apexlegends-leaksnews.com/feed"
      - type: "rsshub"
        url: "https://rsshub.app/twitter/user/PlayApex"
    system_prompt: |
      あなたはAPEX Legendsのニュースをまとめるアシスタントです。
      以下の情報源から収集した情報を基に、簡潔なニュース記事を1件生成してください。
      タイトルは「【APEX】」で始め、本文は300字程度にまとめてください。
      出典を明記し、複数のソースを統合する場合はその旨も記載してください。
    image_search_enabled: true        # 関連画像の自動検索・添付
    research_enabled: true            # 追加リサーチ・ファクトチェックの有無
```

---

## データベーススキーマ（SQLite）

```sql
-- 既に取得した記事の重複防止
CREATE TABLE seen_items (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    topic_id    TEXT NOT NULL,
    source_url  TEXT NOT NULL,
    title       TEXT,
    fetched_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(topic_id, source_url)
);

-- AIが生成した投稿履歴
CREATE TABLE posts (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    topic_id           TEXT NOT NULL,
    title              TEXT NOT NULL,
    content            TEXT NOT NULL,
    image_url          TEXT,                          -- 関連画像URL
    discord_message_id TEXT,
    discord_thread_id  TEXT,
    created_at         TEXT NOT NULL DEFAULT (datetime('now'))
);

-- リアクションログ（v2で実装）
CREATE TABLE reactions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    post_id     INTEGER NOT NULL REFERENCES posts(id),
    emoji       TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
```

---

## ニュース投稿フォーマット

大きく参考にしているサイト: [https://apexlegends-leaksnews.com/](https://apexlegends-leaksnews.com/)

AIが生成する各記事は以下の形式でDiscordに投稿される：

```
【APEX】UNLIMITの劇的優勝でEWC視聴者数が前年比74%増

2026年7月14日、ALGS世界大会で日本のUNLIMITが優勝したことを受け、
EWC 2026の開幕週視聴者データが公開された。前年比74%増となり、
日本語が全体トップの視聴者言語に。今後のスプリット2プロリーグ
予選にも注目が集まっている。

📡 出典: X (@ApexTimes), Reddit (r/CompetitiveApex)
```

Agent モードでは記事先頭に関連画像がDiscord埋め込みで添付される。

| 要素 | 仕様 |
|---|---|
| タイトル | `【トピック名】` で始める |
| 本文 | 300字、1段落程度 |
| 出典表記 | 末尾に📡で情報源を明記 |
| 画像 | Agent モード時、関連画像URLをDiscord埋め込みで添付（あれば） |
| 投稿先 | トピックごとに割り当てられたフォーラムスレッド |

---

## UI設計

### 全体レイアウト

```
┌──────────────────────────────────────────────────────┐
│  ┌────────────┐  ┌─────────────────────────────────┐ │
│  │            │  │  Dashboard                [Force │ │
│  │  🏠 Dashboard│  │                          Refresh] │ │
│  │  📡 Topics  │  │                                 │ │
│  │  ⚙️ Settings│  │  ┌─ Next fetch ──┬─ Articles ──┐│ │
│  │  📋 Logs    │  │  │  🕐 47 min    │  📄 12      ││ │
│  │            │  │  └───────────────┴──────────────┘│ │
│  │  ● Running  │  │  ┌─ Topics Overview ────────────┐│ │
│  └────────────┘  │  │ ┌──────────┐┌────────┐┌──────┐│ │
│                  │  │ │APEXまとめ││AIニュー││テクノ││ │
│                  │  │ │3 sources ││ス      ││ロジー││ │
│                  │  │ │8 new     ││5 sourc ││速報  ││ │
│                  │  │ └──────────┘│6 new   ││4 sou││ │
│                  │  │             └────────┘│rces  ││ │
│                  │  │                       │5 new ││ │
│                  │  │                       └──────┘│ │
│                  │  └───────────────────────────────┘│ │
│                  └──────────────────────────────────────┘ │
└──────────────────────────────────────────────────────┘
```

### 4つの画面

| 画面 | 内容 |
|---|---|
| **Dashboard** | ステータス表示、次回取得までのカウント、トピックカード一覧 |
| **Topics** | トピックの追加・編集・削除、RSSソース、research/image有効設定 |
| **Settings** | Discord連携、AIモード切替（Direct/Agent）、APIキー・モデル、OMP/OpenCodeコマンド設定 |
| **Logs** | 取得・リサーチ・生成・投稿のリアルタイムログ |

### カラーパレット（ダークモード固定）

| トークン | 値 | 用途 |
|---|---|---|
| 背景 | `#0d1117` | ウィンドウ全体 |
| サイドバー | `#161b22` | ナビゲーション背景 |
| カード背景 | `#161b22` | トピックカード等 |
| メインテキスト | `#c9d1d9` | 見出し・ラベル |
| 補助テキスト | `#8b949e` | サブ情報 |
| アクセント（青） | `#58a6ff` | 選択状態・ボタン |
| アクセント（緑） | `#3fb950` | 正常・Running表示 |
| アクセント（黄） | `#d29922` | 別カテゴリ・警告 |
| 境界線 | `#30363d` | カード・ボタン枠線 |

---

## バックエンド詳細

### モジュール責務

| モジュール | 責務 |
|---|---|
| `main.rs` | Tauriアプリケーションの初期化、システムトレイ登録、IPCコマンドの公開 |
| `config.rs` | `my-quick-feed.yaml` の読み込み・監視（ファイル変更時に自動リロード） |
| `db.rs` | SQLiteのマイグレーション（初回テーブル作成）とCRUD操作の関数提供 |
| `fetcher/rss.rs` | RSS 2.0 / Atom 両対応パーサ。`reqwest` でHTTP取得 → `quick-xml` でパース |
| `fetcher/rsshub.rs` | RSSHUBのURLテンプレート管理。publicインスタンスと自前インスタンスの切替 |
| `ai/direct.rs` | （v2）OpenAI互換 Chat Completions APIへのPOST。ストリーミング非対応 |
| `ai/agent.rs` | OMP/OpenCode CLIを `std::process::Command` で子プロセス実行。JSON出力をパース |
| `discord.rs` | serenityクライアントの初期化、フォーラムチャンネルへの投稿（+画像埋め込み） |
| `scheduler.rs` | トピックごとの非同期タイマー管理（tokio::time::interval）。手動Refresh命令も受付 |
| `pipeline.rs` | フェッチ → リサーチ（Agent） → 重複チェック → AI生成 → 投稿 の一連の流れを Orchestrate |

### エラーハンドリング方針

- 各段階でエラーが発生しても後続は続行する（1トピックの1ソースが落ちても他は動く）
- エラーはすべて `Logs` 画面に表示される構造化ログとして記録
- Agentモードのタイムアウトは設定値（`agent_timeout_sec`）で制御、超過時はプロセスを強制終了

---

## v2 追加予定機能

- **Direct モード**：OpenAI互換APIの直接呼び出しに対応。APIキーのみで動作する軽量モード
- **リアクション分析**：Discordのリアクションを収集し、好みの傾向をAIプロンプトに反映
- **TinyFish検索＋返信回答**：ニュースへの返信にTinyFish APIで検索しAIが回答
- **自前RSSHUBホスト対応**：設定でRSSHUBのベースURLを変更可能に
- **複数AIプロバイダのネイティブ対応**：Anthropic等の独自APIにも直接対応

---

## 開発状態

| フェーズ | 状態 |
|---|---|
| 設計・仕様確定 | ✅ 完了 |
| Tauriプロジェクトセットアップ | ⬜ 未着手 |
| コア機能実装 | ⬜ 未着手 |
| 設定GUI実装 | ⬜ 未着手 |
| 動作検証 | ⬜ 未着手 |
