# ローカルモデル選定 — 2026-10-08

Windowsの日本語会話品質を優先し、Qwen3.5 4B Q4_K_Mを標準に採用した。軽量版はQwen3 1.7B Q8_0、省メモリ版は従来のQwen3 0.6B Q8_0。検索用埋め込みはQwen3 Embedding 0.6B Q8_0をCPUで実行する。

## 公開PC統計の使い方

[Steam Hardware & Software Survey、2026年9月](https://store.steampowered.com/hwsurvey/) は任意・匿名のSteam利用者調査で、VRChat利用者に限定した統計ではない。

| 指標 | 公開値と計算 |
|---|---|
| RAM最頻値 | 32GB、42.22%。16GBは37.82% |
| RAM中央値の区分 | 24GB。16GB以下47.58%、24GB以下50.14% |
| VRAM中央値の区分 | 12GB。12GB未満48.63%、12GBを含め61.69% |
| VRAM分布 | 8GB 26.71%、12GB 13.06%、16GB 27.21% |
| CPU物理コア | 8コア30.06%、6コア27.23%、4コア10.79% |

中央値は公開区分の累積からの推定。「Other」を下側に置いてもRAM16GB以下・VRAM12GB未満は50%に届かない。上側に置いても該当区分の累積は50%を超える。上限のない区分やOtherがあり、正確な平均値は算出できない。容量は搭載量であって、VRChat使用中の空き容量ではない。

[VRChat公式 Developer Update、2023-02-16](https://ask.vrchat.com/t/developer-update-16-february-2023/16474) はSteam調査と内部分析を参考にVRAM8GBを中央値として設計したと説明している。2023年のアバター性能設計の根拠であり、2026年の利用者中央値ではない。OS・VR compositor・ワールドもVRAMを消費するため、搭載VRAMすべてをLLMへ割り当てない。

[VRChat公式最低要件](https://help.vrchat.com/hc/en-us/articles/1500002378722-System-Requirements) は利用者分布やLLM同時利用の要件ではない。VRChatおよびBOOTHの最新の母集団付きPCスペック平均・中央値は、今回の公開一次情報の調査では確認できなかった。BOOTHの商品要件や個別アンケートを母集団の統計として代用しない。

## モデルと実装

| 設定 | 重みの取得量 | 用途 |
|---|---:|---|
| 日本語品質 | Qwen3.5 4B Q4_K_M、2.74GB | 会話・検索計画・登録情報を参照した回答 |
| 軽量 | Qwen3 1.7B Q8_0、1.83GB | 低負荷寄りの会話・CPU実行 |
| 省メモリ | Qwen3 0.6B Q8_0、0.64GB | 制約のあるPC。品質の主推奨にはしない |

容量は10進GBで、実行時メモリではない。4B選択時の初回取得は埋め込みとCPU/Vulkanランタイムを含め約3.43GB。別途展開・修復用の空き容量を確認する。

[Qwen3.5 4Bの公式モデルカード](https://huggingface.co/Qwen/Qwen3.5-4B) はApache-2.0、201言語・方言対応を公表している。言語数は日本語精度の保証ではない。GGUFは[Unslothの量子化](https://huggingface.co/unsloth/Qwen3.5-4B-GGUF)を使用し、Qwen公式のGGUF配布とは区別する。全資産はコミット・サイズ・SHA-256を固定し、導入時・起動時に検証する。量子化配布者を隠さない。

GPUはllama.cpp b11497 Vulkan、埋め込みとGPU失敗時はCPU。RTX3060でVulkanデバイスを確認した。並列1、コンテキスト4096、推論思考モードを無効化。チャットと埋め込みのプロセスを保持し、毎段階のモデル再読込を避ける。停止・終了・アイドル期限で解放する。

「VRChat同時利用向け」は1.7B・CPU2スレッド・30秒アイドル解放。「日本語品質」は4B・GPU・CPU4スレッド・120秒。前者はGPUの取り合いを減らす選択肢であり、VRChatのFPSやVRフレーム時間を保証しない。VRChatを実際に起動した同時負荷の検証は未実施。

## 品質評価の範囲

同一PC（i5-10400F、RAM48GiB、RTX3060 12GiB）で0.6B、1.7B、Qwen3 4B、Qwen3.5 4Bを比較した。少数の日本語条件抽出・短い会話ではQwen3.5の会話が主用途に合った。一方、4Bでも未指定の除外条件を出すことがある。公開ベンチマークの順位だけで正確さを保証しない。

探索段階の`artifacts/model-benchmark/results*.json`の`passCount`は、リストの部分一致や「一人」の旧期待値を含む粗い診断値であり、精度率として使用しない。アプリでは人数・機種・必須・除外を明示入力とコードで保持し、AIが追加できるのは希望属性と意味検索文。条件とDBの一致判定、未知の情報の表示をAIの自由文から分離する。

実アプリの測定・検証結果は`artifacts/practical-smoke.json`と検証記録を参照。少数ケース・このPCの観察値で、一般ユーザーの最低スペックや平均速度ではない。

## 実在ワールド

[VRChat Creator Guidelines](https://hello.vrchat.com/creator-guidelines)に沿って連絡先付きUser-Agent、キャッシュ、リクエスト間隔、429時の待機を実装。GET `/worlds/{id}`の公開情報だけを取得する。匿名のGET `/worlds`は401であり、広域検索や人気一覧は実装しない。ログイン情報・Cookieは扱わない。

説明・作者タグは作者の記載であり体験確認ではない。匿名応答でunityPackagesが空なら対応機種はunknown、雨音・夜・軽さ等もunknown。個人用SQLiteキャッシュとデモを分離し、インストーラーへ実ワールド情報や連絡先を同梱しない。
