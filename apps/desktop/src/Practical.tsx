import { useEffect, useState } from "react";
import { invoke, native } from "./bridge";
import { CorpusSetup } from "./CorpusSetup";
export type Preferences = {
  model: string;
  backend: string;
  threads: number;
  idleSeconds: number;
  apiContact: string;
  catalogMode: string;
};
export const modelNames: Record<string, string> = {
  "chat-quality": "Qwen3.5 4B · Q4_K_M（日本語品質）",
  "chat-balanced": "Qwen3 1.7B · Q8_0（軽量）",
  chat: "Qwen3 0.6B · Q8_0（省メモリ）",
};
export function Practical({
  changed,
  fail,
  disabled,
}: {
  changed: () => Promise<void>;
  fail: (message: string) => void;
  disabled: boolean;
}) {
  const [prefs, setPrefs] = useState<Preferences | null>(null),
    [url, setUrl] = useState(""),
    [working, setWorking] = useState(false),
    [message, setMessage] = useState("");
  useEffect(() => {
    if (native)
      invoke<Preferences>("get_preferences")
        .then(setPrefs)
        .catch((e) => fail(String(e)));
  }, []);
  const save = async (next: Preferences) => {
    await invoke("save_preferences", { value: next });
    setPrefs(next);
    await changed();
  };
  const act = async (action: () => Promise<void>) => {
    setWorking(true);
    setMessage("");
    try {
      await action();
    } catch (e) {
      fail(String(e));
    } finally {
      setWorking(false);
    }
  };
  if (!prefs)
    return (
      <section className="panel">
        <h2>ローカルモデル・実在ワールド</h2>
        <p>デスクトップ版で設定できます。</p>
      </section>
    );
  const locked = disabled || working;
  return (
    <>
      <CorpusSetup changed={changed} disabled={disabled || working} />
      <section className="panel practical">
        <h2>ローカルモデル</h2>
        <p>
          日本語の会話には4Bを推奨します。モデルの切り替え後、未導入ならセットアップを実行してください。
        </p>
        <label>
          モデル
          <select
            aria-label="ローカルモデル"
            disabled={locked}
            value={prefs.model}
            onChange={(e) =>
              void act(() => save({ ...prefs, model: e.target.value }))
            }
          >
            {Object.entries(modelNames).map(([id, name]) => (
              <option key={id} value={id}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <label>
          実行デバイス
          <select
            aria-label="実行デバイス"
            disabled={locked}
            value={prefs.backend}
            onChange={(e) =>
              void act(() => save({ ...prefs, backend: e.target.value }))
            }
          >
            <option value="vulkan">GPU（Vulkan・起動失敗時はCPU）</option>
            <option value="cpu">CPU（GPUをVRChatに残す）</option>
          </select>
        </label>
        <label>
          CPUスレッド数
          <input
            aria-label="CPUスレッド数"
            type="number"
            min={1}
            max={8}
            disabled={locked}
            value={prefs.threads}
            onChange={(e) => {
              const n = Number(e.target.value);
              if (n >= 1 && n <= 8)
                void act(() => save({ ...prefs, threads: n }));
            }}
          />
        </label>
        <p className="muted">
          {prefs.idleSeconds}
          秒使わないとモデルを解放します。埋め込み検索はCPUで実行します。
        </p>
        <button
          disabled={locked}
          onClick={() =>
            void act(() =>
              save({
                ...prefs,
                model: "chat-balanced",
                backend: "cpu",
                threads: 2,
                idleSeconds: 30,
              }),
            )
          }
        >
          VRChat同時利用向けに切り替え
        </button>
        <button
          disabled={locked}
          onClick={() =>
            void act(() =>
              save({
                ...prefs,
                model: "chat-quality",
                backend: "vulkan",
                threads: 4,
                idleSeconds: 120,
              }),
            )
          }
        >
          日本語品質を優先
        </button>
      </section>
      <section className="panel practical">
        <h2>実在ワールドを登録</h2>
        <p>
          VRChat公式ページのURLを貼ると、公開APIから説明・タグ・定員を取得します。以後はオフラインで検索できます。
        </p>
        <p className="muted">
          上のDB構築でまとめて追加するか、URLを貼って個別に追加できます。雨音・軽さなど、APIで確認できない情報は未確認として表示します。
        </p>
        <label>
          API User-Agentの連絡先
          <input
            aria-label="APIの連絡先"
            value={prefs.apiContact}
            placeholder="開発者メールアドレス / httpsの連絡先ページ"
            disabled={locked}
            onChange={(e) => setPrefs({ ...prefs, apiContact: e.target.value })}
            onBlur={() => void act(() => save(prefs))}
          />
        </label>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void act(async () => {
              await save(prefs);
              const result = await invoke<{
                world: { name: string };
                cached: boolean;
              }>("add_world", { input: url, refresh: false });
              await save({ ...prefs, catalogMode: "personal" });
              setMessage(
                `${result.world.name}を登録しました${result.cached ? "（保存済み情報）" : ""}`,
              );
              setUrl("");
            });
          }}
        >
          <label>
            ワールドURLまたはID
            <input
              aria-label="登録するワールドURL"
              value={url}
              disabled={locked}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://vrchat.com/home/world/wrld_…"
            />
          </label>
          <button
            className="primary"
            disabled={locked || !url.trim() || !prefs.apiContact}
          >
            公開情報を取得して登録
          </button>
        </form>
        <label>
          チャットで使うワールド
          <select
            aria-label="カタログの種類"
            disabled={locked}
            value={prefs.catalogMode}
            onChange={(e) =>
              void act(() => save({ ...prefs, catalogMode: e.target.value }))
            }
          >
            <option value="personal">登録した実在ワールド</option>
            <option value="demo">架空ワールドのデモ</option>
          </select>
        </label>
        {message && <p role="status">{message}</p>}
      </section>
    </>
  );
}
