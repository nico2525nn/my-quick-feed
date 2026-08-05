import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

// UTC 時刻をローカル時刻（JST）表示に変換する。
// 対応形式:
//   - omp セッション: "2026-08-04T13-00-05-385Z"（ダッシュ区切りのため Date が直接パースできない）
//   - SQLite: "2026-08-05 00:08:37"（UTC。そのまま new Date するとローカル解釈で 9 時間ずれる）
const fmtLocalTime = (s: string): string => {
  let d: Date | null = null;
  const m = s.match(/^(\d{4}-\d{2}-\d{2})T(\d{2})-(\d{2})-(\d{2})-(\d{3})Z$/);
  if (m) d = new Date(`${m[1]}T${m[2]}:${m[3]}:${m[4]}.${m[5]}Z`);
  else if (/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(s))
    d = new Date(s.replace(" ", "T") + "Z");
  if (d && !isNaN(d.getTime()))
    return d.toLocaleString("ja-JP", {
      month: "numeric",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  return s;
};

interface SourceConfig {
  type: string;
  url: string;
}

interface TopicConfig {
  name: string;
  language: string | null;
  interval_min: number;
  sources: SourceConfig[];
  system_prompt: string | null;
  image_search_enabled: boolean | null;
  research_enabled: boolean | null;
  forum_channel_id: string | null;
  reference_urls: string[];
  reference_mode: string | null;
}

interface AppConfig {
  discord: { token: string; forum_channel_id: string };
  ai: {
    mode: string;
    agent_command: string | null;
    agent_timeout_sec: number | null;
    api_key: string | null;
    model: string | null;
    base_url: string | null;
  };
  topics: TopicConfig[];
}

// トピック詳細（get_topic_detail）の戻り値
interface PostSummary {
  id: number;
  topic_id: string;
  title: string;
  created_at: string;
}

interface TopicSession {
  created_at: string;
  prompt_preview: string;
  response_preview: string;
  prompt_full: string;
  response_full: string;
  thinking_full: string;
}

interface TopicDetail {
  config: TopicConfig;
  posts: PostSummary[];
  sessions: TopicSession[];
}

function generateTemplatePrompt(name: string, language: string): string {
  if (!name) return "";
  switch (language) {
    case "ja":
      return `あなたは${name}に関するニュース記事をまとめるアシスタントです。
以下の情報源から収集した情報を基に、簡潔なニュース記事を1件生成してください。
タイトルは「【${name}】」で始め、本文は300字程度にまとめてください。
出典を明記し、複数のソースを統合する場合はその旨も記載してください。`;
    case "en":
      return `You are an assistant that summarizes news about ${name}.
Create one concise news article based on the information collected from the sources below.
Start the title with "【${name}】" and keep the body around 300 characters.
Cite your sources and mention when multiple sources are combined.`;
    default:
      return `You are an assistant that summarizes news about ${name}.
Create one concise news article based on the information provided.
Start the title with "【${name}】" and cite your sources.`;
  }
}

function emptyTopic(): TopicConfig {
  return {
    name: "",
    language: "ja",
    interval_min: 120,
    sources: [{ type: "rss", url: "" }],
    system_prompt: null,
    image_search_enabled: true,
    research_enabled: true,
    forum_channel_id: null,
    reference_urls: [],
    reference_mode: null,
  };
}

export default function TopicsPage() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [editing, setEditing] = useState<TopicConfig | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [autoPrompt, setAutoPrompt] = useState(false);
  // 編集開始時のトピック名（リネーム時に元の名前でマッチングする）
  const [originalName, setOriginalName] = useState<string | null>(null);
  const [message, setMessage] = useState<{ type: string; text: string } | null>(null);
  const messageTimerRef = useRef<number | undefined>(undefined);
  // 実行中（Refresh 中）のトピック名
  const [refreshing, setRefreshing] = useState<Set<string>>(new Set());
  // トピック詳細モーダル（設定・投稿ニュース・セッション履歴）
  const [detail, setDetail] = useState<TopicDetail | null>(null);

  const showMessage = (type: string, text: string) => {
    setMessage({ type, text });
    clearTimeout(messageTimerRef.current);
    messageTimerRef.current = window.setTimeout(() => setMessage(null), 4000);
  };

  // 新規トピック作成時: 名前/言語の変更に応じてプロンプトを自動生成
  const updateEditing = (updates: Partial<TopicConfig>) => {
    if (!editing) return;
    const next = { ...editing, ...updates };
    if (autoPrompt && (updates.name !== undefined || updates.language !== undefined)) {
      const name = next.name || "";
      const lang = next.language || "ja";
      next.system_prompt = name ? generateTemplatePrompt(name, lang) : null;
    }
    setEditing(next);
  };


  const loadConfig = async () => {
    try {
      const c = await invoke<AppConfig>("get_config");
      setConfig(c);
    } catch (e) {
      console.error("Failed to load config", e);
    }
  };

  useEffect(() => {
    loadConfig();
    return () => {
      clearTimeout(messageTimerRef.current);
    };
  }, []);

  const handleSave = async () => {
    if (!config || !editing) return;
    // バリデーション: 名前必須・重複禁止・ソース URL 必須
    if (!editing.name.trim()) {
      showMessage("error", "Topic name is required");
      return;
    }
    if (editing.sources.length === 0) {
      showMessage("error", "At least one source is required");
      return;
    }
    if (editing.sources.some((s) => !s.url.trim())) {
      showMessage("error", "All sources require a URL");
      return;
    }
    const updated = { ...config };
    if (isNew) {
      if (updated.topics.some((t) => t.name === editing.name)) {
        showMessage("error", "A topic with this name already exists");
        return;
      }
      updated.topics = [...updated.topics, editing];
    } else {
      // リネームされても元の名前でマッチングする（名前変更で保存が消えるバグ対策）
      updated.topics = updated.topics.map((t) =>
        t.name === originalName ? editing : t
      );
    }
    try {
      await invoke("update_config", { config: updated });
      setConfig(updated);
      setEditing(null);
      setIsNew(false);
      setAutoPrompt(false);
      showMessage("success", "Saved");
    } catch (e) {
      showMessage("error", `Failed to save topic: ${e}`);
    }
  };

  const handleDelete = async (name: string) => {
    if (!config) return;
    const updated = {
      ...config,
      topics: config.topics.filter((t) => t.name !== name),
    };
    try {
      await invoke("update_config", { config: updated });
      setConfig(updated);
      showMessage("success", "Deleted: " + name);
    } catch (e) {
      showMessage("error", `Failed to delete topic: ${e}`);
    }
  };

  const handleRefresh = async (name: string) => {
    if (refreshing.has(name)) return;
    setRefreshing((prev) => new Set(prev).add(name));
    try {
      await invoke("refresh_topic", { topicName: name });
      showMessage("success", "Refreshed: " + name);
    } catch (e) {
      showMessage("error", `Refresh failed: ${e}`);
    } finally {
      setRefreshing((prev) => {
        const next = new Set(prev);
        next.delete(name);
        return next;
      });
    }
  };

  const openDetail = async (topic: TopicConfig) => {
    setMessage(null);
    try {
      const detail = await invoke<TopicDetail>("get_topic_detail", {
        topicName: topic.name,
      });
      setDetail(detail);
    } catch (e) {
      showMessage("error", `詳細の取得に失敗しました: ${e}`);
    }
  };

  const openNew = () => {
    setEditing(emptyTopic());
    setIsNew(true);
    setAutoPrompt(true);
    setOriginalName(null);
    setMessage(null);
  };

  const openEdit = (topic: TopicConfig) => {
    setEditing({ ...topic });
    setIsNew(false);
    // 既存トピックの手書きプロンプトを自動生成で上書きしない
    setAutoPrompt(false);
    setOriginalName(topic.name);
    setMessage(null);
  };

  const addSource = () => {
    if (!editing) return;
    setEditing({
      ...editing,
      sources: [...editing.sources, { type: "rss", url: "" }],
    });
  };

  const updateSource = (index: number, field: keyof SourceConfig, value: string) => {
    if (!editing) return;
    const sources = [...editing.sources];
    sources[index] = { ...sources[index], [field]: value };
    setEditing({ ...editing, sources });
  };

  const removeSource = (index: number) => {
    if (!editing) return;
    const sources = editing.sources.filter((_, i) => i !== index);
    setEditing({ ...editing, sources });
  };

  if (!config) return <div className="page-body">Loading...</div>;

  return (
    <div className="fade-in">
      <div className="page-header">
        <h1>Topics</h1>
        <button className="btn btn-primary" onClick={openNew}>
          + Add Topic
        </button>
      </div>
      <div className="page-body">
        {message && (
          <div
            style={{
              padding: "10px 16px",
              borderRadius: "var(--radius-sm)",
              marginBottom: 16,
              background:
                message.type === "success"
                  ? "rgba(63, 185, 80, 0.15)"
                  : "rgba(248, 81, 73, 0.15)",
              color:
                message.type === "success"
                  ? "var(--accent-green)"
                  : "var(--accent-red)",
              fontSize: 13,
            }}
          >
            {message.text}
          </div>
        )}
        {config.topics.length === 0 && (
          <div className="empty-state">
            <div className="empty-icon">{"\u{1F4ED}"}</div>
            <p>No topics configured. Click "Add Topic" to get started.</p>
          </div>
        )}

        <div className="card-grid">
          {config.topics.map((topic) => (
            <div className="topic-card" key={topic.name}>
              <div className="topic-card-header">
                <span className="topic-card-title">{topic.name}</span>
                <div className="topic-card-actions">
                  <button
                    className="btn btn-sm"
                    onClick={() => openDetail(topic)}
                    title="設定・投稿・セッション履歴を表示"
                  >
                    詳細
                  </button>
                  <button className="btn btn-sm" onClick={() => openEdit(topic)}>
                    Edit
                  </button>
                  <button
                    className="btn btn-sm"
                    onClick={() => handleRefresh(topic.name)}
                    disabled={refreshing.has(topic.name)}
                    title="Fetch and generate now"
                  >
                    {refreshing.has(topic.name) ? "..." : "\u{1F504}"}
                  </button>
                  <button className="btn btn-sm btn-danger" onClick={() => handleDelete(topic.name)}>
                    Delete
                  </button>
                </div>
              </div>
              <div className="topic-card-meta">
                <span>{"\u{1F4E1}"} {topic.sources.length} sources</span>
                <span>{"\u23F1"} Every {topic.interval_min} min</span>
                {topic.language && <span className="tag tag-blue">{topic.language}</span>}
                {topic.research_enabled && <span className="tag tag-green">Research</span>}
                {topic.image_search_enabled && <span className="tag tag-yellow">Images</span>}
              </div>
              <div style={{ fontSize: 12, color: "var(--text-muted)" }}>
                {topic.sources.map((s, i) => (
                  <div key={i}>
                    <span className="tag" style={{ background: "var(--bg-primary)", marginRight: 4 }}>
                      {s.type}
                    </span>
                    {s.url}
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* Edit Modal */}
      {editing && (
        <div className="modal-overlay" onClick={() => setEditing(null)}>
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <h2 style={{ marginBottom: 20, fontSize: 16, fontWeight: 600 }}>
              {isNew ? "Add Topic" : "Edit Topic"}
            </h2>

            <div className="form-group">
              <label>Topic Name</label>
              <input
                className="form-input"
                value={editing.name}
                onChange={(e) => updateEditing({ name: e.target.value })}
                placeholder="e.g. APEXまとめ"
              />
            </div>

            <div className="form-row">
              <div className="form-group">
                <label>Language</label>
                <select
                  className="form-select"
                  value={editing.language ?? "ja"}
                  onChange={(e) =>
                    updateEditing({ language: e.target.value || "ja" })
                  }
                >
                  <option value="ja">Japanese</option>
                  <option value="en">English</option>
                  <option value="ko">Korean</option>
                  <option value="zh">Chinese</option>
                </select>
              </div>
              <div className="form-group">
                <label>Interval (minutes)</label>
                <input
                  className="form-input"
                  type="number"
                  min={1}
                  value={editing.interval_min}
                  onChange={(e) =>
                    setEditing({
                      ...editing,
                      // 0/負数はバックエンドの .max(1) に任せず UI でクランプする
                      interval_min: Math.max(1, parseInt(e.target.value) || 60),
                    })
                  }
                />
              </div>
            </div>

            <div className="form-group">
              <label>Forum Channel ID（省略時は Settings のグローバル設定）</label>
              <input
                className="form-input"
                value={editing.forum_channel_id ?? ""}
                onChange={(e) =>
                  updateEditing({ forum_channel_id: e.target.value || null })
                }
                placeholder="例: 1234567890123456789（空ならグローバル設定を使用）"
              />
            </div>

            <div className="form-group">
              <label>Sources</label>
              {editing.sources.map((src, i) => (
                <div key={i} style={{ display: "flex", gap: 8, marginBottom: 8 }}>
                  <select
                    className="form-select"
                    style={{ width: 120, flexShrink: 0 }}
                    value={src.type}
                    onChange={(e) => updateSource(i, "type", e.target.value)}
                  >
                    <option value="rss">RSS</option>
                    <option value="rsshub">RSSHUB</option>
                  </select>
                  <input
                    className="form-input"
                    value={src.url}
                    onChange={(e) => updateSource(i, "url", e.target.value)}
                    placeholder="Feed URL"
                  />
                  <button
                    className="btn btn-sm btn-danger"
                    onClick={() => removeSource(i)}
                    style={{ flexShrink: 0 }}
                  >
                    Remove
                  </button>
                </div>
              ))}
              <button className="btn btn-sm" onClick={addSource} style={{ marginTop: 4 }}>
                + Add Source
              </button>
            </div>

            <div className="form-group">
              <label>Reference URLs（1行に1つ）</label>
              <textarea
                className="form-textarea"
                rows={3}
                value={editing.reference_urls.join("\n")}
                onChange={(e) =>
                  updateEditing({
                    reference_urls: e.target.value
                      .split("\n")
                      .map((u) => u.trim())
                      .filter((u) => u.length > 0),
                  })
                }
                placeholder="例: https://apexlegends.swiki.jp/"
              />
              <label style={{ marginTop: 8 }}>Reference の使い方</label>
              <select
                className="form-select"
                value={editing.reference_mode ?? "on-demand"}
                onChange={(e) =>
                  updateEditing({ reference_mode: e.target.value })
                }
              >
                <option value="on-demand">必要に応じて読む（URL のみ・軽量）</option>
                <option value="preload">事前に全部読んで知識を得てから書く（幻覚対策）</option>
              </select>
              <div style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 4 }}>
                preload は「このサイトのページを可能な限り全て読んでから記事を書く」よう指示します。
                読む量が多いと生成が遅くなる・コンテキストが大きくなる点に注意。
              </div>
            </div>

            <div className="form-group">
              <label>System Prompt</label>
              <textarea
                className="form-textarea"
                rows={4}
                value={editing.system_prompt ?? ""}
                  onChange={(e) => {
                    setAutoPrompt(false);
                    updateEditing({ system_prompt: e.target.value || null });
                  }}
                placeholder="Optional: custom system prompt for the AI agent"
              />
            </div>

            <div className="form-row">
              <label className="form-checkbox">
                <input
                  type="checkbox"
                  checked={editing.research_enabled ?? true}
                  onChange={(e) =>
                    setEditing({ ...editing, research_enabled: e.target.checked })
                  }
                />
                Enable Research
              </label>
              <label className="form-checkbox">
                <input
                  type="checkbox"
                  checked={editing.image_search_enabled ?? true}
                  onChange={(e) =>
                    setEditing({ ...editing, image_search_enabled: e.target.checked })
                  }
                />
                Enable Image Search
              </label>
            </div>

            <div className="modal-actions">
              <button className="btn" onClick={() => setEditing(null)}>
                Cancel
              </button>
              <button className="btn btn-primary" onClick={handleSave}>
                {isNew ? "Create" : "Save"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* トピック詳細モーダル */}
      {detail && (
        <div className="modal-overlay" onClick={() => setDetail(null)}>
          <div
            className="modal-content"
            style={{ maxWidth: 720 }}
            onClick={(e) => e.stopPropagation()}
          >
            <h2 style={{ marginBottom: 20, fontSize: 16, fontWeight: 600 }}>
              {detail.config.name}
            </h2>

            <div className="form-group">
              <label>設定</label>
              <div style={{ fontSize: 13, lineHeight: 1.9 }}>
                <div>言語: {detail.config.language ?? "ja"}</div>
                <div>間隔: {detail.config.interval_min} 分ごと</div>
                <div>ソース数: {detail.config.sources.length}</div>
                <div>
                  フォーラムチャンネル:{" "}
                  {detail.config.forum_channel_id || "（グローバル設定を使用）"}
                </div>
                {detail.config.reference_urls.length > 0 && (
                  <div>
                    参考文献:
                    {detail.config.reference_urls.map((u, i) => (
                      <div key={i} style={{ wordBreak: "break-all" }}>
                        {"\u2022"} {u}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>

            <div className="form-group">
              <label>投稿ニュース（直近10件）</label>
              {detail.posts.length === 0 ? (
                <div style={{ fontSize: 13, color: "var(--text-muted)" }}>
                  まだ投稿がありません
                </div>
              ) : (
                <ul style={{ margin: 0, paddingLeft: 18, fontSize: 13, lineHeight: 1.8 }}>
                  {detail.posts.map((p) => (
                    <li key={p.id}>
                      <span style={{ color: "var(--text-muted)", marginRight: 8 }}>
                        {fmtLocalTime(p.created_at)}
                      </span>
                      {p.title}
                    </li>
                  ))}
                </ul>
              )}
            </div>

            <div className="form-group">
              <label>セッション履歴（直近10件）</label>
              {detail.sessions.length === 0 ? (
                <div style={{ fontSize: 13, color: "var(--text-muted)" }}>
                  セッション履歴がありません
                </div>
              ) : (
                detail.sessions.map((s, i) => (
                  <div
                    key={i}
                    style={{
                      border: "1px solid var(--border-color)",
                      borderRadius: "var(--radius-sm)",
                      padding: 10,
                      marginBottom: 10,
                    }}
                  >
                    <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 6 }}>
                      {fmtLocalTime(s.created_at)}
                    </div>
                    <details style={{ marginBottom: 8 }}>
                      <summary
                        style={{
                          fontSize: 12,
                          cursor: "pointer",
                          color: "var(--accent-blue)",
                          marginBottom: 4,
                        }}
                      >
                        <span className="tag tag-blue">プロンプト</span> 展開して全文を見る
                      </summary>
                      <pre
                        style={{
                          margin: 0,
                          fontSize: 11,
                          whiteSpace: "pre-wrap",
                          wordBreak: "break-all",
                          color: "var(--text-secondary)",
                          background: "var(--bg-primary)",
                          borderRadius: "var(--radius-sm)",
                          padding: 8,
                          maxHeight: 300,
                          overflowY: "auto",
                        }}
                      >
                        {s.prompt_full || s.prompt_preview}
                      </pre>
                    </details>
                    <details style={{ marginBottom: 8 }}>
                      <summary
                        style={{
                          fontSize: 12,
                          cursor: "pointer",
                          color: "var(--accent-green)",
                          marginBottom: 4,
                        }}
                      >
                        <span className="tag tag-green">回答</span> 展開して全文を見る
                      </summary>
                      <pre
                        style={{
                          margin: 0,
                          fontSize: 11,
                          whiteSpace: "pre-wrap",
                          wordBreak: "break-all",
                          color: "var(--text-secondary)",
                          background: "var(--bg-primary)",
                          borderRadius: "var(--radius-sm)",
                          padding: 8,
                          maxHeight: 400,
                          overflowY: "auto",
                        }}
                      >
                        {s.response_full || s.response_preview}
                      </pre>
                    </details>
                    {s.thinking_full && (
                      <details style={{ marginBottom: 8 }}>
                        <summary
                          style={{
                            fontSize: 12,
                            cursor: "pointer",
                            color: "var(--accent-yellow)",
                            marginBottom: 4,
                          }}
                        >
                          <span className="tag tag-yellow">思考ログ</span> 展開して全文を見る
                        </summary>
                        <pre
                          style={{
                            margin: 0,
                            fontSize: 11,
                            whiteSpace: "pre-wrap",
                            wordBreak: "break-all",
                            color: "var(--text-muted)",
                            background: "var(--bg-primary)",
                            borderRadius: "var(--radius-sm)",
                            padding: 8,
                            maxHeight: 400,
                            overflowY: "auto",
                          }}
                        >
                          {s.thinking_full}
                        </pre>
                      </details>
                    )}
                  </div>
                ))
              )}
            </div>

            <div className="modal-actions">
              <button className="btn" onClick={() => setDetail(null)}>
                Close
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
