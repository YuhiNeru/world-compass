import { useEffect, useRef, useState } from "react";
import { version } from "../package.json";
import {
  Compass,
  Search,
  Heart,
  BookOpen,
  Settings,
  Download,
  ArrowUpRight,
  Check,
  ShieldCheck,
  SlidersHorizontal,
  X,
  MapPin,
  Clock,
  LoaderCircle,
  Monitor,
  Leaf,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import {
  CatalogSchema,
  ConditionsSchema,
  emptyPersonal,
  type Catalog,
  type Conditions,
  type Personal,
  type Reasons,
  type WorldRecord,
} from "../../../packages/contracts";
import {
  invoke,
  invokeInference,
  loadData,
  native,
  savePersonal,
} from "./bridge";
import { WorldImage, WorldDescription } from "./WorldPresentation";
import {
  parseConditions,
  groundConditions,
  rankWorlds,
  labels,
  validateReasons,
  evidenceText,
  officialUrl,
} from "./features/search/engine";
import {
  ChatPanel,
  ChatHistory,
  useChats,
  type Reply,
} from "./features/chat/Chat";
import { conversationConditions } from "./features/chat/context";
import { Practical, modelNames, type Preferences } from "./Practical";
import apacheLicense from "../../../resources/legal/Qwen3-LICENSE.txt?raw";
import runtimeLicense from "../../../resources/legal/llama-LICENSE.txt?raw";
type Page = "search" | "library" | "setup" | "settings";
type Setup = {
  root: string;
  ram: number;
  free: number;
  gpu: string;
  ready: boolean;
  busy: boolean;
  manifest: {
    version: string;
    assets: {
      id: string;
      version: string;
      size: number;
      license: string;
      licenseUrl: string;
    }[];
  };
};
const examples = [
  "雨の夜に2人でゆっくりできる場所",
  "Questで行ける自然の撮影スポット",
  "ゲームなしで静かに過ごしたい",
];
const mb = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(0)} MB`;
export function App() {
  const [page, setPage] = useState<Page>("search"),
    [catalog, setCatalog] = useState<Catalog | null>(null),
    [personal, setPersonal] = useState<Record<string, Personal>>({});
  const [query, setQuery] = useState("雨の夜に2人でゆっくりできる場所"),
    [conditions, setConditions] = useState<Conditions>(
      parseConditions("雨の夜に2人でゆっくりできる場所"),
    );
  const [error, setError] = useState(""),
    [busy, setBusy] = useState(""),
    [notice, setNotice] = useState("通常検索"),
    [scores, setScores] = useState<Record<string, number>>({}),
    [reasons, setReasons] = useState<Reasons>({ items: [] }),
    [candidateIds, setCandidateIds] = useState<string[] | null>(null),
    [manualConditions, setManualConditions] = useState<Partial<Conditions>>({});
  const [ai, setAi] = useState(
      () => localStorage.getItem("world-compass-ai") !== "false",
    ),
    [setup, setSetup] = useState<Setup | null>(null),
    [aiReasons, setAiReasons] = useState(
      () => localStorage.getItem("world-compass-ai-reasons") === "true",
    ),
    [runtime, setRuntime] = useState({ running: false, memory: 0 }),
    [installing, setInstalling] = useState(false),
    [progress, setProgress] = useState({ id: "", done: 0, total: 1 }),
    [libraryFilter, setLibraryFilter] = useState("favorite");
  const request = useRef(0),
    file = useRef<HTMLInputElement>(null),
    personalRef = useRef<Record<string, Personal>>({}),
    writes = useRef<Promise<void>>(Promise.resolve());
  const reload = async () => {
    await writes.current;
    const d = await loadData();
    setCatalog(CatalogSchema.parse(d.catalog));
    personalRef.current = d.personal;
    setPersonal(d.personal);
    setCandidateIds(null);
    setScores({});
    setReasons({ items: [] });
  };
  const refreshSetup = async () => {
    if (native) setSetup(await invoke<Setup>("setup_status"));
  };
  useEffect(() => {
    if (!native || (!busy && page !== "settings")) return;
    const update = () =>
      invoke<{ running: boolean; memory: number }>("runtime_status")
        .then(setRuntime)
        .catch(() => {});
    update();
    const timer = setInterval(update, 2000);
    return () => clearInterval(timer);
  }, [busy, page]);
  useEffect(() => {
    reload().catch((e) => setError(String(e)));
    refreshSetup().catch((e) => setError(String(e)));
    let active = true,
      dispose: () => void = () => {};
    if (native) {
      invoke<{ stage: string }>("corpus_status")
        .then((status) => {
          if (active && ["ready", "paused"].includes(status.stage))
            setPage("setup");
        })
        .catch(() => {});
      listen<typeof progress>("setup-progress", (e) =>
        setProgress(e.payload),
      ).then((fn) => {
        if (active) dispose = fn;
        else fn();
      });
    }
    return () => {
      active = false;
      dispose();
    };
  }, []);
  useEffect(() => {
    if (!native) return;
    let active = true,
      lastCount = -1,
      polling = false;
    const update = async () => {
      if (polling) return;
      polling = true;
      try {
        const status = await invoke<{ completed: number }>("corpus_status");
        if (active && lastCount >= 0 && status.completed !== lastCount)
          await reload();
        lastCount = status.completed;
      } catch {
        /* The saved catalog remains usable during a transient status error. */
      } finally {
        polling = false;
      }
    };
    void update();
    const timer = setInterval(() => void update(), 5000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);
  const update = (id: string, patch: Partial<Personal>) => {
    const before = personalRef.current[id] ?? emptyPersonal,
      next = { ...before, ...patch };
    personalRef.current = { ...personalRef.current, [id]: next };
    setPersonal(personalRef.current);
    writes.current = writes.current
      .then(async () => {
        await savePersonal(id, next, personalRef.current);
      })
      .catch((e) => {
        if (personalRef.current[id] === next) {
          personalRef.current = { ...personalRef.current, [id]: before };
          setPersonal(personalRef.current);
        }
        setError(`保存できませんでした: ${e}`);
      });
  };
  const search = async (
    text: string,
    previous?: Conditions,
    history: { role: string; content: string }[] = [],
    progress?: (reply: Reply) => void,
  ): Promise<Reply | undefined> => {
    const n = ++request.current;
    const model = <T,>(command: string, args: Record<string, unknown>) =>
      invokeInference<T>(command, args, () => n !== request.current);
    setQuery(text);
    setError("");
    let c = conversationConditions(text, previous),
      ids: string[] | null = null,
      weights: Record<string, number> = {},
      evidence: Reasons = { items: [] },
      mode = "カタログ検索";
    let answer: string | undefined,
      intent = "search";
    const listing = /登録.*(紹介|一覧|見せ|教え)|一覧を/.test(text);
    const searchRequest =
      listing ||
      /おすすめ|お勧め|オススメ|ワールド.*(?:探|教|紹介|見せ)|(?:探|教|紹介).*ワールド/.test(
        text,
      );
    const rankingQuery = listing ? "" : text;
    setConditions(c);
    setBusy("候補を探しています");
    let frozen: Reply | undefined,
      frozenConditions: Conditions | undefined,
      frozenIds: string[] | null | undefined,
      frozenWeights: Record<string, number> | undefined;
    const snapshot = (): Reply => {
      if (
        !frozen ||
        frozenConditions !== c ||
        frozenIds !== ids ||
        frozenWeights !== weights
      ) {
        const ranked = rankWorlds(
          (catalog?.worlds ?? []).filter(
            (w) => ids === null || ids.includes(w.worldId),
          ),
          c,
          rankingQuery,
          personal,
          weights,
        );
        const selected = [
          ...ranked.filter((r) => !r.unknown.length).slice(0, 9),
          ...ranked.filter((r) => r.unknown.length).slice(0, 3),
        ].map((r) => r.world);
        frozen = {
          conditions: c,
          ids: selected.map((w) => w.worldId),
          scores: Object.fromEntries(
            selected.map((w) => [w.worldId, weights[w.worldId] ?? 0]),
          ),
          reasons: evidence,
          mode,
          worlds: selected,
          fixture: catalog?.fixture ?? true,
          rankingQuery,
        };
        frozenConditions = c;
        frozenIds = ids;
        frozenWeights = weights;
      }
      return { ...frozen, reasons: evidence, mode, answer };
    };
    try {
      if (
        native &&
        /^(https:\/\/(?:www\.)?vrchat\.com\/|https:\/\/vrch\.at\/|wrld_)/.test(
          text.trim(),
        )
      ) {
        setBusy("ワールドの公開情報を取得しています");
        const result = await invoke<{ world: WorldRecord; cached: boolean }>(
          "add_world",
          { input: text.trim(), refresh: false },
        );
        const prefs = await invoke<Preferences>("get_preferences");
        await invoke("save_preferences", {
          value: { ...prefs, catalogMode: "personal" },
        });
        await reload();
        const data = await loadData();
        const registered = CatalogSchema.parse(data.catalog);
        return {
          conditions: parseConditions(""),
          ids: [result.world.worldId],
          scores: {},
          reasons: { items: [] },
          mode: "公開APIから登録",
          rankingQuery: "",
          answer: `${result.world.name}を登録しました。説明・タグ・定員を使って、このPCの中で検索できます。対応機種や雨音・軽さなど、取得できない情報は未確認です。`,
          worlds: [result.world],
          fixture: false,
        };
      }
      if (native) {
        try {
          const sql = await invoke<{
            ids: string[];
            weights: Record<string, number>;
          }>("search_candidates", { query: text, conditions: c });
          if (n !== request.current) return;
          ids = sql.ids;
          weights = sql.weights;
        } catch {
          mode = "カタログ検索";
        }
      }
      if (ai && native && setup?.ready) {
        try {
          setBusy("条件を読み取っています");
          const plan = await model<{
            intent: string;
            conditions: Conditions;
            semanticQuery: string;
          }>("ai_plan", { query: text, previous: previous ?? null });
          if (n !== request.current) return;
          intent = searchRequest ? "search" : plan.intent;
          const parsed = groundConditions(text, plan.conditions);
          c = conversationConditions(text, previous, parsed);
          const sql = await invoke<{
            ids: string[];
            weights: Record<string, number>;
          }>("search_candidates", { query: text, conditions: c });
          if (n !== request.current) return;
          ids = sql.ids;
          setBusy("ワールドを比較しています");
          const semanticQuery = plan.semanticQuery || text;
          const vectors = await model<Record<string, number>>(
            "semantic_scores",
            { query: semanticQuery },
          );
          if (n !== request.current) return;
          weights = { ...vectors };
          for (const [id, weight] of Object.entries(sql.weights))
            weights[id] = (weights[id] ?? 0) + weight;
          const candidates = rankWorlds(
            (catalog?.worlds ?? []).filter((w) => sql.ids.includes(w.worldId)),
            c,
            rankingQuery,
            personal,
            weights,
          )
            .filter((r) => !r.unknown.length)
            .slice(0, 3)
            .map((r) => r.world);
          if (candidates.length && aiReasons) {
            setBusy("提案の根拠を確認しています");
            evidence = validateReasons(
              await model("ai_reasons", {
                ids: candidates.map((w) => w.worldId),
                conditions: c,
              }),
              candidates,
              c,
            );
            if (n !== request.current) return;
          }
          mode = "ローカルAI + 通常検索";
          if (intent === "chat") ids = [];
          const shown =
            intent === "chat"
              ? []
              : rankWorlds(
                  (catalog?.worlds ?? []).filter((w) =>
                    ids?.includes(w.worldId),
                  ),
                  c,
                  rankingQuery,
                  personal,
                  weights,
                )
                  .slice(0, 6)
                  .map((r) => r.world.worldId);
          const requestId = crypto.randomUUID();
          const unsubscribe = await listen<{ requestId: string; text: string }>(
            "chat-token",
            (event) => {
              if (
                n === request.current &&
                event.payload.requestId === requestId
              ) {
                answer = event.payload.text;
                progress?.(snapshot());
              }
            },
          );
          try {
            setBusy("返答を生成しています");
            answer = await model<string>("ai_answer", {
              requestId,
              query: text,
              ids: shown,
              conditions: c,
              history: history.slice(-4),
              intent,
            });
          } catch (e) {
            if (n !== request.current) return;
            setError(`AIの返答を生成できませんでした: ${e}`);
          } finally {
            unsubscribe();
          }
        } catch (e) {
          if (n !== request.current) return;
          setError(`ローカルAIを利用できませんでした: ${e}`);
          c = conversationConditions(text, previous);
          ids = null;
          weights = {};
          evidence = { items: [] };
          mode = "AIを利用できないためカタログ検索";
        }
      }
      if (n !== request.current) return;
      setConditions(c);
      setCandidateIds(ids);
      setScores(weights);
      setReasons(evidence);
      setNotice(mode);
      return snapshot();
    } finally {
      if (n === request.current) setBusy("");
    }
  };
  const cancel = () => {
    request.current++;
    setBusy("");
    setNotice("推論を中断・通常検索");
    if (native) invoke("cancel_inference").catch((e) => setError(String(e)));
  };
  const chat = useChats(search, cancel);
  const install = async () => {
    setError("");
    setInstalling(true);
    try {
      await invoke("install_assets");
      await refreshSetup();
      setNotice("ローカルAIの導入が完了しました");
    } catch (e) {
      setError(String(e));
    } finally {
      setInstalling(false);
    }
  };
  const importCatalog = async (selected: File) => {
    try {
      const c = CatalogSchema.parse(JSON.parse(await selected.text()));
      if (!native) throw Error("カタログ導入はデスクトップ版で利用できます");
      await invoke("import_catalog", { catalog: c });
      await reload();
      setNotice("カタログを更新しました");
    } catch (e) {
      setError(`旧カタログを保持しています: ${e}`);
    }
  };
  const ranked = rankWorlds(
      (catalog?.worlds ?? []).filter(
        (w) => candidateIds === null || candidateIds.includes(w.worldId),
      ),
      conditions,
      query,
      personal,
      scores,
    ),
    confirmed = ranked.filter((r) => !r.unknown.length),
    unknown = ranked.filter((r) => r.unknown.length);
  const worlds =
    page === "library"
      ? (catalog?.worlds ?? []).filter(
          (w) =>
            !w.deleted &&
            ((personal[w.worldId] ?? emptyPersonal)[
              libraryFilter as "favorite" | "visited" | "excluded"
            ] ||
              (libraryFilter === "note" && personal[w.worldId]?.note)),
        )
      : confirmed.map((r) => r.world);
  const nav = [
    { id: "search" as Page, label: "チャット", icon: Compass },
    { id: "library" as Page, label: "マイライブラリ", icon: BookOpen },
    { id: "setup" as Page, label: "セットアップ", icon: Download },
    { id: "settings" as Page, label: "設定", icon: Settings },
  ];
  const card = (w: WorldRecord, unconfirmed = false, reply?: Reply) => {
    const cardRanked = reply
      ? rankWorlds(
          reply.worlds ?? catalog?.worlds ?? [],
          reply.conditions,
          "",
          personal,
          reply.scores,
        )
      : ranked;
    const p = personal[w.worldId] ?? emptyPersonal;
    const r = (reply?.reasons ?? reasons).items.find(
      (r) => r.worldId === w.worldId,
    );
    const attributes = Object.entries(w.attributes).filter(
      ([, v]) => v.value === true,
    );
    const matched =
      cardRanked.find((r) => r.world.worldId === w.worldId)?.matched ?? [];
    return (
      <article className="world-card" key={w.worldId}>
        <WorldImage world={w}>
          <button
            className={`favorite ${p.favorite ? "active" : ""}`}
            aria-label={`${w.name}を${p.favorite ? "お気に入り解除" : "お気に入り登録"}`}
            onClick={() => update(w.worldId, { favorite: !p.favorite })}
          >
            <Heart size={18} fill={p.favorite ? "currentColor" : "none"} />
          </button>
        </WorldImage>
        <div className="card-body">
          <div className="world-meta">
            <span>
              {w.platforms?.includes("quest")
                ? "PC + Quest"
                : w.platforms?.includes("pc")
                  ? "PC"
                  : "機種未確認"}
            </span>
            <span>{w.capacity ? `定員 ${w.capacity}人` : "定員未確認"}</span>
          </div>
          <h3>{w.name}</h3>
          <WorldDescription
            key={`${w.worldId}-${w.description}`}
            world={w}
            enabled={!busy && !!setup?.ready}
          />
          <div className="tags">
            {attributes.map(([k]) => (
              <span key={k}>{labels[k as keyof typeof labels]}</span>
            ))}
          </div>
          <div className="reason">
            <Leaf size={14} />
            <p>
              {unconfirmed
                ? `必須条件が未確認: ${cardRanked.find((r) => r.world.worldId === w.worldId)?.unknown.join("・")}`
                : r
                  ? evidenceText(w, r)
                  : matched.length
                    ? `${matched.map((k) => labels[k]).join("・")}が架空カタログの設定と一致しています。`
                    : "カタログ条件で表示。希望する雰囲気は個別にご確認ください。"}
            </p>
          </div>
          <details className="evidence">
            <summary>情報の出典・未確認事項</summary>
            <p>
              取得日: {w.fetchedAt.slice(0, 10)} / {w.sourceLicense}
            </p>
            <p>根拠: {w.sources.map((s) => s.label).join("、")}</p>
            <p>
              未確認:{" "}
              {Object.entries(w.attributes)
                .filter(([, a]) => a.value === null)
                .map(([k]) => labels[k as keyof typeof labels])
                .join("・") || "属性の未確認なし"}
              。定員は二人向け・快適さを保証しません。
            </p>
            <code>{w.worldId}</code>
          </details>
          <div className="card-actions">
            <button
              className={p.visited ? "selected" : ""}
              onClick={() => update(w.worldId, { visited: !p.visited })}
            >
              <Check size={14} />
              {p.visited ? "訪問済み" : "訪問を記録"}
            </button>
            {officialUrl(w) ? (
              <button
                onClick={() =>
                  invoke("open_world", { id: w.worldId }).catch((e) =>
                    setError(String(e)),
                  )
                }
              >
                公式ページ
                <ArrowUpRight size={14} />
              </button>
            ) : (
              <span>架空データ・公式ページなし</span>
            )}
          </div>
          {page === "library" && (
            <div className="personal">
              <label>
                自分用のメモ
                <textarea
                  key={`${w.worldId}-note`}
                  defaultValue={p.note}
                  maxLength={4000}
                  placeholder="ここで過ごした時間をメモする"
                  onBlur={(e) => update(w.worldId, { note: e.target.value })}
                />
              </label>
              <button
                onClick={() => update(w.worldId, { excluded: !p.excluded })}
              >
                {p.excluded ? "検索の除外を解除" : "検索結果から除外"}
              </button>
            </div>
          )}
        </div>
      </article>
    );
  };
  return (
    <div className="app">
      <aside className="sidebar">
        <a
          className="brand"
          href="#"
          onClick={(e) => {
            e.preventDefault();
            setPage("search");
          }}
        >
          <span className="brand-icon">
            <Compass size={27} />
          </span>
          <span>
            <b>World Compass</b>
          </span>
        </a>
        <div className="workspace">ワールドを探すAIチャット</div>
        <ChatHistory chat={chat} open={() => setPage("search")} />
        <nav>
          {nav.map((n) => (
            <button
              key={n.id}
              className={page === n.id ? "current" : ""}
              onClick={() => setPage(n.id)}
            >
              <n.icon size={19} />
              {n.label}
              {n.id === "library" && (
                <small>
                  {Object.values(personal).filter((p) => p.favorite).length}
                </small>
              )}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="privacy">
            <ShieldCheck size={19} />
            <div>
              検索は、このPCの中で。<p>検索文・メモを外部送信しません</p>
            </div>
          </div>
          <div className="version">
            v{version} <span>開発プレビュー</span>
          </div>
        </div>
      </aside>
      <main>
        <header className="topbar">
          <span>
            {page === "search"
              ? "World Compass"
              : page === "library"
                ? "YOUR COLLECTION"
                : page === "setup"
                  ? "GET READY"
                  : "PREFERENCES"}
          </span>
          <div>
            <span className="status-dot" />
            {native ? "ローカルデスクトップ" : "ブラウザプレビュー"}
            <span className="catalog-badge">
              {catalog?.fixture
                ? "架空カタログ"
                : `登録ワールド ${catalog?.worlds.length ?? 0}件`}
            </span>
          </div>
        </header>
        {error && (
          <div className="alert" role="alert">
            {error}
            <button aria-label="通知を閉じる" onClick={() => setError("")}>
              <X size={16} />
            </button>
          </div>
        )}
        {page === "search" ? (
          <ChatPanel
            chat={chat}
            worlds={catalog?.worlds ?? []}
            personal={personal}
            busy={busy}
            ready={!!catalog}
            mode={ai && native && setup?.ready ? "ローカルAI" : "カタログ検索"}
            renderCard={(w, u, r) => card(w, u, r)}
            fixture={catalog?.fixture ?? true}
            openSetup={() => setPage("setup")}
          />
        ) : page === "library" ? (
          <div className="content">
            <div className="page-title">
              <div className="eyebrow">YOUR COLLECTION</div>
              <h1>マイライブラリ</h1>
              <p>お気に入りも、訪れた記録も、このPCに。</p>
            </div>
            <div className="library-tabs">
              {[
                ["favorite", "お気に入り"],
                ["visited", "訪問済み"],
                ["note", "メモあり"],
                ["excluded", "除外中"],
              ].map(([id, label]) => (
                <button
                  key={id}
                  className={libraryFilter === id ? "selected" : ""}
                  onClick={() => setLibraryFilter(id)}
                >
                  {label}
                </button>
              ))}
            </div>
            <div className="results-heading">
              <h2>
                保存したワールド <span>{worlds.length}件</span>
              </h2>
              <span>ローカル保存</span>
            </div>
            <div className="fixture-notice">
              {catalog?.fixture
                ? "開発用の架空ワールドです。"
                : "登録した公開ワールドの情報です。"}
            </div>
            {!catalog ? (
              <div className="empty">読み込んでいます…</div>
            ) : worlds.length ? (
              <div className="world-grid">{worlds.map((w) => card(w))}</div>
            ) : (
              <div className="empty">
                <BookOpen size={30} />
                <h3>まだ記録がありません</h3>
                <p>チャットの提案からお気に入りや訪問を記録できます。</p>
              </div>
            )}
            <footer>
              <ShieldCheck size={14} />
              検索文・メモは外部送信しません
              <span>
                カタログ更新日{" "}
                {catalog?.worlds[0]?.fetchedAt.slice(0, 10) ?? "—"}
              </span>
            </footer>
          </div>
        ) : page === "setup" ? (
          <div className="content narrow">
            <div className="eyebrow">LOCAL AI, READY WHEN YOU ARE</div>
            <h1>このPCに、探索の準備を。</h1>
            <p className="sub">
              通常検索はすぐに使えます。AIを使う場合は、専用ランタイムとモデルを導入します。
            </p>
            <Practical
              changed={async () => {
                await reload();
                await refreshSetup();
              }}
              fail={setError}
              disabled={!!busy || installing}
            />
            <section className="panel">
              <h2>
                <Monitor size={20} />
                このPCの状態
              </h2>
              <dl>
                <dt>保存先</dt>
                <dd>{setup?.root ?? "デスクトップ版で表示"}</dd>
                <dt>RAM</dt>
                <dd>{setup ? mb(setup.ram) : "—"}</dd>
                <dt>空き容量</dt>
                <dd>{setup ? mb(setup.free) : "—"}</dd>
                <dt>GPU / VRAM</dt>
                <dd>{setup?.gpu ?? "未計測"}</dd>
              </dl>
            </section>
            <section className="panel">
              <h2>
                <Download size={20} />
                導入するもの
              </h2>
              <p>
                通信先はGitHubとHugging
                Faceの固定バージョン配布元（4B量子化はUnsloth）です。検索文・履歴は送信しません。各配布元には通常のダウンロード通信情報が渡ります。
              </p>
              {setup?.manifest.assets.map((a) => (
                <div className="asset" key={a.id}>
                  <div>
                    <strong>
                      {modelNames[a.id] ??
                        (a.id.startsWith("runtime")
                          ? `llama.cpp ${a.id === "runtime-vulkan" ? "Vulkan" : "CPU"}`
                          : "Qwen3 Embedding 0.6B · Q8_0")}
                    </strong>
                    <small>
                      {a.version.slice(0, 12)} · {a.license}
                    </small>
                  </div>
                  <span>{mb(a.size)}</span>
                </div>
              ))}
              <p>
                取得容量:{" "}
                {setup
                  ? mb(setup.manifest.assets.reduce((n, a) => n + a.size, 0))
                  : "約 1.3 GB"}
                。展開・検証用に追加の空き容量が必要です。モデルのライセンスはApache-2.0、ランタイムはMITです。最低スペックと日本語品質は未確定です。
              </p>
              <details className="license-text">
                <summary>Apache-2.0 / MIT</summary>
                <pre>{apacheLicense}</pre>
                <pre>{runtimeLicense}</pre>
              </details>
              <div className="setup-buttons">
                <button
                  className="primary"
                  disabled={!native || installing || !!busy}
                  onClick={install}
                >
                  {installing ? (
                    <LoaderCircle className="spin" size={17} />
                  ) : (
                    <Download size={17} />
                  )}
                  利用条件を確認して{setup?.ready ? "検証・修復" : "導入する"}
                </button>
                {installing && (
                  <button onClick={() => invoke("cancel_setup")}>
                    中断する
                  </button>
                )}
              </div>
              {installing && (
                <div className="download-progress">
                  <progress value={progress.done} max={progress.total} />
                  <span>
                    {progress.id} {mb(progress.done)} / {mb(progress.total)}
                  </span>
                </div>
              )}
              <p className="muted">
                中断後は同じボタンで再開できます。導入はこのアプリ専用の保存先で行います。
              </p>
            </section>
            <section className="panel">
              <h2>カタログの更新</h2>
              <p>
                実在ワールドは上のURL登録で追加できます。デモ用JSONの更新にも対応しています。
              </p>
              <input
                ref={file}
                type="file"
                accept=".json"
                hidden
                onChange={(e) => {
                  const f = e.target.files?.[0];
                  if (f) importCatalog(f);
                  e.target.value = "";
                }}
              />
              <button disabled={!native} onClick={() => file.current?.click()}>
                架空カタログJSONを導入
              </button>
            </section>
          </div>
        ) : (
          <div className="content narrow">
            <div className="eyebrow">YOUR PC, YOUR PREFERENCES</div>
            <h1>探索の設定</h1>
            <section className="panel">
              <div className="setting-row">
                <div>
                  <h2>ローカルAIを使う</h2>
                  <p>
                    条件抽出、意味検索、根拠選択をPC内で実行。失敗時は通常検索に戻ります。
                  </p>
                </div>
                <input
                  aria-label="ローカルAIを使う"
                  type="checkbox"
                  checked={ai}
                  onChange={(e) => {
                    setAi(e.target.checked);
                    localStorage.setItem(
                      "world-compass-ai",
                      String(e.target.checked),
                    );
                  }}
                />
              </div>
              <p>
                {setup?.ready
                  ? "モデル導入済み"
                  : native
                    ? "モデルを使うにはセットアップで導入してください"
                    : "ブラウザプレビューでは通常検索のみ利用できます"}
              </p>
              <div className="setting-row">
                <div>
                  <h2>AIで提案根拠を選ぶ</h2>
                  <p>
                    上位3件を比較します。追加の推論時間がかかります。オフでもカタログの根拠を表示します。
                  </p>
                </div>
                <input
                  aria-label="AIで提案根拠を選ぶ"
                  type="checkbox"
                  checked={aiReasons}
                  onChange={(e) => {
                    setAiReasons(e.target.checked);
                    localStorage.setItem(
                      "world-compass-ai-reasons",
                      String(e.target.checked),
                    );
                  }}
                />
              </div>
            </section>
            <section className="panel">
              <h2>VRChatとの同時利用</h2>
              <dl>
                <dt>実行デバイス</dt>
                <dd>{setup?.gpu ?? "未計測"}</dd>
                <dt>スレッド / 並列数</dt>
                <dd>設定したCPUスレッド数 / 1</dd>
                <dt>コンテキスト長</dt>
                <dd>4096 tokens</dd>
                <dt>モデル解放</dt>
                <dd>モデル設定の待機時間で停止</dd>
                <dt>推論プロセスのRAM</dt>
                <dd>
                  {mb(runtime.memory)} / {runtime.running ? "実行中" : "停止中"}
                  （2秒ごとに更新）
                </dd>
              </dl>
              <button disabled={!native} onClick={cancel}>
                推論停止・モデルを解放
              </button>
              <p>
                RAM使用量・VRChat同時起動・応答時間は実機測定が必要です。現時点で性能は保証していません。
              </p>
            </section>
            <section className="panel">
              <h2>プライバシーとデータ</h2>
              <p>
                お気に入り・手動訪問記録・メモ・除外はローカルに保存します。VRChatへのログイン・認証情報取得・自動訪問記録・テレメトリーはありません。画像はVRChatの公開配信先から取得して保存します。本文の日本語訳もPC内で生成・保存します。
              </p>
              <p>
                非公式の開発版です。名称の商標確認、本番データの許諾、商用利用、クリーンなWindowsでの動作確認は未完了です。
              </p>
            </section>
          </div>
        )}
      </main>
    </div>
  );
}
