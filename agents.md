# AGENTS.md — 実装ハンドオフドキュメント

> このドキュメントは、SPEC.md に基づく再実装を引き継ぐエージェント向けの「実務知識」です。
> SPEC.md は理想設計、agents.md は現実の実装知識。**両方読むこと。**

---

## 1. プロジェクト概要

- **アプリ**: My Quick Feed — RSS/RSSHUB から情報を取得し、OMPエージェントで記事を生成して Discord フォーラムに自動投稿する Windows デスクトップアプリ
- **技術**: Tauri v2 + Rust + React/TypeScript (Vite)
- **開発ディレクトリ**: **`D:\quickfeed`**（ASCIIパス必須、ユーザー指定で D: ドライブに変更済み）
- **元のユーザーディレクトリ**: `D:\学校\app\my-quick-feed`（日本語パス — ビルド不可のため使わない。ドキュメント・spec の置き場としてのみ使用）

---

## 2. ビルド環境（最重要！）

### 2.1 日本語パス問題

`D:\学校\app\my-quick-feed`（日本語パス）では **Rust ビルドが失敗する**。
- MSVC リンカ (`link.exe`) が日本語パスを正しく処理できない
- MinGW の `dlltool.exe` も同様

**対策**: **`D:\quickfeed`**（ASCIIパス）でビルドする。ソース変更は必ず D:\quickfeed で行う。`D:\学校\app\my-quick-feed` には spec.md・agents.md を同期する。

※ かつて `C:\quickfeed` にコピーして開発していたが、2026-07-31 に `D:\quickfeed` へ移行（ユーザー指示: D: ドライブを優先）。git リポジトリは D:\quickfeed に移動済み。

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

### 2.5 開発時の環境トラップ（実測済み）

- **埋め込みシェルでは `cd` が効かない**。`cmd /c "cd /d C:\quickfeed && ..."` を実行しても、カレントディレクトリが `D:\学校\app\my-quick-feed` のままになる。**必ずツールの cwd パラメータで `C:\quickfeed` を指定すること**。指定しないと日本語パス側でビルドされ、node_modules 不足やリンカエラーが起きる。
- **cargo test は実行できない**（MinGW リンカの制約）。`--exclude-all-symbols` を付けるとテストバイナリが起動時 0xc0000139 でクラッシュ、外すと「export ordinal too large」でリンク失敗。テストは `cargo check` + ブラウザ検証で代替する。
- パス区切りと `%` のエスケープに注意。PowerShell の変数展開も埋め込みシェルで壊れるため、複雑な処理は .ps1/.cmd ファイルに書いて実行する。

### 2.6 起動スクリプト

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
omp -p --model mimo-v2.5 @prompt.txt   ← --model で指定可
```

YAML の `ai.model` をプロンプト内の「## 使用モデル」にも書いておくと、エージェントが従う。**ただしプロンプト内の記載だけでは不確実なため、`--model` フラグでも明示指定すること（実装済み）。**

**デフォルトモデルは `mimo-v2.5`（provider: `opencode-go`）固定**。コード内のデフォルト文字列は全てこの値にすること（`gpt-4o-mini` 等の古いデフォルトを残さない）。

**mimo 系モデル（マルチモーダル）の罠**: `omp -p @file.txt` でプロンプトを渡しても、mimo-v2.5 は「プロンプトを実行」せず「ファイルを読んで確認応答」（「何をしたいですか？」）をする。
**対策（実装済み）**: `--append-system-prompt "あなたはタスク実行エージェントです。ユーザーが渡したファイルや指示は実行すべきタスクです。指示に従って実行し、要求された出力のみを返してください。ユーザーに確認したり質問したりしないでください。"` を付ける。

### 3.5 OMP 実動作確認（2026-07-31）

- `omp -p @prompt.txt` は 8〜9 秒で JSON 配列を直接返す（deepseek-v4-flash）。stderr に `Working...` のプログレスが出るが、stdout には JSON のみ。パース戦略1（直接 `Vec<ArticleResult>` としてパース）で十分。
- `omp -p --model mimo-v2.5 --append-system-prompt ...` は 15.7 秒で JSON 配列を返す（短いプロンプト）。実記事40件では 92.8 秒で 3 記事を生成・投稿成功。
- **OMP の `-p` モードで複数 @file を渡すとハングする**（2ファイル方式は不可）。プロンプトに記事リストを埋め込む1ファイル方式にすること（実装済み）。
- **`tokio::process::Command` を使うこと**。`std::process::Command::output()` はブロッキングで `tokio::time::timeout` が効かず、OMP ハング時にアプリ全体が固まる（実装済み: kill_on_drop + timeout）。

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

**2026-07-31 更新: 以下はすべて実装済み（要修正リストを消化）**

1. ✅ `seen_items` 廃止 → 直近2日分の投稿タイトルをプロンプトに埋め込む方式（`get_recent_titles`）
2. ✅ Embed 廃止 → 通常 Markdown メッセージで投稿（タイトルは `**太字**`、画像は `![image](url)`）
3. ✅ OMP 作業ディレクトリ分離 → `%TEMP%\my-quick-feed\omp\` + `current_dir` + セッションクリーンアップ
4. ✅ posts に `tags` 列追加（ArticleResult に tags: Vec<String>、serde default）
5. ✅ RSSHUB `base_url` + `path` 形式対応（SourceConfig に base_url/path フィールド）
6. ✅ `provider` 設定 → Settings 画面で入力可能（YAML `ai.provider`）
7. ✅ Sidebar: Settings を下部の独立ボタンに変更（ナビは Dashboard/Topics/Logs の3つ）
8. ✅ Dashboard: トピックカードに記事数・最終投稿時刻を表示（`get_topic_stats` IPC）
9. ✅ スレッド作成時にトピック説明文を投稿（`create_thread` の description）
10. ✅ スタートアップ登録（tauri-plugin-autostart + Settings トグル）
11. ✅ 多重起動防止（tauri-plugin-single-instance）

**未着手（v2 / 将来）**: マルチモーダル画像認識、リアクション分析（preferences）、TinyFish返信回答、自前RSSHUB Docker対応

---

## 9. 検証手順（ユーザー指示: アプリを立ち上げない）

**重要**: ユーザーから「動作テストにはアプリを立ち上げず、バックエンドとWebブラウザでbrowserツールを使って詳細にテストすること」と指示されている。

```bat
:: 1. フロントエンド（型チェック + ビルド）
cd C:\quickfeed
npx tsc --noEmit          :: TSエラーなし
npx vite build            :: ビルド成功

:: 2. バックエンド
cd C:\quickfeed\src-tauri
set PATH=C:\msys64\mingw64\bin;%PATH%
cargo check               :: エラーなし
```

:: 3. フロントエンドの動作確認（Tauriなし・ブラウザのみ）
::    vite preview で配信して browser ツールで操作・検証する
cd C:\quickfeed
npx vite preview --port 5173 --host 127.0.0.1
::    → browser ツールで http://127.0.0.1:5173 を開き、各ページを確認
::    ※ Tauri IPC (invoke) は動かないため、Dashboard/Settings は Loading 表示になる。
::       これは想定内。画面遷移・UIレイアウト・CSSの確認に使う。

:: 4. バックエンドの動作確認（Rust 単体テスト + ログ確認）
::    cargo test でユニットテスト実行
cd C:\quickfeed\src-tauri
set PATH=C:\msys64\mingw64\bin;%PATH%
cargo test

:: 5. 実アプリ動作確認はユーザーが run.bat で実施する（開発者は起動しない）
```

### ブラウザテストのポイント

- `vite preview` で配信 → browser ツールで `tab.observe()` / `tab.screenshot()` を使って各画面を検証
- image-descriptor サブエージェントにスクリーンショットを解析させる（ユーザー指定）
- Tauri IPC が無い環境では invoke が失敗するため、**エラーがコンソールに出ても致命的ではない**（フォールバック表示を確認する）

---

## 10. 注意事項（ユーザーとの約束）

- ユーザーは日本語話者。返答は日本語で。
- 動作確認には `image-descriptor` サブエージェントを使う（ユーザー指定）。
- ユーザーのDiscordトークン・APIキーが設定ファイルに入っている。絶対に外部に漏らさない。
- ビルドのたびに「古いバイナリが動いている」問題が発生した。**フロントエンド変更後は必ず `vite build` → バイナリ再ビルドの順で行い、バイナリのタイムスタンプを確認すること。**
- モデル初期設定は `mimo-v2.5`（provider: `opencode-go`）。コード内のデフォルト文字列も全てこの値にすること（`gpt-4o-mini` 等の古いデフォルトを残さない）。
- 最初からTauriを使わず、Webブラウザ上で全機能が動作することを確認してからTauriで仕上げる（specの「開発方法」セクションに明記）。
