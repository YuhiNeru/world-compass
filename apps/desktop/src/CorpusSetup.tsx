import { useEffect, useRef, useState } from "react";
import { Database, LoaderCircle, LockKeyhole } from "lucide-react";
import { invoke, native } from "./bridge";
type Auth = {
  loggedIn: boolean;
  saved: boolean;
  displayName: string;
  twoFactorMethods: string[];
};
type Progress = {
  running: boolean;
  stage: string;
  completed: number;
  indexed: number;
  queued: number;
  skipped: number;
  target: number;
  message: string;
  bytes: number;
  expanded?: boolean;
  remote?: boolean;
  category?: string;
  queriesDone?: number;
  queriesTotal?: number;
  limitedQueries?: number;
};
export function CorpusSetup({
  changed,
  disabled,
}: {
  changed: () => Promise<void>;
  disabled: boolean;
}) {
  const [auth, setAuth] = useState<Auth | null>(null),
    [progress, setProgress] = useState<Progress | null>(null);
  const [username, setUsername] = useState(""),
    [password, setPassword] = useState(""),
    [code, setCode] = useState(""),
    [method, setMethod] = useState("totp"),
    [working, setWorking] = useState(false),
    [error, setError] = useState("");
  const lastRunning = useRef(false);
  const expansionRequested = useRef(false);
  useEffect(() => {
    if (!native) return;
    let active = true;
    invoke<Auth>("vrchat_auth_status")
      .then((a) => {
        if (active) setAuth(a);
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    const poll = async () => {
      try {
        const p = await invoke<Progress>("corpus_status");
        if (!active) return;
        setProgress(p);
        if (p.running) setAuth(await invoke<Auth>("vrchat_auth_status"));
        if (lastRunning.current && !p.running) await changed();
        lastRunning.current = p.running;
      } catch (e) {
        if (active) setError(String(e));
      }
    };
    void poll();
    const timer = setInterval(() => void poll(), 1500);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);
  const act = async (action: () => Promise<void>) => {
    setWorking(true);
    setError("");
    try {
      await action();
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(false);
    }
  };
  const begin = async (expand = false) => {
    await invoke(expand ? "corpus_expand" : "corpus_start");
    lastRunning.current = true;
    setProgress(await invoke<Progress>("corpus_status"));
    await changed();
  };
  const accepted = async (a: Auth) => {
    setAuth(a);
    setCode("");
    if (a.loggedIn) await begin(expansionRequested.current);
    else if (a.twoFactorMethods.includes("totp")) setMethod("totp");
    else if (a.twoFactorMethods.includes("emailOtp")) setMethod("emailOtp");
  };
  if (!native) return null;
  const locked = working || disabled || !!progress?.running;
  const pending = !!auth?.twoFactorMethods.length;
  return (
    <section className="panel practical corpus-setup">
      <h2>
        <Database size={20} /> ワールド検索DBを作る
      </h2>
      <p>
        {progress?.remote
          ? "VPSが公開ワールドの情報を収集します。このPCへ同期した説明・タグ・対応機種を使って検索できます。作者の国籍は未確認です。"
          : "公開ワールドの説明・タグ・対応機種をこのPCに保存します。最初の1,000件から、日本語向けワールドのカテゴリー別収集へ拡張できます。作者の国籍は未確認です。"}
      </p>
      <p className="muted">
        <LockKeyhole size={13} />{" "}
        {progress?.remote
          ? "VPSのVRChat認証はSSHで行います。画像と日本語訳はこのPCでカード表示時に取得・保存します。"
          : "パスワードは保存しません。セッションはWindowsで暗号化して保存します。画像と日本語訳はカード表示時に取得・保存します。"}
      </p>
      {!progress?.remote && !auth?.loggedIn && !pending && !auth?.saved && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            const enteredPassword = password;
            setPassword("");
            void act(async () =>
              accepted(
                await invoke<Auth>("vrchat_login", {
                  username,
                  password: enteredPassword,
                }),
              ),
            );
          }}
        >
          <label>
            VRChatユーザー名（ログイン用）
            <input
              aria-label="VRChatユーザー名"
              autoComplete="username"
              placeholder="表示名ではなく、ログイン用ユーザー名"
              value={username}
              disabled={locked}
              onChange={(e) => setUsername(e.target.value)}
            />
          </label>
          <label>
            VRChatパスワード
            <input
              aria-label="VRChatパスワード"
              type="password"
              autoComplete="off"
              value={password}
              disabled={locked}
              onChange={(e) => setPassword(e.target.value)}
            />
          </label>
          <button
            className="primary"
            disabled={locked || !username || !password}
          >
            ログインしてDB構築
          </button>
          <p className="muted">
            VRChatアカウントでログインしてください。Steamアカウントのパスワードは使えません。
          </p>
        </form>
      )}
      {!progress?.remote && pending && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            const enteredCode = code;
            setCode("");
            void act(async () =>
              accepted(
                await invoke<Auth>("vrchat_verify", {
                  method,
                  code: enteredCode,
                }),
              ),
            );
          }}
        >
          <label>
            二段階認証
            <select
              aria-label="VRChat認証方法"
              value={method}
              disabled={locked}
              onChange={(e) => setMethod(e.target.value)}
            >
              {auth?.twoFactorMethods
                .filter((m) => ["totp", "emailOtp"].includes(m))
                .map((m) => (
                  <option key={m} value={m}>
                    {m === "totp"
                      ? "認証アプリのコード"
                      : "メールで届いたコード"}
                  </option>
                ))}
            </select>
          </label>
          <label>
            認証コード
            <input
              aria-label="VRChat認証コード"
              autoComplete="one-time-code"
              inputMode="numeric"
              value={code}
              disabled={locked}
              onChange={(e) =>
                setCode(e.target.value.replace(/\D/g, "").slice(0, 8))
              }
            />
          </label>
          <button className="primary" disabled={locked || code.length < 6}>
            認証してDB構築
          </button>
        </form>
      )}
      {!progress?.remote && auth?.saved && !auth.loggedIn && !pending && (
        <button
          className="primary"
          disabled={locked}
          onClick={() =>
            void act(async () => accepted(await invoke<Auth>("vrchat_restore")))
          }
        >
          保存済みログインでDB構築
        </button>
      )}
      {!progress?.remote && auth?.loggedIn && (
        <p className="login-status">{auth.displayName}でログイン済み</p>
      )}
      {progress && (
        <div className="corpus-progress" aria-live="polite">
          <p>
            {progress.running && <LoaderCircle size={15} className="spin" />}{" "}
            {progress.message}
          </p>
          {!progress.expanded && (
            <progress
              aria-label="ワールドDB構築進捗"
              max={progress.target * 2}
              value={progress.completed + progress.indexed}
            />
          )}
          <p className="muted">
            公開情報 {progress.completed.toLocaleString()}
            {!progress.expanded && ` / ${progress.target.toLocaleString()}`}件 ·
            意味検索 {progress.indexed.toLocaleString()} /{" "}
            {(progress.expanded
              ? progress.completed
              : progress.target
            ).toLocaleString()}
            件 · DB保存量 {(progress.bytes / 1024 / 1024).toFixed(1)}MB
          </p>
          {progress.expanded && (
            <>
              <p className="muted">
                {progress.category || "カテゴリーの準備中"} · 候補待ち{" "}
                {progress.queued.toLocaleString()}件 · 検索巡回{" "}
                {progress.queriesDone ?? 0} / {progress.queriesTotal ?? 0}
              </p>
              <p className="muted">
                {progress.remote
                  ? "ワールド収集はVPSで実行しています。このPCでは同期済みデータの意味検索を準備します。表示件数は最後に同期した時点のものです。"
                  : "総件数は未確定です。アプリを開いている間に収集し、次回起動時にも再開します。「中断して保存」で自動再開を止められます。"}
                {(progress.limitedQueries ?? 0) > 0 &&
                  ` APIのページ制限等で巡回できなかった検索：${progress.limitedQueries}件。`}
              </p>
            </>
          )}
          {progress.running ? (
            <button
              onClick={() =>
                void act(async () => {
                  await invoke("corpus_pause");
                })
              }
              disabled={working}
            >
              {progress.remote ? "意味検索の準備を中断" : "中断して保存"}
            </button>
          ) : (progress.remote || auth?.loggedIn) && progress.stage !== "complete" ? (
            <button
              className="primary"
              disabled={locked}
              onClick={() => void act(() => begin())}
            >
              {progress.remote ? "意味検索の準備を再開" : progress.completed ? "続きからDB構築" : "1,000件のDB構築を開始"}
            </button>
          ) : null}
          {!progress.running &&
            !progress.remote &&
            !progress.expanded &&
            (auth?.saved || auth?.loggedIn) &&
            !pending && (
              <button
                className="primary"
                disabled={locked}
                onClick={() =>
                  void act(async () => {
                    expansionRequested.current = true;
                    if (auth?.loggedIn) await begin(true);
                    else await accepted(await invoke<Auth>("vrchat_restore"));
                  })
                }
              >
                日本語ワールドの収集を拡張
              </button>
            )}
          {progress.stage === "complete" && (
            <p>
              チャットで「ちるわのおすすめ」「自然がきれいなワールド」などを検索できます。
            </p>
          )}
        </div>
      )}
      {working && <p role="status">VRChatに接続しています…</p>}
      {error && (
        <p role="alert" className="corpus-error">
          {error}
        </p>
      )}
      {(auth?.saved || pending || auth?.loggedIn) && (
        <button
          disabled={locked}
          onClick={() =>
            void act(async () => {
              await invoke("vrchat_forget");
              setAuth(await invoke<Auth>("vrchat_auth_status"));
            })
          }
        >
          このアプリのログイン情報を消去
        </button>
      )}
    </section>
  );
}
