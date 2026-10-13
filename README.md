# World Compass

Windows向けローカルAIワールド探索ツール。Tauri 2 + React + TypeScript + Rust + SQLite。VRChatのワールドURLをチャットまたはセットアップへ貼ると、公開情報を個人用に保存し、日本語で登録ワールドを検索・比較できます。品質重視のQwen3.5 4B、軽量なQwen3 1.7B、省メモリな0.6Bを選べます。VRChat風のシアンのチャットUIを継続しています。

公開ワールドの検索DB構築はVRChat認証が必要です。アプリ内でログイン・二段階認証を行い、まず1,000件を保存できます。パスワード・認証コードは保存せず、セッションはWindows DPAPIで暗号化します。日本語の名称・説明・タグ・作者名を取得順の指標とし、作者の国籍は推定しません。画像と日本語訳はカード表示時に取得・保存します。デモと実在ワールドは別のカタログです。対応機種や雨音・軽さが取得できないときは未確認として表示します。

モデル選定に用いたSteam統計、VRChatの過去の公式VRAM中央値、実機比較と制約は[モデル選定記録](docs/research/model-selection.md)にまとめています。

## 起動・検証

開発PCにはNode.js 24、Rust MSVC、Visual Studio C++ Build Tools、WebView2が必要です。購入者PCにNode/Rust/Python/Ollamaは要求しません。

```powershell
npm ci
npm run desktop
```

ブラウザでチャットUIと通常検索を見る場合は `npm run dev` を実行して `http://127.0.0.1:1420` を開きます。お気に入り・メモはブラウザ版でlocalStorage、デスクトップ版でSQLiteに保存します。会話履歴はデスクトップ版で個人用SQLite、プレビューではlocalStorageに保存します（最大40会話）。以前のWebView履歴は初回に移行します。AI・モデル導入・カタログ更新はデスクトップ版で利用できます。

「新しいチャット」で会話を始め、Enterで送信、Shift + Enterで改行できます。返答内に上位3候補を表示し、残りと必須条件が未確認の候補は展開して確認できます。「条件」で次のメッセージの対応機種・人数を指定できます。「条件をクリア」と送ると会話の検索条件をリセットします。ローカルLLMは検索計画・意味検索・登録情報を参照した回答を担当し、文章を逐次表示します。人数・機種・必須・除外条件はコードで保持し、AIの自由文で検索判定を変更しません。モデルなしでも通常検索は利用できます。

テーマはVRChatをイメージしたシアン（`#6ae3f9`）とダークブルー。配色は `apps/desktop/src/chat.css` 冒頭のCSS変数で管理しています。

```powershell
npm run verify
npx playwright test -c apps/desktop/playwright.config.ts
powershell -ExecutionPolicy Bypass -File tools/release/build.ps1
```

Playwrightは現開発環境のMicrosoft Edgeを使用します。他環境ではplaywright.config.tsのexecutablePathを変更してください。

AI導入・実推論のテストは選択モデルと埋め込みモデルをダウンロードするため、通常テストから分離しています。

```powershell
$env:WORLD_COMPASS_DATA_DIR = 'C:\Code\world-compass\.runtime'
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked real_local_ai -- --ignored --nocapture
```

## 実装

- 日本語条件抽出、SQLite条件検索・FTS5、ローカル埋め込みのcosine検索、好み・未訪問による順位付け。
- 必須条件が不明な候補を別枠表示。Quest対応・定員から軽さを推測しない。
- LLMの根拠選択は任意。回答はDBの候補情報を参照。JSON・許可ID・根拠を検証し、名称・リンクはDBから構築。
- SQLiteによるお気に入り・手動訪問記録・メモ・除外の保存。
- 固定版モデル（4BはUnsloth量子化）とllama.cpp CPU/Vulkan版の専用導入。Range再開、容量確認、SHA-256、検証後配置、修復。
- 127.0.0.1＋起動ごとの認証、設定可能なCPUスレッド・並列1、キャンセル、30〜600秒アイドル解放、Windows Job Object。
- 公開APIの個人用キャッシュ・24時間TTL・リクエスト間隔・429待機。デモカタログの原子的更新・差分・削除の受け口。
- ユーザーがアプリ内でVRChatにログインし、公開ワールド1,000件を取得して個人用検索DBを構築。二段階認証、Windows DPAPIによるセッション保存、中断・再開に対応。パスワード・認証コードは保存しません。
- 無断テレメトリーとクラウド推論は実装していません。公開画像はVRChatから取得して個人用にキャッシュします。本文はローカルAIで日本語訳を生成し、翻訳の表示と原文の閲覧に対応します。

## 資料

- [原設計](world-compass-plan.md)
- [一次情報の調査](docs/research/2026-10-08.md)
- [構成・データフロー・実装の制約](docs/architecture/data-flow.md)
- [検証記録](docs/research/validation.md)
- [ライセンス一覧](docs/distribution/licenses.md)
- [導入手順](docs/distribution/install.md)
- [公開ワールドDBの構築](docs/distribution/world-database.md)
- [BOOTH説明草案](docs/distribution/booth-draft.md)

製品完成版ではありません。大量カタログの検索品質・販売許諾・名称確認・最低スペック・クリーンWindows検証・コード署名・オンライン更新は未完了です。具体的な検証結果と残件は検証記録に記載します。

作成済みの開発用インストーラーは [WorldCompass-Setup-0.5.0-win-x64.exe](artifacts/WorldCompass-Setup-0.5.0-win-x64.exe)。Windows実アプリの起動、ユーザーによるログイン、日本語優先の公開情報取得を確認済み。初回AI導入とDB構築は「セットアップ」、有効化と任意の根拠選択は「設定」から行います。通常検索はモデル導入なしで使えます。

再起動は `artifacts/Start-WorldCompass.cmd`。導入済みモデルと個人用データを再利用します。画像と日本語訳はSQLiteにキャッシュし、取得・翻訳済みならオフラインでも表示できます。原文は変更しません。

?????????????????????[??????](docs/research/world-discovery-categories.md)??????????
