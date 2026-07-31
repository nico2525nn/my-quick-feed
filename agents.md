# AGENTS.md — 実装ハンドオフドキュメント

> このドキュメントは、SPEC.md に基づく再実装を引き継ぐエージェント向けの「実務知識」です。
> SPEC.md は理想設計、agents.md は現実の実装知識。**両方読むこと。**

---

## 1. プロジェクト概要

- **アプリ**: My Quick Feed — RSS/RSSHUB から情報を取得し、OMPエージェントで記事を生成して Discord フォーラムに自動投稿する Windows デスクトップアプリ
- **技術**: Tauri v2 + Rust + React/TypeScript (Vite)
- **開発ディレクトリ**: `C:\quickfeed`（**ASCIIパス必須**、後述）
- **元のユーザーディレクトリ**: `D:\学校\app\my-quick-feed`（日本語パス — ビルド不可のため C: にコピーして開発）

---

## 2. ビルド環境（最重要！）

### 2.1 日本語パス問題

`D:\学校\app\my-quick-feed`（日本語パス）では **Rust ビルドが失敗する**。
- MSVC リンカ (`link.exe`) が日本語パスを正しく処理できない
- MinGW の `dlltool.exe` も同様

**対策**: `C:\quickfeed` にコピーしてビルドする。ソースは両方に存在するが、**変更は必ず C:\quickfeed で行い、D: に同期する**。

### 2.2 ツールチェーン（インストール済み）

| ツール | パス | 用途 |
|---|---|---|
| Rust (GNU) | `C:\Users\nico\.cargo\bin\rustc.exe` | **stable-x86_64-pc-windows-gnu** をデフォルトに |
| MinGW-w64 | `C:\msys64\mingw64\bin\gcc.exe` | Cリンカ |
| LLVM (lld-link) | `C:\Program Files\LLVM\bin\lld-link.exe` | 代替リンカ（MSVC用、未使用） |
| Node.js | `C:\Users\nico\AppData\Roaming\npm` | フロントエンド |
| Git | `C:\Program Files\Git\cmd\git.exe` | リポジトリ管理 |
| OMP | PATH 上（`omp --help` で確認可能） | AIエージェント |

### 2.3 ビルドコマンド

```bat
:: フロントエンド
cd C:\quickfeed
npx vite build

:: バックエンド（cargo check / build）
cd C:\quickfeed\src-tauri
set PATH=C:\msys64\mingw64\bin;%PATH%
cargo build --release

:: 注意: .cargo/config.toml に RUSTFLAGS が設定済み
::   [target.x86_64-pc-windows-gnu]
::   rustflags = ["-C", "link-arg=-Wl,--exclude-all-symbols"]
```

**`--exclude-all-symbols` がないと MinGW の ld が「export ordinal too large」でリンクに失敗する**（PEフォーマットの制限）。絶対に消さないこと。

### 2.4 起動スクリプト

- `run.bat` — フロントエンド→バックエンドを毎回ビルドして起動
- `dev.bat` — `npx tauri dev`（ホットリロード）
- 両方とも `C:\quickfeed` にあり、D: 側にもコピー

---

## 3. OMP CLI の実務知識（重要！）

### 3.1 正しい呼び出し方

```
# 非インタラクティブ + ファイルからプロンプト読み込み
omp -p @C:\Users\nico\AppData\Local\Temp\mqf_prompt_1234.txt

# 非インタラクティブ + 直接引数（短いプロンプトのみ）
omp -p "プロンプト"
```

### 3.2 ハマりポイント

1. **`omp task` は存在しない**。`omp --help` で確認済み。`task` はサブエージェント起動ツールの名前。
2. **`--mode json` は使わない**。OMPの内部セッションプロトコル（JSONL）を出力するもので、記事JSONではない。
3. **stdin 経由ではプロンプトを渡せない**。`omp -p` は引数 or `@file` でのみ受け取る。
4. **ウインドウが出る** → `CREATE_NO_WINDOW` (0x08000000) フラグが必要（`std::os::windows::process::CommandExt::creation_flags`）。
5. **セッション汚染** → 作業ディレクトリを `%TEMP%\my-quick-feed\omp\` に分離すること（`current_dir()` 指定）。OMPのセッションは `{cwd}/.omp/agent/sessions/` に作られる。
6. **コマンドライン長制限** → Windows は 8191 文字まで。プロンプトが 30KB になるので **必ず `@file` 方式** を使う。

### 3.3 出力パース

OMP は `-p` モードでも JSONL（セッションプロトコル）を出力することがある:

```json
{"type":"session","version":3,...}
{"type":"turn_start"}
{"type":"message_start",...}
{"type":"message_stop","message":{"role":"assistant","content":[{"type":"text","text":"実際の応答"}]}}
```

**パース戦略（順番に試す）**:
1. 全体を `Vec<ArticleResult>` として直接パース
2. `[` 〜 `]` を抽出して配列としてパース
3. `{` 〜 `}` を抽出して単一記事としてパース
4. JSONL を1行ずつ読み、`type == "message_stop"` の `message.content[].text` を結合してから 1〜3 を試す

### 3.4 モデル指定

```
omp -p --model deepseek-v4-flash @prompt.txt   ← --model で指定可
```

YAML の `ai.model` をプロンプト内の「## 使用モデル」にも書いておくと、エージェントが従う。

---

## 4. Discord REST API（serenity 不使用）

spec には `serenity + poise` と書いてあるが、**実装は REST API 直呼び出し**（reqwest）。

### 4.1 エンドポイント

```
POST https://discord.com/api/v10/channels/{forum_channel_id}/threads   # スレッド作成
POST https://discord.com/api/v10/channels/{thread_id}/messages         # メッセージ投稿
GET  https://discord.com/api/v10/users/@me                             # トークン検証
```

ヘッダー: `Authorization: Bot {token}`、`Content-Type: application/json`、`User-Agent: MyQuickFeed/0.1`

### 4.2 スレッド作成ボディ

```json
{
  "name": "スレッド名（記事タイトル）",
  "message": {
    "content": "",
    "embeds": [{ "title": "...", "description": "...", "color": 0x58a6ff, "image": {"url": "..."} }]
  }
}
```

### 4.3 メッセージ投稿ボディ

```json
{
  "content": "Markdown本文",
  "embeds": []   // spec は Embed なしと言っているが、実装は Embed 使用中
}
```

**注意**: spec は「Embed枠なし・通常メッセージ」だが、実装は Embed を使用。再実装時はどちらかに統一すること。

### 4.4 スレッド管理

`topic_threads` テーブルでトピック→スレッドID を永続化。初回のみスレッド作成、以降は既存スレッドに追記。

---

## 5. データベース設計（実装済み）

```sql
-- 重複防止（specでは廃止予定だが実装済み）
CREATE TABLE seen_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    topic_id TEXT NOT NULL,
    source_url TEXT NOT NULL,
    title TEXT,
    fetched_at TEXT DEFAULT (datetime('now')),
    UNIQUE(topic_id, source_url)
);

CREATE TABLE posts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    topic_id TEXT NOT NULL,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    image_url TEXT,
    discord_message_id TEXT,
    discord_thread_id TEXT,
    created_at TEXT DEFAULT (datetime('now'))
);

-- トピック→フォーラムスレッドのマッピング
CREATE TABLE topic_threads (
    topic_id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL,
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE reactions (...);  -- v2
```

DB パス: `C:\Users\nico\AppData\Roaming\com.myquickfeed.app\my-quick-feed.db`

---

## 6. 設定ファイル

パス: `C:\Users\nico\AppData\Roaming\com.myquickfeed.app\my-quick-feed.yaml`

**注意**: ユーザーの実データが入っている（Discordトークン・APIキー・トピック）。削除・上書きしないこと。

```
discord:
  token: "..."          # 実トークンあり
  forum_channel_id: "..."  # 実IDあり
ai:
  mode: agent
  agent_command: omp
  agent_timeout_sec: 120
  api_key: "..."        # 実キーあり（Directモード用）
  model: deepseek-v4-flash
  base_url: "https://opencode.ai/zen/go/v1/chat/completions"
topics:
  - name: APEX
    language: ja
    interval_min: 120
    sources:
      - type: rss, url: https://apexlegends-leaksnews.com/feed
      - type: rss, url: https://www.reddit.com/r/apexlegends.rss
      - type: rss, url: http://www.reddit.com/r/ApexUncovered.rss   ← 404でエラー
      - type: rss, url: http://www.reddit.com/r/Apexclips.rss       ← 404でエラー
```

---

## 7. 既知の問題・教訓（再実装時に必ず反映）

### 7.1 RSS パーサー

- XML宣言 `<?xml ...?>` や BOM があるフィードを判定できるようにする（`strip_xml_header()` 相当）
- Reddit の `http://www.reddit.com/r/xxx.rss` は `http` だと 404 になる。**`https://` に置換する処理が必要**
- パースエラー時はレスポンスの先頭200文字をエラーに含めてデバッグ可能に

### 7.2 フロントエンド

- Dashboard の `loadData` は `Promise.all` だと1つ失敗したら全部失敗する → **個別に try/catch**
- Settings の `value={x ?? "default"}` は、消したときにデフォルトが勝手に入る → `value={x ?? ""}` + placeholder
- スクロール: `.page-body` に `flex:1; overflow-y:auto; min-height:0` が必須。`.main-content` に `overflow:hidden` + `min-height:0`

### 7.3 ログ

- `tracing_subscriber` のカスタム `Layer` で `LOG_BUFFER`（グローバル共有）に書き込む方式
- `LogEntry { timestamp, level, topic, message }` を `get_logs` IPC で返す
- ログエクスポート: `export_logs` IPC → `%APPDATA%\com.myquickfeed.app\logs\mqf_YYYYMMDD_HHMMSS.log`

### 7.4 Git

- `C:\quickfeed` で管理済み。`D:` 側は同期用
- コミット済み: 初期実装、テンプレプロンプト、スレッド方式、ログ修正、モデル設定、RSS修正、OMP修正、複数記事対応、アイコン

---

## 8. 次に実装すべきこと（SPEC 準拠で不足している部分）

1. **`seen_items` 廃止** → 直近2日分の投稿タイトルをプロンプトに埋め込む方式に移行
2. **Embed 廃止** → 通常 Markdown メッセージで投稿するように変更（spec 準拠）
3. **OMP 作業ディレクトリ分離** → `%TEMP%\my-quick-feed\omp\` + セッションクリーンアップ
4. **マルチモーダル画像対応** → 記事の画像URL を OMP に評価させる
5. **RSSHUB `base_url` + `path` 形式対応** → rsshub.rs の修正
6. **`provider` 設定** → YAML の `provider` フィールドを OMP の `--model` 解決に使う
7. **Settings のスクロール確認** → 既に CSS 修正済みだが再確認
8. **Dashboard のトピック表示** → `get_posts` の失敗でトピックが隠れないように（修正済みだが再確認）

---

## 9. 検証手順

```bat
:: 1. フロントエンド
cd C:\quickfeed
npx tsc --noEmit          :: TSエラーなし
npx vite build            :: ビルド成功

:: 2. バックエンド
cd C:\quickfeed\src-tauri
set PATH=C:\msys64\mingw64\bin;%PATH%
cargo check               :: エラーなし
cargo build --release     :: リンク成功

:: 3. 起動
cd C:\quickfeed
run.bat

:: 4. 動作確認
::  - Dashboard にトピックが表示される
::  - Settings がスクロールできる
::  - Logs にパイプラインログが出る
::  - Export でログファイルが保存される
```

---

## 10. 注意事項（ユーザーとの約束）

- ユーザーは日本語話者。返答は日本語で。
- 動作確認には `image-descriptor` サブエージェントを使う（ユーザー指定）。
- ユーザーのDiscordトークン・APIキーが設定ファイルに入っている。絶対に外部に漏らさない。
- ビルドのたびに「古いバイナリが動いている」問題が発生した。**フロントエンド変更後は必ず `vite build` → バイナリ再ビルドの順で行い、バイナリのタイムスタンプを確認すること。**
