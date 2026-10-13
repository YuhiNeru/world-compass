import { useEffect, useRef, useState, type ReactNode } from "react";
import { z } from "zod";
import {
  ArrowUp,
  Compass,
  MessageSquare,
  Plus,
  SlidersHorizontal,
  Square,
  Sparkles,
  ChevronDown,
} from "lucide-react";
import {
  ConditionsSchema,
  ReasonsSchema,
  WorldSchema,
  type Conditions,
  type WorldRecord,
} from "../../../../../packages/contracts";
import { labels, rankWorlds } from "../search/engine";
import { invoke, native } from "../../bridge";

export const ReplySchema = z.object({
  conditions: ConditionsSchema,
  scores: z.record(z.string(), z.number()),
  ids: z.array(z.string()).nullable(),
  reasons: ReasonsSchema,
  mode: z.string(),
  answer: z.string().optional(),
  worlds: z.array(WorldSchema).optional(),
  fixture: z.boolean().optional(),
  rankingQuery: z.string().optional(),
});
export type Reply = z.infer<typeof ReplySchema>;
const TurnSchema = z.object({
  id: z.string(),
  text: z.string().max(2000),
  reply: ReplySchema.optional(),
  stopped: z.boolean().optional(),
  error: z.string().optional(),
});
const SessionSchema = z.object({
  id: z.string(),
  title: z.string(),
  turns: z.array(TurnSchema),
  updated: z.number(),
});
type Session = z.infer<typeof SessionSchema>;
const storageKey = "world-compass-chats-v1";
function readSessions(): Session[] {
  try {
    return z
      .array(SessionSchema)
      .parse(JSON.parse(localStorage.getItem(storageKey) ?? "[]"))
      .slice(0, 40);
  } catch {
    return [];
  }
}
export function useChats(
  run: (
    text: string,
    previous?: Conditions,
    history?: { role: string; content: string }[],
    progress?: (reply: Reply) => void,
  ) => Promise<Reply | undefined>,
  cancel: () => void,
) {
  const [sessions, setSessions] = useState(readSessions);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const [hydrated, setHydrated] = useState(!native),
    [storageError, setStorageError] = useState("");
  const writes = useRef(Promise.resolve());
  useEffect(() => {
    if (native)
      invoke<unknown[]>("load_chats")
        .then((raw) => {
          if (raw.length) setSessions(z.array(SessionSchema).parse(raw));
          setHydrated(true);
        })
        .catch((e) => setStorageError(`履歴を読み込めませんでした: ${e}`));
  }, []);
  const active = sessions.find((s) => s.id === activeId);
  const generation = useRef(0),
    lock = useRef(false);
  useEffect(() => {
    if (!hydrated) return;
    if (native) {
      const chats = sessions.slice(0, 40);
      const timer = setTimeout(() => {
        writes.current = writes.current
          .catch(() => {})
          .then(async () => {
            await invoke("save_chats", { chats });
            setStorageError("");
          })
          .catch((e) => setStorageError(`履歴を保存できませんでした: ${e}`));
      }, 250);
      return () => clearTimeout(timer);
    }
    try {
      localStorage.setItem(storageKey, JSON.stringify(sessions.slice(0, 40)));
    } catch {
      /* In-memory conversation remains available if storage is full. */
    }
  }, [sessions, hydrated]);
  const stop = () => {
    generation.current++;
    lock.current = false;
    cancel();
    setPending(null);
    setSessions((old) =>
      old.map((s) => ({
        ...s,
        turns: s.turns.map((t) =>
          t.id === pending ? { ...t, stopped: true } : t,
        ),
      })),
    );
  };
  const select = (id: string | null) => {
    if (lock.current) stop();
    setActiveId(id);
  };
  const send = async (text: string) => {
    text = text.trim();
    if (!text || lock.current || !hydrated) return;
    lock.current = true;
    const token = ++generation.current,
      sessionId = active?.id ?? crypto.randomUUID(),
      turnId = crypto.randomUUID();
    const previous = active?.turns.filter((t) => t.reply).at(-1)
      ?.reply?.conditions;
    const turn = { id: turnId, text };
    setActiveId(sessionId);
    setPending(turnId);
    setSessions((old) =>
      active
        ? old.map((s) =>
            s.id === sessionId
              ? { ...s, turns: [...s.turns, turn], updated: Date.now() }
              : s,
          )
        : [
            {
              id: sessionId,
              title: text.slice(0, 30),
              turns: [turn],
              updated: Date.now(),
            },
            ...old,
          ].slice(0, 40),
    );
    try {
      const history = (active?.turns ?? [])
        .slice(-2)
        .flatMap((t) => [
          { role: "user", content: t.text },
          ...(t.reply?.answer
            ? [{ role: "assistant", content: t.reply.answer.slice(0, 2000) }]
            : []),
        ]);
      const reply = await run(text, previous, history, (reply) => {
        if (token === generation.current)
          setSessions((old) =>
            old.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    turns: s.turns.map((t) =>
                      t.id === turnId ? { ...t, reply } : t,
                    ),
                  }
                : s,
            ),
          );
      });
      if (token !== generation.current) return;
      setSessions((old) =>
        old.map((s) =>
          s.id === sessionId
            ? {
                ...s,
                turns: s.turns.map((t) =>
                  t.id === turnId
                    ? { ...t, ...(reply ? { reply } : { stopped: true }) }
                    : t,
                ),
              }
            : s,
        ),
      );
    } catch {
      if (token === generation.current)
        setSessions((old) =>
          old.map((s) =>
            s.id === sessionId
              ? {
                  ...s,
                  turns: s.turns.map((t) =>
                    t.id === turnId
                      ? {
                          ...t,
                          error:
                            "候補を取得できませんでした。もう一度メッセージを送ってください。",
                        }
                      : t,
                  ),
                }
              : s,
          ),
        );
    } finally {
      if (token === generation.current) {
        lock.current = false;
        setPending(null);
      }
    }
  };
  const remove = () => {
    if (activeId) {
      if (lock.current) stop();
      setSessions((old) => old.filter((s) => s.id !== activeId));
      setActiveId(null);
    }
  };
  return {
    sessions,
    active,
    activeId,
    pending,
    select,
    send,
    stop,
    remove,
    storageError,
  };
}
export type Chats = ReturnType<typeof useChats>;
export function ChatHistory({ chat, open }: { chat: Chats; open: () => void }) {
  return (
    <>
      <button
        className="new-chat"
        onClick={() => {
          chat.select(null);
          open();
        }}
      >
        <Plus size={17} />
        新しいチャット
      </button>
      <div className="history-label">最近のチャット</div>
      {chat.activeId && (
        <button className="new-chat" onClick={chat.remove}>
          このチャットを削除
        </button>
      )}
      <div className="chat-history">
        {chat.sessions.length ? (
          chat.sessions.map((s) => (
            <button
              key={s.id}
              className={chat.activeId === s.id ? "active" : ""}
              onClick={() => {
                chat.select(s.id);
                open();
              }}
            >
              <MessageSquare size={15} />
              <span>{s.title}</span>
            </button>
          ))
        ) : (
          <p>会話はここに保存されます</p>
        )}
      </div>
    </>
  );
}
const starters = [
  {
    icon: "☂",
    title: "雨音を聞きながら",
    text: "雨の夜に2人でゆっくりできる場所",
  },
  {
    icon: "◈",
    title: "自然の中で写真を",
    text: "Questで行ける自然の撮影スポット",
  },
  { icon: "☾", title: "静かにひと休み", text: "ゲームなしで静かに過ごしたい" },
];
export function ChatPanel({
  chat,
  worlds,
  personal,
  busy,
  ready,
  mode,
  renderCard,
  openSetup,
  fixture,
}: {
  chat: Chats;
  worlds: WorldRecord[];
  personal: Parameters<typeof rankWorlds>[3];
  busy: string;
  ready: boolean;
  mode: string;
  renderCard: (world: WorldRecord, unknown: boolean, reply: Reply) => ReactNode;
  openSetup: () => void;
  fixture: boolean;
}) {
  const [draft, setDraft] = useState(""),
    [filters, setFilters] = useState(false),
    [platform, setPlatform] = useState(""),
    [people, setPeople] = useState("");
  const scroll = useRef<HTMLDivElement>(null),
    input = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    setDraft("");
    setFilters(false);
    setPlatform("");
    setPeople("");
    input.current?.focus();
  }, [chat.activeId]);
  useEffect(() => {
    const container = scroll.current;
    const last = container?.querySelector(".chat-turn:last-child");
    if (container && last)
      container.scrollTo({
        top:
          container.scrollTop +
          last.getBoundingClientRect().top -
          container.getBoundingClientRect().top -
          20,
        behavior: "instant",
      });
  }, [chat.activeId, chat.active?.turns.length, chat.pending]);
  const send = (text = draft) => {
    if (!text.trim() || chat.pending || !ready) return;
    const full = [
      platform === "quest" ? "Quest必須" : platform === "pc" ? "PC必須" : "",
      people ? `${people}人で行きたい` : "",
      text,
    ]
      .filter(Boolean)
      .join("。 ");
    setDraft("");
    void chat.send(full);
  };
  return (
    <section
      className={`chat-panel ${chat.active?.turns.length ? "has-messages" : ""}`}
    >
      <div className="chat-scroll" ref={scroll}>
        {!chat.active?.turns.length ? (
          <div className="chat-welcome">
            <div className="welcome-mark">
              <Compass size={30} />
            </div>
            <div className="welcome-kicker">WORLD COMPASS · AI</div>
            <h1>今日は、どこへ行きましょう？</h1>
            <p>気分を話すだけで、あなたに合うワールドを一緒に探します。</p>
            <div className="starter-grid">
              {starters.map((s) => (
                <button
                  key={s.title}
                  disabled={!ready}
                  onClick={() => send(s.text)}
                >
                  <span className="starter-icon">{s.icon}</span>
                  <strong>{s.title}</strong>
                  <span>{s.text}</span>
                  <ArrowUp size={15} />
                </button>
              ))}
            </div>
            <span className="welcome-note">まずは、こんな話しかけ方から</span>
          </div>
        ) : (
          <div className="messages" role="log" aria-label="会話履歴">
            {chat.active.turns.map((turn) => {
              const ranked = turn.reply
                ? rankWorlds(
                    (turn.reply.worlds ?? worlds).filter(
                      (w) =>
                        turn.reply!.ids === null ||
                        turn.reply!.ids.includes(w.worldId),
                    ),
                    turn.reply.conditions,
                    turn.reply.rankingQuery ?? turn.text,
                    personal,
                    turn.reply.scores,
                  )
                : [];
              const confirmed = ranked.filter((r) => !r.unknown.length),
                unknown = ranked.filter((r) => r.unknown.length);
              return (
                <div className="chat-turn" key={turn.id}>
                  <div className="user-message">
                    <span className="sr-only">あなた：</span>
                    {turn.text}
                  </div>
                  <div className="assistant-message">
                    <div className="assistant-avatar">
                      <Compass size={18} />
                    </div>
                    <div className="assistant-body">
                      <div className="assistant-label">
                        World Compass <span>AI</span>
                      </div>
                      {turn.reply ? (
                        <>
                          <p className="reply-intro">
                            {turn.reply.answer ??
                              (confirmed.length
                                ? `ご希望に合わせて、${confirmed.length}件の候補が見つかりました。まずはこちらの${Math.min(3, confirmed.length)}件はいかがでしょう。`
                                : "確認済みの候補が見つかりませんでした。条件を少し変えて、一緒に探してみましょう。")}
                          </p>
                          {chat.pending === turn.id && (
                            <div className="thinking" role="status">
                              {busy}
                            </div>
                          )}
                          {turn.stopped && (
                            <p className="muted">応答を停止しました。</p>
                          )}
                          <div className="condition-chips">
                            {turn.reply.conditions.people && (
                              <span>{turn.reply.conditions.people}人</span>
                            )}
                            {turn.reply.conditions.platform && (
                              <span>
                                {turn.reply.conditions.platform.toUpperCase()}
                                対応
                              </span>
                            )}
                            {turn.reply.conditions.preferred.map((k) => (
                              <span key={k}>{labels[k]}</span>
                            ))}
                            {turn.reply.conditions.required.map((k) => (
                              <span key={k}>{labels[k]}必須</span>
                            ))}
                            {turn.reply.conditions.excluded.map((k) => (
                              <span key={k}>{labels[k]}なし</span>
                            ))}
                          </div>
                          <div className="world-grid">
                            {confirmed
                              .slice(0, 3)
                              .map((r) =>
                                renderCard(r.world, false, turn.reply!),
                              )}
                          </div>
                          {confirmed.length > 3 && (
                            <details className="more-worlds">
                              <summary>
                                ほかの候補を表示（{confirmed.length - 3}件）
                                <ChevronDown size={14} />
                              </summary>
                              <div className="world-grid">
                                {confirmed
                                  .slice(3)
                                  .map((r) =>
                                    renderCard(r.world, false, turn.reply!),
                                  )}
                              </div>
                            </details>
                          )}
                          {unknown.length > 0 && (
                            <details className="more-worlds">
                              <summary>
                                必須条件が未確認の候補（{unknown.length}件）
                                <ChevronDown size={14} />
                              </summary>
                              <p>
                                こちらは必須条件を満たすか確認できていません。
                              </p>
                              <div className="world-grid">
                                {unknown.map((r) =>
                                  renderCard(r.world, true, turn.reply!),
                                )}
                              </div>
                            </details>
                          )}
                          <div className="reply-meta">
                            {turn.reply.mode} ·{" "}
                            {(turn.reply.fixture ?? fixture)
                              ? "架空カタログによる提案"
                              : "登録した公開ワールドの情報"}
                          </div>
                          <div className="followups">
                            {[
                              "Quest対応にして",
                              "4人で行きたい",
                              "ゲームなしで探して",
                            ].map((q) => (
                              <button
                                key={q}
                                disabled={!!chat.pending}
                                onClick={() => send(q)}
                              >
                                {q}
                              </button>
                            ))}
                          </div>
                        </>
                      ) : turn.stopped ? (
                        <p className="muted">
                          応答を停止しました。続けて話しかけられます。
                        </p>
                      ) : turn.error ? (
                        <p role="alert">{turn.error}</p>
                      ) : (
                        <div className="thinking" role="status">
                          <Sparkles size={16} />
                          {busy || "候補を探しています"}
                          <span className="thinking-dots">•••</span>
                        </div>
                      )}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
      <div className="composer-dock">
        {chat.storageError && <p role="alert">{chat.storageError}</p>}
        <div className="composer-wrap">
          <form
            className="chat-composer"
            onSubmit={(e) => {
              e.preventDefault();
              send();
            }}
          >
            <textarea
              ref={input}
              rows={2}
              aria-label="メッセージ"
              placeholder={
                chat.active
                  ? "条件を追加したり、別の気分を伝えてください…"
                  : "World Compass にメッセージを送る…"
              }
              maxLength={1900}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (
                  e.key === "Enter" &&
                  !e.shiftKey &&
                  !e.nativeEvent.isComposing
                ) {
                  e.preventDefault();
                  send();
                }
              }}
            />
            <div className="composer-toolbar">
              <button
                type="button"
                className={`filter-toggle ${filters ? "selected" : ""}`}
                aria-expanded={filters}
                onClick={() => setFilters(!filters)}
              >
                <SlidersHorizontal size={15} />
                条件
              </button>
              <button
                type="button"
                className="model-selector"
                onClick={openSetup}
              >
                <span className="status-dot" />
                {mode}
                <ChevronDown size={12} />
              </button>
              {chat.pending ? (
                <button
                  type="button"
                  className="send-button"
                  aria-label="応答を停止"
                  onClick={chat.stop}
                >
                  <Square size={15} fill="currentColor" />
                </button>
              ) : (
                <button
                  className="send-button"
                  type="submit"
                  aria-label="メッセージを送信"
                  disabled={!draft.trim() || !ready}
                >
                  <ArrowUp size={19} />
                </button>
              )}
            </div>
            {filters && (
              <div className="composer-filters">
                <label>
                  対応機種
                  <select
                    aria-label="必須の対応機種"
                    value={platform}
                    onChange={(e) => setPlatform(e.target.value)}
                  >
                    <option value="">会話に合わせる</option>
                    <option value="pc">PC 必須</option>
                    <option value="quest">Quest 必須</option>
                  </select>
                </label>
                <label>
                  人数
                  <input
                    aria-label="必要な人数"
                    type="number"
                    min={1}
                    max={100}
                    value={people}
                    onChange={(e) => setPeople(e.target.value)}
                  />
                </label>
                <span>次のメッセージに適用</span>
              </div>
            )}
          </form>
          <p className="composer-note">
            {fixture
              ? "デモ：架空ワールドを提案します。"
              : "登録ワールド内を検索。未確認の条件は明示します。"}
            <span>Enterで送信 · Shift + Enterで改行</span>
          </p>
        </div>
      </div>
    </section>
  );
}
