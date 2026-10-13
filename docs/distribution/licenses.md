# ライセンスと表示

## 外部資産

| 資産 | 出所 | ライセンス | 表示・配布対応 |
|---|---|---|---|
| llama.cpp b11497 Windows CPU / Vulkan | ggml-org公式GitHubリリース | MIT | LICENSE本文を同梱。配布ZIPも保存 |
| Qwen3-0.6B-GGUF Q8_0 | Qwen公式Hugging Face、23749fef… | Apache-2.0 | 公式LICENSE・モデルカードを同梱。重みは初回取得 |
| Qwen3-Embedding-0.6B-GGUF Q8_0 | Qwen公式Hugging Face、370f27d7… | Apache-2.0 | Apache本文・公式モデルカードを同梱。重みは初回取得 |
| Qwen3.5-4B Q4_K_M | Qwen原モデル・Unsloth GGUF、固定コミット | Apache-2.0 | Apache本文・配布者と出所を明示。重みは初回取得 |
| Qwen3-1.7B Q8_0 | Qwen公式GGUF、固定コミット | Apache-2.0 | Apache本文・出所を明示。重みは初回取得 |
| LLVM OpenMP DLL | llama.cpp配布ZIP | Apache-2.0 with LLVM exceptions | ZIP内のLICENSE-LLVM-OpenMPをアプリにも同梱 |
| synthetic fixture | 本プロジェクトの架空設定 | CC0-1.0 | 実ワールドの権利を意味しない |
| アプリの自作コード | 本プロジェクト | 未選定・頒布条件未確定 | 作者の頒布方針で決定。第三者依存のライセンスとは別 |

## 実装依存

ReactはMIT、TauriはMIT OR Apache-2.0、SQLite本体はpublic domain、rusqliteはMIT、ZodはMIT、LucideはISC。完全な版別一覧と上流ライセンス本文は `resources/legal/dependencies.json` と `THIRD-PARTY-NOTICES.txt` に収録する。`tools/release/notices.py` でpackage-lock/Cargo.lockから解決された依存の実ファイルを収集する。

一覧には開発ツールと他プラットフォーム用依存も含む。どのコードが実バイナリに入るか、license-fileだけで宣言する依存、同梱CPU DLL・TLS/圧縮等のネイティブ依存の通知範囲は頒布前に最終レビューが必要。モデルにApache-2.0が表示されていることは、VRChat API・実ワールド説明文・画像・商品名の使用許諾を代替しない。

実ワールドのAPI応答と開発者連絡先は個人用保存先だけに置き、インストーラーへ同梱しない。取得情報の再配布・販売許諾を本実装から推定しない。
