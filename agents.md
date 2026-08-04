# AGENTS.md — 実装ハンドオフドキュメント

> このドキュメントは、SPEC.md に基づく再実装を引き継ぐエージェント向けの「実務知識」です。
> SPEC.md は理想設計、agents.md は現実の実装知識。**両方読むこと。**
> 2026-07-31 時点で実装・検証済みの全ノウハウを集約。

---

## 1. プロジェクト概要

- **アプリ**: My Quick Feed — RSS/RSSHUB から情報を取得し、OMPエージェントで記事を生成して Discord フォーラムに自動投稿する Windows デスクトップアプリ
- **技術**: Tauri v2 + Rust + React/TypeScript (Vite)

### ディレクトリ構成（2026-07-31 整理済み）

| 場所 | 役割 | git |
|---|---|---|
| **`D:\学校\app\my-quick-feed`** | **本拠地（元の場所）**。ソース編集・git 管理はここ | ✅ master（e7dc770 まで） |
| **`D:\quickfeed`** | **テスト用**。日本語パスではビルド不可のため、ビルド・実行はここで行う（D:\学校 からソースを同期） | ❌ なし |
| ~~`C:\quickfeed`~~ | ~~旧開発場所~~ 削除済み（2026-07-31） | ❌ |

**注意**: 日本語パス（D:\学校\app\my-quick-feed）では Rust ビルドが失敗する（MSVC/MinGW リンカが日本語パスを処理できない）。**ビルドは必ず D:\quickfeed で行うこと**。

### 開発フロー（重要）

```
1. D:\学校\app\my-quick-feed でソース編集 → git commit
2. D:\quickfeed にソースを同期: robocopy D:\学校\app\my-quick-feed D:\quickfeed /MIR /XD target node_modules dist .git /XF run.bat /NFL /NDL /NJH /NJS
3. run.bat 本体（run-build.bat）を同期: copy D:\学校\app\my-quick-feed\run-build.bat D:\quickfeed\run.bat
4. D:\quickfeed でビルド・テスト（cwd パラメータで D:\quickfeed を指定）
5. run.bat は D:\quickfeed のものを実行（D:\学校 の run.bat はラッパー）
```

**⚠ run.bat は同期から除外すること（/XF run.bat）**: D:\学校 の run.bat はラッパー（D:\quickfeed を呼ぶだけ）。/MIR 同期でラッパーが D:\quickfeed の本体 run.bat を上書きすると、run.bat が自分自身を call する無限ループになり「動かない」（2026-08-04 実害あり・修正済み）。**run.bat 本体は `run-build.bat` として git 管理**し、同期後に copy で D:\quickfeed\run.bat に反映する（copy コマンドは上記 3 番）。
**⚠ run.bat はビルド前に旧アプリを taskkill する**（`taskkill /IM my-quick-feed.exe /F`）。アプリ起動中に再実行すると exe がロックされ cargo build が失敗するため（2026-08-04 実害あり）。

---

## 2. ビルド環境（最重要！）

### 2.1 日本語パス問題

`D:\学校\app\my-quick-feed`（日本語パス）では **Rust ビルドが失敗する**。
- MSVC リンカ (`link.exe`) が日本語パスを正しく処理できない
- MinGW の `dlltool.exe` も同様

**対策**: **`D:\quickfeed`**（ASCIIパス）でビルドする。ソース変更（git管理）は `D:\学校\app\my-quick-feed` で行い、robocopy で D:\quickfeed へ同期してからビルドする。

### 2.2 ツールチェーン（インストール済み）

| ツール | パス | 用途 |
|---|---|---|
| Rust (GNU) | `C:\Users\nico\.cargo\bin\rustc.exe` | **stable-x86_64-pc-windows-gnu** をデフォルトに |
| MinGW-w64 | `C:\msys64\mingw64\bin\gcc.exe` | Cリンカ（WinLibs 16.1.0、2026-08-04 再展開） |
| LLVM (lld-link) | `C:\Program Files\LLVM\bin\lld-link.exe` | 代替リンカ（MSVC用、未使用） |
| Node.js | `C:\Users\nico\AppData\Roaming\npm` | フロントエンド |
| Git | `C:\Program Files\Git\cmd\git.exe` | リポジトリ管理 |
| OMP | PATH 上（`omp --help` で確認可能） | AIエージェント |

**⚠ MinGW 消失事件（2026-08-04）**: `C:\msys64` が丸ごと消えていた（原因不明。当日のビルド時点では存在）。run.bat が `[ERROR] MinGW not found` で失敗する。復元手順:
1. WinLibs の zip をダウンロード（GitHub: brechtsanders/winlibs_mingw、`winlibs-x86_64-posix-seh-gcc-*-mingw-w64msvcrt-*.zip` を選択）
2. `mkdir C:\msys64` → `C:\Windows\System32\tar.exe -xf winlibs.zip -C C:\msys64`（Windows 標準 tar は zip 展開可）
3. `C:\msys64\mingw64\bin\gcc.exe --version` で確認 → ビルド再開
- 消えたら run.bat のエラーメッセージで気づく。.cargo/config.toml の linker パスはそのままで OK（同じ場所に展開するため）。

### 2.3 ビルドコマンド

```bat
:: フロントエンド
cd /d D:\quickfeed
npx tsc --noEmit        :: TS型チェック
npx vite build          :: フロントビルド

:: バックエンド（cargo check / build）
cd /d D:\quickfeed\src-tauri
set PATH=C:\msys64\mingw64\bin;%PATH%
cargo check             :: 型チェック
cargo build --release   :: リリースビルド
```

**注意**: `.cargo/config.toml` に RUSTFLAGS が設定済み:
```toml
[target.x86_64-pc-windows-gnu]
linker = "C:\\msys64\\mingw64\\bin\\gcc.exe"
rustflags = ["-C", "link-arg=-Wl,--exclude-all-symbols"]
```
**`--exclude-all-symbols` がないと MinGW の ld が「export ordinal too large」でリンクに失敗する**（PEフォーマットの制限、エクスポート順序が 65535 を超える）。絶対に消さないこと。

**⚠ `custom-protocol` feature 必須（2026-08-04 実害あり）**: `Cargo.toml` の tauri 依存は `features = ["tray-icon", "custom-protocol"]` であること。`tauri build`（tauri-cli）は自動で custom-protocol を有効化するが、**素の `cargo build --release`（run.bat の方式）は feature を付けない**。custom-protocol が無いと dev モードでビルドされ、WebView が `devUrl`（localhost:1420）をロードしようとして「localhost 接続が拒否されました」になる（vite dev が動いていないため）。run.bat でビルドする限り、**この feature が無いと UI が一切表示されない**。

### 2.4 環境トラップ（実測済み・最重要）

1. **埋め込みシェルでは `cd` が効かない**。`cmd /c "cd /d D:\quickfeed && ..."` を実行しても、カレントディレクトリが変わらない（D:\学校 のまま）。**必ずツールの cwd パラメータで `D:\quickfeed`（または `D:\quickfeed\src-tauri`）を指定すること**。指定しないと日本語パス側でビルドされ、node_modules 不足やリンカエラーが起きる。
2. **`cargo test` は実行できない**（MinGW リンカの制約）。`--exclude-all-symbols` を付けるとテストバイナリが起動時 0xc0000139 でクラッシュ、外すと「export ordinal too large」でリンク失敗。テストは `cargo check` + ブラウザ検証 + 実機パイプライン検証で代替する。
3. **PowerShell の変数展開が埋め込みシェルで壊れる**（`$var` が消える）。複雑な処理は .ps1/.cmd ファイルに書いて `powershell -ExecutionPolicy Bypass -File xxx.ps1` で実行する。
4. **PowerShell 5.1 は BOM なし UTF-8 の ps1 内の日本語を読めない**。ps1 に日本語を書く場合は注意。ASCII のみにするか UTF-8 BOM 付きで保存する。
5. **bat ファイルに日本語を書かない（2026-08-04 実害あり）**。UTF-8 で保存した bat の日本語コメント/echo は、cmd がコードページ 932（Shift-JIS）で読み込むため文字化けし、**後続の行（call やパス指定）まで壊れる**（`'[INFO]' is not recognized` エラー、run.bat が動かない）。bat は**完全に ASCII のみ**で書くこと。D:\学校 の run.bat（ラッパー）は ASCII 化済み。
5. **`findstr` は 769 バイトで出力が切れる**。minified な JS/CSS の検索には不向き。`Select-String`（PowerShell）を使う。

### 2.5 MinGW のヘッダー修復（2026-07-31 実績）

- `C:\msys64\mingw64\include\stdarg.h` が無いと `#include_next <stdarg.h>` エラーで `libsqlite3-sys` のビルドが失敗する
- pacman のローカル DB が壊れていて `pacman -S` が失敗する場合は、パッケージを直接ダウンロードして展開する:
  ```
  # パッケージのルートが mingw64/ なので -C C:\msys64 で展開すること
  C:\Windows\System32\tar.exe -xf mingw-w64-x86_64-headers-*.pkg.tar.zst -C C:\msys64
  ```
  （Windows 標準 tar は zst 対応。MSYS の tar は Windows パスを解釈できないので使わない）

### 2.6 起動スクリプト

- `run.bat` — フロントエンド→バックエンドを毎回ビルドして起動（`%~dp0` 方式なのでどこに置いても動く）
- dev.bat / run-dev.bat は削除済み（ユーザーが不要と判断）

---

## 3. OMP CLI の実務知識（最重要・実測済み）

### 3.1 正しい呼び出し方

```
# 非インタラクティブ + ファイルからプロンプト読み込み（標準）
omp -p @<prompt_file_path>

# モデルを明示指定
omp -p --model mimo-v2.5 @prompt.txt

# mimo 系モデルはタスク実行を強制する append-system-prompt が必須
omp -p --model mimo-v2.5 --append-system-prompt "あなたはタスク実行エージェントです。..." @prompt.txt
```

### 3.2 ハマりポイント（実測済み）

1. **`omp task` は存在しない**。`omp --help` で確認済み。`task` は OMP 内部のサブエージェント起動ツール名。
2. **`--mode json` は使わない**。OMP の内部セッションプロトコル（JSONL）を出力するもので、記事 JSON ではない。
3. **stdin 経由ではプロンプトを渡せない**。`omp -p` は引数 or `@file` でのみ受け取る。
4. **ウインドウが出る** → `CREATE_NO_WINDOW` (0x08000000) フラグが必要（`tokio::process::Command` の `creation_flags`）。tokio の Command は `creation_flags` を inherent メソッドとして持つ。
5. **セッション汚染** → 作業ディレクトリを `%TEMP%\my-quick-feed\omp\` に分離すること（`current_dir()` 指定）。OMP のセッションは `{cwd}/.omp/agent/sessions/` に作られる。
   - **⚠ 方針（ユーザー指定 2026-08-03）**: **セッションの削除は行わない**。ユーザーの当初の意図は「OMP の実行フォルダを %TEMP% に退避してセッションを分離する」だけで、セッションファイルの削除は求めていない。`cleanup_omp_sessions()` は勝手に追加された機能で実害（下記）を起こしたため、**完全に削除済み**。以後、セッション削除系のコードを追加しないこと。cwd 分離のみで対応する。
   - **重要（2026-08-03 実測）**: セッションは **`%USERPROFILE%\.omp\agent\sessions\`（グローバル）にも作られ、実行のたびに溜まり続ける**。ディレクトリ名は cwd パス由来。**削除しない方針のため、溜まるのは許容**（害はディスク消費のみ）。他プロジェクトのセッションには絶対に触れない。
   - **⚠ 命名規則の実測（2026-08-03 重要）**: 新命名のプレフィックスは **cwd がホームディレクトリ配下なら `home-`、ホーム外なら `abs-`**。実測: %TEMP%\my-quick-feed\omp で `omp -p` 実行 → **`home-omp-<sha256>` が作られた**（hash = cwd を前方スラッシュ化した文字列の SHA-256。`C:/Users/nico/AppData/Local/Temp/my-quick-feed/omp` → d61fb329… で一致確認済み）。D:\学校\app\my-quick-feed では `abs-my-quick-feed-c363672c…`。**cleanup は `abs-` と `home-` の両方を完全一致で判定する**（2026-08-03 修正・実機検証済み: アプリ起動で `home-omp-<hash>` が削除され、ユーザーセッションは残存）。`home-` のプレフィックス一致も絶対にしないこと（他のホーム配下プロジェクトを巻き込む）。
   - **⚠ 重大な教訓（2026-08-03、実害あり）**: `abs-my-quick-feed-` のプレフィックス一致で削除すると、**ユーザーが `D:\学校\app\my-quick-feed` で対話的に使っている本物のセッションまで削除する**（`abs-my-quick-feed-c363672c…` = ユーザー作業ディレクトリの cwd ハッシュ。2026-07-15 の初期構築セッションがこのバグで消えた）。絶対にプレフィックス一致に戻さないこと。
   - **復元方法（実績あり 2026-08-03）**: `.jsonl` が消えても `~/.omp/agent/history.db` の `history` テーブル（`session_id` 列）にユーザーメッセージ全文が残っている。同IDの `.jsonl` を `title` / `session`(version 3) / `model_change` / `thinking_level_change` / `message` / `title_change` の行形式で再構築すれば resume 可能（検証は `omp --export <file> <out.html>` でロード確認）。
6. **コマンドライン長制限** → Windows は 8191 文字まで。プロンプトが 30〜70KB になるので **必ず `@file` 方式** を使う。
7. **`tokio::process::Command` を使うこと（重大）**。`std::process::Command::output()` はブロッキングで `tokio::time::timeout` が効かず、OMP ハング時にアプリ全体が固まる。`tokio::process::Command` + `kill_on_drop(true)` + `tokio::time::timeout` で、タイムアウト時に子プロセスを kill する（実装済み）。
8. **`-p` モードで複数 @file を渡すとハングする**（2ファイル方式は不可・実測）。プロンプトに記事リストを埋め込む 1ファイル方式にすること（実装済み）。
9. **プロンプトサイズ制限**: 記事 85 件で 73KB になると OMP が処理できない。**最大 40 件に制限**すること（pipeline.rs で truncate、実装済み）。
10. **OMP 実行のたびに `Working...` が stderr に出る**。stdout には JSON のみ。エラー判定は `status.code()` と stdout のパース結果で行う。

### 3.3 モデル差の原因と対策（重要）

| モデル | @file プロンプトの挙動 |
|---|---|
| **deepseek-v4-flash** | プロンプトとして実行 → JSON を返す（13.8秒） |
| **mimo-v2.5** | 「読むべき添付ファイル」と認識し「何をしたいですか？」と確認応答（22.7秒） |
| **mimo-v2.5 + --append-system-prompt** | 正常にタスク実行（15.7秒〜92.8秒） |

**mimo 系モデル（マルチモーダル）は対話型・確認型**で、OMP のデフォルトシステムプロンプト（コーディングアシスタント）では「プロンプトを実行」と解釈しない。
**対策（実装済み）**: model 名に `mimo` が含まれる場合、`--append-system-prompt` を付ける:

```
あなたはタスク実行エージェントです。ユーザーが渡したファイルや指示は実行すべきタスクです。指示に従って実行し、要求された出力のみを返してください。ユーザーに確認したり質問したりしないでください。
```

### 3.4 出力パース

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

### 3.5 モデル指定とデフォルト

- YAML の `ai.model` をプロンプト内の「## 使用モデル」にも書いておくと、エージェントが従う。**ただしプロンプト内の記載だけでは不確実なため、`--model` フラグでも明示指定すること（実装済み）**。
- **デフォルトモデルは `mimo-v2.5`（provider: `opencode-go`）固定**。コード内のデフォルト文字列は全てこの値にすること（`gpt-4o-mini` 等の古いデフォルトを残さない）。
- ユーザーの実設定は `%APPDATA%\com.myquickfeed.app\my-quick-feed.yaml` の `ai.model`（2026-07-31 現在: mimo-v2.5 に変更済み、バックアップ .bak あり）。

### 3.6 OMP 実動作確認（2026-07-31）

- `omp -p @prompt.txt`（deepseek-v4-flash）: 8〜9 秒で JSON 配列を直接返す
- `omp -p --model mimo-v2.5 --append-system-prompt ...`（短いプロンプト）: 15.7 秒
- 実記事 40 件 + mimo-v2.5: 92.8 秒で 3 記事を生成・Discord 投稿成功
- パイプライン全体（Reddit 429 リトライ込み）: 229.7 秒

---

## 4. Discord REST API（serenity 不使用）

spec には `serenity + poise` と書いてあるが、**実装は REST API 直呼び出し**（reqwest）。v2 のリアクション監視で Gateway を使う予定。

### 4.1 エンドポイント

```
POST https://discord.com/api/v10/channels/{forum_channel_id}/threads   # スレッド作成（説明文を最初の投稿に）
POST https://discord.com/api/v10/channels/{thread_id}/messages         # 記事投稿（通常メッセージ）
GET  https://discord.com/api/v10/users/@me                             # トークン検証
```

ヘッダー: `Authorization: Bot {token}`、`Content-Type: application/json`、`User-Agent: MyQuickFeed/0.1`

### 4.2 実装済みの投稿方式（Embed なし・通常メッセージ）

```rust
// create_thread: スレッド作成 + トピック説明文を最初の投稿に
POST /channels/{forum_channel_id}/threads
{ "name": "トピック名", "message": { "content": "🖊️ 【トピック名】...まとめます", "embeds": [] } }

// post_article: 記事を通常メッセージで投稿
POST /channels/{thread_id}/messages
{ "content": "**【APEX】タイトル**\n\n本文...\n\n![image](url)", "embeds": [] }
```

### 4.3 スレッド管理

- `topic_threads` テーブルでトピック→スレッドID を永続化。初回のみスレッド作成、以降は既存スレッドに追記。
- スレッド作成時はトピック説明文を最初の投稿にする（spec 準拠）。

---

## 5. データベース設計（実装済み・spec 準拠）

```sql
CREATE TABLE posts (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    topic_id           TEXT NOT NULL,
    title              TEXT NOT NULL,
    content            TEXT NOT NULL,
    image_url          TEXT,
    tags               TEXT,                -- JSON配列: ["競技シーン","バグ"]
    discord_message_id TEXT,
    discord_thread_id  TEXT,
    created_at         TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE topic_threads (
    topic_id    TEXT PRIMARY KEY,
    thread_id   TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE reactions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    post_id     INTEGER NOT NULL REFERENCES posts(id),
    emoji       TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
```

- **`seen_items` テーブルは廃止**。重複防止は「直近2日分の投稿タイトルをプロンプトに埋め込む」方式（`get_recent_titles`）+ 同一実行内はメモリ上の HashSet。
- 既存 DB への `tags` 列追加は `PRAGMA table_info(posts)` で確認してから `ALTER TABLE` で自動マイグレーション（実装済み）。
- DB パス: `C:\Users\nico\AppData\Roaming\com.myquickfeed.app\my-quick-feed.db`

---

## 6. 設定ファイル

パス: `C:\Users\nico\AppData\Roaming\com.myquickfeed.app\my-quick-feed.yaml`

**注意**: ユーザーの実データが入っている（Discordトークン・APIキー・トピック）。削除・上書きしないこと。変更する場合は必ずバックアップ（.bak）を取る。

```
discord:
  token: "..."              # 実トークンあり
  forum_channel_id: "..."   # 実IDあり
ai:
  mode: agent
  agent_command: omp
  agent_timeout_sec: 120
  api_key: "..."            # 実キーあり
  model: mimo-v2.5          # 2026-07-31 変更済み
  provider: opencode-go
  base_url: "https://opencode.ai/zen/go/v1/chat/completions"
topics:
  - name: APEX
    language: ja
    interval_min: 120
    sources:
      - type: rss, url: https://apexlegends-leaksnews.com/feed
      - type: rss, url: https://www.reddit.com/r/apexlegends.rss
      - type: rss, url: http://www.reddit.com/r/ApexUncovered.rss   ← http→https自動置換で対応
      - type: rss, url: http://www.reddit.com/r/Apexclips.rss       ← 429で失敗することあり（リトライ5回）
```

**BOM の罠（実測）**: PowerShell で `[System.IO.File]::WriteAllText($path, $content, [System.Text.Encoding]::UTF8)` と書くと **BOM 付き UTF-8** になり、serde_yaml が `missing field 'ai'` エラーで設定を読めなくなる。
**対策**: `New-Object System.Text.UTF8Encoding -ArgumentList $false` で BOM なしエンコーディングを作って使う。

---

## 7. 既知の問題・教訓（再実装時に必ず反映）

### 7.1 RSS パーサー

- XML宣言 `<?xml ...?>` や BOM があるフィードを判定できるようにする（`strip_xml_header()` 相当、実装済み）
- Reddit の `http://www.reddit.com/r/xxx.rss` は `http` だと 429/404 になる。**`https://` に置換する処理が必要**（実装済み）
- **429（レート制限）対策**: Retry-After ヘッダーを尊重しつつ、最大5回リトライ（5/10/15/20/25秒バックオフ、実装済み）
- パースエラー時はレスポンスの先頭200文字をエラーに含めてデバッグ可能に（実装済み）
- ソース間のリクエストに 2 秒のディレイ（Reddit のレート制限回避、実装済み）
- **⚠ RSSHUB の URL 二重結合（2026-08-04 修正）**: 設定が `url: "https://rsshub.app/twitter/user/PlayApex"`（フル URL）で base_url/path 未指定の場合、旧実装はデフォルト base_url と結合して `https://rsshub.app/https://rsshub.app/...` の二重 URL になり 403 になる。**`path` が http(s):// で始まる場合はそのまま使う**よう修正（`build_rsshub_url`）。設定は url フル指定でも base_url+path 形式でも動く。

### 7.2 フロントエンド

- Dashboard の `loadData` は `Promise.all` だと1つ失敗したら全部失敗する → **個別に try/catch**（実装済み）
- Settings の `value={x ?? "default"}` は、消したときにデフォルトが勝手に入る → `value={x ?? ""}` + placeholder（実装済み）
- **スクロール**: `.fade-in` に `flex:1; min-height:0; display:flex; flex-direction:column` が必須（これが無いと `.page-body` の `overflow-y:auto` が機能しない）。`.page-body` に `flex:1; overflow-y:auto; min-height:0`。`.main-content` に `overflow:hidden; min-height:0`（すべて実装済み）
- **テーマ**: 黒背景 `#0a0a0c` ベース。色はアクセント（状態表示）のみ。グラデーション禁止（ユーザー指示）
- Sidebar: ナビは Dashboard/Topics/Logs の3つ。Settings は下部の独立ボタン（`.sidebar-settings`）
- Dashboard: 稼働状態ピル（Running/Stopped）、トピックカードに記事数・最終投稿時刻・次回実行カウントダウン
- **⚠ Tauri v2 の invoke 引数は camelCase 必須（重大・2026-08-04 修正）**: Rust 側のパラメータが `topic_id` でも、JS 側は `{ topicId }` で渡す。snake_case で渡すと「missing field `topicId`」エラーで**静かに失敗**する。これが「Refresh ボタンが効かない」問題の根本原因だった（以前はトーストで可視化しただけで、原因は直っていなかった）。Dashboard の `get_posts`/`refresh_topic`、TopicsPage の `refresh_topic` を修正済み。
- **⚠ SQLite の `created_at` は UTC**（`datetime('now')` = "YYYY-MM-DD HH:MM:SS"）。`new Date(iso)` にそのまま渡すとローカル時刻として解釈され、JST で 9 時間ずれる。**`iso.replace(" ", "T") + "Z"` で UTC 解釈すること**（2026-08-04 修正。fmtRelative/fmtLastTime は共通の `parseUtc` を使う）。
- **TopicsPage のリネームバグ（2026-08-04 修正）**: 既存トピックの名前を変更して保存すると、`t.name === editing.name`（編集後の名前）でマッチングして一致せず、**更新が黙って消える**。編集開始時に元の名前を `originalName` に保存してマッチングすること。
- **BrowserRouter は Tauri で使わない**（2026-08-04 修正）: 非ルートパス（/topics 等）でのリロード時に WebView2 が index.html を返さず 404 になる。**HashRouter を使用**。
- トピック保存時はバリデーション必須（名前空・ソース URL 空・重複名を弾く。2026-08-04 実装）。失敗は console.error でなくユーザーに見えるメッセージで（保存の無言消失防止）。

### 7.3 ログ

- **`tracing_appender::non_blocking` はバッファ満杯でブロックしアプリ全体が固まる（実測・重大）**。ログファイルは **同期 `std::fs::OpenOptions::append` で書くこと**（実装済み）。
- `tracing_subscriber` のカスタム `Layer` で `LOG_BUFFER`（グローバル共有、`std::sync::LazyLock<Arc<parking_lot::Mutex<Vec<LogEntry>>>>`）に書き込む方式（実装済み）
- `LogEntry { timestamp, level, topic, message }` を `get_logs` IPC で返す
- ログエクスポート: `export_logs` IPC → `%APPDATA%\com.myquickfeed.app\logs\mqf_YYYYMMDD_HHMMSS.log`
- 常時ログ: `%APPDATA%\com.myquickfeed.app\logs\app.log`（同期書き込み）
- **ログファイルは正しい UTF-8 で書かれる**。PowerShell コンソールで文字化けして見えても、ファイル自体は正常（表示エンコーディングの問題）。メモ帳や VS Code で開けば正しい。

### 7.4 スケジューラ

- **`tokio::time::interval` の最初の tick は即発火する**。起動時即実行（1回）+ ループの最初の tick で2回連続実行される罠がある。**`timer.tick().await` を一度消費してからループに入る**こと（実装済み）。
- **2026-08-04 変更**: interval 方式をやめ、**「実行完了後に sleep(interval)」方式**にした。パイプライン実行が interval より長い（例: OMP 180s タイムアウト vs interval 60s）と、interval の即発火で実行完了直後に連続実行されるため。次回実行予定時刻も実実行ベースで正確になる。
- **⚠ 同一トピックの並行実行ガード（2026-08-04 実装）**: 手動 `refresh_topic` とスケジューラ定期実行が同時に走ると、重複防止（直近タイトル）が実行開始時のスナップショットなので**二重投稿する**。Scheduler に `running` マップを追加し、実行中なら定期実行はスキップ / refresh はエラーを返す。
- 次回実行予定時刻を `Arc<SyncMutex<HashMap<String, DateTime<Local>>>>` で保持し、`get_status` IPC で Dashboard に表示（実装済み）。

### 7.5 Git

- **正規リポジトリは `D:\学校\app\my-quick-feed`**（2026-07-31 に D:\quickfeed から移設）。ブランチ `master`。
- `D:\quickfeed` はテスト用コピーで **.git なし**。`C:\quickfeed` は削除済み。
- リモート: `origin` = `https://github.com/nico2525nn/my-quick-feed.git`（2026-07-31 に設定・Push 済み）。
- **コミットまでは勝手に行ってよいが、Push はユーザーの指示があるまで実行しない**（ユーザー指定 2026-07-31）。
- コミット済み: 初期実装、テンプレプロンプト、スレッド方式、ログ修正、モデル設定、RSS修正、OMP修正、複数記事対応、アイコン、spec準拠修正、UX改善、mimo対応、agents.md集約

---

## 8. 実装済み機能（SPEC 準拠、2026-07-31 時点）

1. ✅ `seen_items` 廃止 → 直近2日分の投稿タイトルをプロンプトに埋め込む方式（`get_recent_titles`）
2. ✅ Embed 廃止 → 通常 Markdown メッセージで投稿（タイトルは `**太字**`、画像は `![image](url)`）
3. ✅ OMP 作業ディレクトリ分離 → `%TEMP%\my-quick-feed\omp\` + `current_dir` + セッションクリーンアップ
4. ✅ posts に `tags` 列追加（ArticleResult に tags: Vec<String>、serde default）
5. ✅ RSSHUB `base_url` + `path` 形式対応（SourceConfig に base_url/path フィールド）
6. ✅ `provider` 設定 → Settings 画面で入力可能（YAML `ai.provider`）
7. ✅ Sidebar: Settings を下部の独立ボタンに変更（ナビは Dashboard/Topics/Logs の3つ）
8. ✅ Dashboard: トピックカードに記事数・最終投稿時刻・次回実行カウントダウン表示（`get_topic_stats` / `get_status` IPC）
9. ✅ スレッド作成時にトピック説明文を投稿（`create_thread` の description）
10. ✅ スタートアップ登録（tauri-plugin-autostart + Settings トグル）
11. ✅ 多重起動防止（tauri-plugin-single-instance）
12. ✅ 複数記事生成（0〜N件、JSON配列）
13. ✅ OMP: `--model` 明示指定 + mimo 用 `--append-system-prompt`
14. ✅ OMP: `tokio::process::Command` + kill_on_drop + timeout（ハング対策）
15. ✅ RSS: 429 リトライ（Retry-After 尊重、5回）+ Reddit https 置換 + 2秒ディレイ
16. ✅ アイテム数 40 件制限（OMP コンテキスト対策）
17. ✅ ログ: 同期ファイル書き込み（non_blocking の罠回避）

**未着手（v2 / 将来）**: マルチモーダル画像認識（画像ファイルを @file で渡す方式）、リアクション分析（preferences）、TinyFish返信回答、自前RSSHUB Docker対応、投稿内容のアプリ内表示（ユーザー要望）

---

## 9. 検証手順（ユーザー指示: アプリを立ち上げない）

**重要**: ユーザーから「動作テストにはアプリを立ち上げず、バックエンドとWebブラウザでbrowserツールを使って詳細にテストすること」と指示されている。

```bat
:: 1. フロントエンド（型チェック + ビルド）— cwd は D:\quickfeed
npx tsc --noEmit          :: TSエラーなし
npx vite build            :: ビルド成功

:: 2. バックエンド — cwd は D:\quickfeed\src-tauri
set PATH=C:\msys64\mingw64\bin;%PATH%
cargo check               :: エラーなし
cargo build --release     :: リンク成功

:: 3. フロントエンドの動作確認（Tauriなし・ブラウザのみ）
::    vite preview で配信して browser ツールで操作・検証する
npx vite preview --port 5173 --host 127.0.0.1
::    → browser ツールで http://127.0.0.1:5173 を開き、各ページを確認
::    ※ Tauri IPC (invoke) は動かないため、Dashboard/Settings は Loading 表示になる。
::       これは想定内。画面遷移・UIレイアウト・CSSの確認に使う。

:: 4. バックエンドの動作確認（実パイプライン）
::    アプリをバックグラウンド起動（Start-Process で）→ ログを確認
::    app.log: %APPDATA%\com.myquickfeed.app\logs\app.log
::    ※ フォアグラウンド起動はツールのタイムアウトで kill されるため、
::      Start-Process（バックグラウンド）で起動すること

:: 5. 実アプリ動作確認はユーザーが run.bat で実施する（開発者は起動しない）
```

### ブラウザテストのポイント

- `vite preview` で配信 → browser ツールで `tab.observe()` / `tab.screenshot()` を使って各画面を検証
- image-descriptor サブエージェントにスクリーンショットを解析させる（ユーザー指定）
- **browser ツールの screenshot は `save:` パラメータを無視し、`%TEMP%\omp-sshots-*.webp` に保存される**。webp ファイルをコピーして image-descriptor に渡すこと
- Tauri IPC が無い環境では invoke が失敗するため、**エラーがコンソールに出ても致命的ではない**（フォールバック表示を確認する）
- **ポート 5173 は古い preview プロセスが占有し続けることがある**。`hub ps` で確認して古いプロセスを停止してから起動すること

---

## 10. 注意事項（ユーザーとの約束）

- ユーザーは日本語話者。返答は日本語で。
- 動作確認には `image-descriptor` サブエージェントを使う（ユーザー指定）。クレジット不足（402）で失敗したら、ユーザーに伝えて補充を依頼する。
- ユーザーのDiscordトークン・APIキーが設定ファイルに入っている。絶対に外部に漏らさない。
- ビルドのたびに「古いバイナリが動いている」問題が発生した。**フロントエンド変更後は必ず `vite build` → バイナリ再ビルドの順で行い、バイナリのタイムスタンプを確認すること。**
- モデル初期設定は `mimo-v2.5`（provider: `opencode-go`）。コード内のデフォルト文字列も全てこの値にすること（`gpt-4o-mini` 等の古いデフォルトを残さない）。
- 最初からTauriを使わず、Webブラウザ上で全機能が動作することを確認してからTauriで仕上げる（specの「開発方法」セクションに明記）。
- 設定ファイルの変更（model 切替など）はバックアップを取ってから行う（.bak 方式）。
