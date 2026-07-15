import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

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
  };
}

export default function TopicsPage() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [editing, setEditing] = useState<TopicConfig | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [autoPrompt, setAutoPrompt] = useState(false);

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
  }, []);

  const handleSave = async () => {
    if (!config || !editing) return;
    const updated = { ...config };
    if (isNew) {
      updated.topics = [...updated.topics, editing];
    } else {
      updated.topics = updated.topics.map((t) =>
        t.name === editing.name ? editing : t
      );
    }
    try {
      await invoke("update_config", { config: updated });
      setConfig(updated);
      setEditing(null);
      setIsNew(false);
    } catch (e) {
      console.error("Failed to save topic", e);
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
    } catch (e) {
      console.error("Failed to delete topic", e);
    }
  };

  const handleRefresh = async (name: string) => {
    try {
      await invoke("refresh_topic", { topic_name: name });
    } catch (e) {
      console.error("Refresh failed", e);
    }
  };

  const openNew = () => {
    setEditing(emptyTopic());
    setIsNew(true);
    setAutoPrompt(true);
  };

  const openEdit = (topic: TopicConfig) => {
    setEditing({ ...topic });
    setIsNew(false);
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
                  <button className="btn btn-sm" onClick={() => openEdit(topic)}>
                    Edit
                  </button>
                  <button className="btn btn-sm" onClick={() => handleRefresh(topic.name)}>
                    {"\u{1F504}"}
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
                  onChange={(e) => updateEditing({ language: e.target.value })}
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
                    setEditing({ ...editing, interval_min: parseInt(e.target.value) || 60 })
                  }
                />
              </div>
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
    </div>
  );
}
