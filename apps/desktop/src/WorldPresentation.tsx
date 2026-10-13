import { useEffect, useRef, useState, type ReactNode } from "react";
import { ImageOff, LoaderCircle } from "lucide-react";
import type { WorldRecord } from "../../../packages/contracts";
import { invoke, invokeInference, native } from "./bridge";
export function needsTranslation(text: string) {
  const latin = (text.match(/[a-z]/gi) ?? []).length;
  const japanese = (text.match(/[\u3040-\u30ff\u4e00-\u9fff]/g) ?? []).length;
  return latin >= 12 && japanese < latin * 0.4;
}
export function WorldImage({
  world,
  children,
}: {
  world: WorldRecord;
  children: ReactNode;
}) {
  const [src, setSrc] = useState(""),
    [error, setError] = useState(""),
    [retry, setRetry] = useState(0),
    [visible, setVisible] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const fixture = world.worldId.startsWith("fixture-");
  useEffect(() => {
    const target = ref.current;
    if (!target || fixture || !native) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setVisible(true);
          observer.disconnect();
        }
      },
      { rootMargin: "200px" },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [world.worldId]);
  useEffect(() => {
    if (fixture || !native || !visible) return;
    let active = true,
      objectUrl = "";
    setError("");
    invoke<ArrayBuffer | number[]>("world_image", { id: world.worldId })
      .then((bytes) => {
        if (!active) return;
        const data = Array.isArray(bytes)
          ? new Uint8Array(bytes).buffer
          : bytes;
        if (!(data instanceof ArrayBuffer))
          throw Error("画像データの形式が不正です");
        objectUrl = URL.createObjectURL(new Blob([data]));
        setSrc(objectUrl);
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [world.worldId, visible, retry]);
  return (
    <div
      ref={ref}
      className={`world-art art-${world.worldId.replace("fixture-", "")} ${!fixture ? "public-world-art" : ""} ${src ? "has-image" : ""}`}
    >
      {src ? (
        <img
          className="world-thumbnail"
          src={src}
          alt={`${world.name}の公開画像`}
          onError={() => {
            setSrc("");
            setError("画像を表示できませんでした");
          }}
        />
      ) : fixture ? (
        <>
          <span className="art-label">架空ワールド / イメージ</span>
          <div className="art-orbit" />
          <span className="art-coordinates">WORLD COMPASS</span>
        </>
      ) : (
        <div className="image-placeholder">
          {error ? (
            <ImageOff size={22} />
          ) : (
            <LoaderCircle className="spin" size={22} />
          )}
          <span>
            {error ? "画像を取得できませんでした" : "画像を読み込んでいます"}
          </span>
          {error && (
            <button
              className="image-retry"
              title={error}
              onClick={() => setRetry((r) => r + 1)}
            >
              再試行
            </button>
          )}
        </div>
      )}
      {children}
    </div>
  );
}
export function WorldDescription({
  world,
  enabled,
}: {
  world: WorldRecord;
  enabled: boolean;
}) {
  const original = world.description ?? "";
  const foreign =
    needsTranslation(original) && !world.worldId.startsWith("fixture-");
  const [text, setText] = useState(""),
    [error, setError] = useState(""),
    [retry, setRetry] = useState(0),
    [visible, setVisible] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const target = ref.current;
    if (!target || !native || !foreign) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setVisible(true);
          observer.disconnect();
        }
      },
      { rootMargin: "200px" },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [world.worldId, original, foreign]);
  useEffect(() => {
    if (!native || !foreign || !enabled || !visible || text) return;
    let active = true;
    setError("");
    invokeInference<{ text: string; cached: boolean }>(
      "translate_description",
      { id: world.worldId, original },
      () => !active,
    )
      .then((result) => {
        if (active) setText(result.text);
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, [world.worldId, original, foreign, enabled, visible, retry, text]);
  useEffect(() => {
    setText("");
    setError("");
  }, [world.worldId, original]);
  if (!foreign || !native) return <p className="description">{original}</p>;
  return (
    <div className="translated-description" ref={ref}>
      <span className="translation-label">
        {text
          ? "日本語訳 · ローカルAI"
          : error
            ? "日本語訳を取得できませんでした"
            : enabled
              ? "日本語に翻訳中…"
              : "日本語訳を準備しています…"}
      </span>
      {text && <p className="description">{text}</p>}
      <details className="original-description" open={!!error}>
        <summary>原文（作者の説明）</summary>
        <p className="description">{original}</p>
      </details>
      {error && (
        <button
          title={error}
          disabled={!enabled}
          onClick={() => setRetry((r) => r + 1)}
        >
          翻訳を再試行
        </button>
      )}
    </div>
  );
}
