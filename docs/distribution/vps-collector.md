# VPS収集とPC検索

ConoHa VPS（Ubuntu 24.04、1GB、2コア）用に、Python標準ライブラリだけで動く収集プログラムを `services/collector` に用意した。AIモデル・ブラウザはVPSへ導入しない。SQLiteの公開情報と巡回状態を引き継ぎ、取得間隔は最低2秒。検索のページ上限、重複、削除、不正なメタデータを記録する。認証失効時には停止して再認証を待つ。

## 配置

- プログラム：`/opt/world-compass-collector`
- DB・設定・セッション：`/var/lib/world-compass-collector`
- ユーザー：`worldcompass`（管理者権限なし）
- サービス：`world-compass-collector.service`
- 初期移行：公開ワールド53,103件、検索計画18,806件

PCの会話、お気に入り、訪問記録、メモ、画像、翻訳、Windows暗号化セッションは転送しない。SSH鍵はWindowsに残す。VPSでは対話式のVRChatログインを行い、セッションCookieは専用ユーザーのみ読み書きできるファイルに保存する。パスワード・認証コードは保存しない。VPSの管理者はセッションにアクセスできるため、Cookieファイルを共有・配布しない。

## 利用

認証は `artifacts/Login-VpsCollector-Gui.ps1` を開く。通常の入力欄でユーザー名・パスワードを入力し、必要なら認証コードを入力する。入力値はファイルやコマンド引数へ保存せずSSHの標準入力でVPSへ渡す。成功後にサービスが有効化・起動する。失効後も同じ手順で再認証する。対話式ターミナル版は `artifacts/Login-VpsCollector.ps1`。

VPSからPCへの同期は `artifacts/Sync-VpsCollector.cmd` を開く。アプリを終了し、公開DBの一貫したスナップショットをSSHで取得し、ローカルバックアップ後に新しいワールドを追加してアプリを起動する。既存ワールドと既存ベクトル、会話・個人ライブラリは保持する。同期は現状手動であり、アプリの再起動を伴う。

PCに `userdata/remote-collector.json` がある場合、アプリはローカルAPI巡回を行わず、同期済みワールドの意味検索データだけを作成する。セットアップ画面の件数は最後の同期時点の値であり、VPSのリアルタイム件数ではない。画像・翻訳は従来どおりカード表示時に取得する。

VPSの確認コマンド：

```sh
systemctl status world-compass-collector.service --no-pager
cat /var/lib/world-compass-collector/status.json
journalctl -u world-compass-collector.service -n 20 --no-pager
```

サービスを停止するには `systemctl stop world-compass-collector.service`。SQLiteの進捗を保持し、次回起動時に続きを処理する。PCとVPSの収集を同時起動しない。

## 検証

Pythonテスト11件：公開情報だけの書き出し、巡回位置維持、重複・作者・タグの検証、正規化できない情報の除外、認証失効時のキュー維持、APIページ上限の識別、PC同期時の既存データ・個人情報・ベクトル保持を確認した。

完了した計画は自動で全件再取得しない。現在は初回巡回の継続が目的であり、新着発見や定期更新のスケジュールは別途実装する。
