import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface DashboardStats {
  total_topics: number;
  total_articles: number;
  total_posts: number;
}

interface TopicInfo {
  name: string;
  language: string | null;
  interval_min: number;
  sources: { type: string; url: string }[];
}

interface TopicStat {
  topic_id: string;
  post_count: number;
  last_post_at: string | null;
}

interface AppStatus {
  running: boolean;
  topic_next_fetch: Record<string, string>;
}

interface PostSummary {
  id: number;
  topic_id: string;
  title: string;
  created_at: string;
}

export default function Dashboard() {
  const [stats, setStats] = useState<DashboardStats | null>(null);
  const [topics, setTopics] = useState<TopicInfo[]>([]);
  const [topicStats, setTopicStats] = useState<TopicStat[]>([]);
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [recentPosts, setRecentPosts] = useState<PostSummary[]>([]);
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);
  const toastTimerRef = useRef<number | undefined>(undefined);
  const mountedRef = useRef(true);

  const showToast = (msg: string, ok: boolean) => {
    setToast({ msg, ok });
    clearTimeout(toastTimerRef.current);
    toastTimerRef.current = window.setTimeout(() => setToast(null), 4000);
  };

  const loadingRef = useRef(false);

  const loadData = async () => {
    // ポーリングのオーバーラップ防止（invoke 連鎖が interval より遅い場合）
    if (loadingRef.current) return;
    loadingRef.current = true;
    try {
      // 各データを独立に取得（1つが失敗しても他は表示する）
      try {
        const s = await invoke<DashboardStats>("get_stats");
        setStats(s);
      } catch (e) {
        console.error("get_stats failed", e);
      }

      try {
        const t = await invoke<TopicInfo[]>("get_topics");
        setTopics(t);
      } catch (e) {
        console.error("get_topics failed", e);
      }

      try {
        const ts = await invoke<TopicStat[]>("get_topic_stats");
        setTopicStats(ts);
      } catch (e) {
        console.error("get_topic_stats failed", e);
      }

      try {
        const st = await invoke<AppStatus>("get_status");
        setStatus(st);
      } catch (e) {
        console.error("get_status failed", e);
      }

      try {
        const p = await invoke<PostSummary[]>("get_posts", { topicId: "", limit: 10 });
        setRecentPosts(p);
      } catch (e) {
        console.error("get_posts failed", e);
      }
    } finally {
      loadingRef.current = false;
    }
  };

  useEffect(() => {
    mountedRef.current = true;
    loadData();
    const interval = setInterval(loadData, 10000);
    return () => {
      mountedRef.current = false;
      clearInterval(interval);
      clearTimeout(toastTimerRef.current);
    };
  }, []);

  const handleRefresh = async (topicName: string) => {
    try {
      await invoke("refresh_topic", { topicName });
      showToast("Refreshed: " + topicName, true);
      setTimeout(() => {
        if (mountedRef.current) loadData();
      }, 1500);
    } catch (e) {
      showToast("Refresh failed: " + e, false);
    }
  };

  const statFor = (name: string) => topicStats.find((s) => s.topic_id === name);

  // SQLite datetime('now') は UTC の "YYYY-MM-DD HH:MM:SS" → Z 付きで UTC 解釈する。
  // RFC3339（get_status 等）は T 区切りなのでそのまま。
  const parseUtc = (iso: string | null | undefined): number => {
    if (!iso) return NaN;
    const normalized = iso.includes("T") ? iso : iso.replace(" ", "T") + "Z";
    return new Date(normalized).getTime();
  };

  const fmtRelative = (iso: string | null | undefined) => {
    const t = parseUtc(iso);
    if (Number.isNaN(t)) return null;
    const diffSec = Math.round((t - Date.now()) / 1000);
    if (diffSec <= 0) return "now";
    if (diffSec < 60) return `${diffSec}s`;
    if (diffSec < 3600) return `${Math.floor(diffSec / 60)}m`;
    if (diffSec < 86400) return `${Math.floor(diffSec / 3600)}h`;
    return `${Math.floor(diffSec / 86400)}d`;
  };

  const fmtLastTime = (iso: string | null | undefined) => {
    if (!iso) return "never";
    const t = parseUtc(iso);
    if (Number.isNaN(t)) return "never";
    const diffSec = Math.round((Date.now() - t) / 1000);
    if (diffSec < 60) return "just now";
    if (diffSec < 3600) return `${Math.floor(diffSec / 60)}m ago`;
    if (diffSec < 86400) return `${Math.floor(diffSec / 3600)}h ago`;
    return `${Math.floor(diffSec / 86400)}d ago`;
  };

  return (
    <div className="fade-in" style={{ position: "relative" }}>
      {/* Toast notification */}
      {toast && (
        <div
          style={{
            position: "fixed",
            top: 16,
            right: 16,
            zIndex: 999,
            padding: "10px 18px",
            borderRadius: "var(--radius-sm)",
            background: toast.ok ? "rgba(63,185,80,0.9)" : "rgba(248,81,73,0.9)",
            color: "#fff",
            fontSize: 13,
            fontWeight: 500,
            boxShadow: "0 4px 12px rgba(0,0,0,0.3)",
            maxWidth: 360,
            wordBreak: "break-word",
          }}
        >
          {toast.msg}
        </div>
      )}

      <div className="page-header">
        <h1>Dashboard</h1>
        <div className="status-pill">
          <span
            className={`status-dot ${
              status === null ? "" : status.running ? "" : "idle"
            }`}
          />
          {status === null ? "..." : status.running ? "Running" : "Stopped"}
        </div>
      </div>
      <div className="page-body">
        <div className="stats-bar">
          <div className="stat-card">
            <span className="stat-icon">{"\u{1F4E1}"}</span>
            <div className="stat-info">
              <div className="stat-label">Topics</div>
              <div className="stat-value">{stats?.total_topics ?? "-"}</div>
            </div>
          </div>
          <div className="stat-card">
            <span className="stat-icon">{"\u{1F4C4}"}</span>
            <div className="stat-info">
              <div className="stat-label">Articles</div>
              <div className="stat-value">{stats?.total_articles ?? "-"}</div>
            </div>
          </div>
          <div className="stat-card">
            <span className="stat-icon">{"\u{1F4AC}"}</span>
            <div className="stat-info">
              <div className="stat-label">Posts Published</div>
              <div className="stat-value">{stats?.total_posts ?? "-"}</div>
            </div>
          </div>
        </div>

        <h2 style={{ fontSize: 16, fontWeight: 600, marginBottom: 12, color: "var(--text-secondary)" }}>
          Topics Overview
        </h2>
        <div className="card-grid">
          {topics.length === 0 && (
            <div className="empty-state" style={{ gridColumn: "1 / -1" }}>
              <div className="empty-icon">{"\u{1F4ED}"}</div>
              <p>No topics configured. Add topics in the Topics page.</p>
            </div>
          )}
          {topics.map((topic) => {
            const st = statFor(topic.name);
            const next = status?.topic_next_fetch?.[topic.name];
            return (
              <div className="topic-card" key={topic.name}>
                <div className="topic-card-header">
                  <span className="topic-card-title">{topic.name}</span>
                  <button
                    className="btn btn-sm"
                    onClick={() => handleRefresh(topic.name)}
                    title="Fetch and generate now"
                  >
                    {"\u{1F504}"} Refresh
                  </button>
                </div>
                <div className="topic-card-meta">
                  <span>{"\u{1F4E1}"} {topic.sources.length} sources</span>
                  <span>{"\u23F1"} Every {topic.interval_min} min</span>
                  {topic.language && <span className="tag tag-blue">{topic.language}</span>}
                </div>
                <div className="topic-card-meta" style={{ borderTop: "1px solid var(--border-color)", paddingTop: 8 }}>
                  <span>{"\u{1F4C4}"} {st?.post_count ?? 0} posts</span>
                  <span title={st?.last_post_at ?? ""}>
                    {"\u{1F551}"} Last: {fmtLastTime(st?.last_post_at)}
                  </span>
                  {next && (
                    <span className="tag tag-green">
                      {"\u{23F1}"} Next in {fmtRelative(next)}
                    </span>
                  )}
                </div>
              </div>
            );
          })}
        </div>

        {recentPosts.length > 0 && (
          <>
            <h2 style={{ fontSize: 16, fontWeight: 600, marginTop: 32, marginBottom: 12, color: "var(--text-secondary)" }}>
              Recent Posts
            </h2>
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {recentPosts.map((post) => (
                <div className="card" key={post.id}>
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                    <div>
                      <span className="tag tag-green" style={{ marginRight: 8 }}>{post.topic_id}</span>
                      {post.title}
                    </div>
                    <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
                      {new Date(post.created_at + "Z").toLocaleString()}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
