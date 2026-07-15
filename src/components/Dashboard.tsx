import { useEffect, useState } from "react";
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

interface PostSummary {
  id: number;
  topic_id: string;
  title: string;
  created_at: string;
}

export default function Dashboard() {
  const [stats, setStats] = useState<DashboardStats | null>(null);
  const [topics, setTopics] = useState<TopicInfo[]>([]);
  const [recentPosts, setRecentPosts] = useState<PostSummary[]>([]);
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);

  const showToast = (msg: string, ok: boolean) => {
    setToast({ msg, ok });
    setTimeout(() => setToast(null), 4000);
  };

  const loadData = async () => {
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
      const p = await invoke<PostSummary[]>("get_posts", { topic_id: "", limit: 10 });
      setRecentPosts(p);
    } catch (e) {
      console.error("get_posts failed", e);
    }
  };

  useEffect(() => {
    loadData();
    const interval = setInterval(loadData, 15000);
    return () => clearInterval(interval);
  }, []);

  const handleRefresh = async (topicName: string) => {
    try {
      await invoke("refresh_topic", { topic_name: topicName });
      showToast("Refreshed: " + topicName, true);
      setTimeout(loadData, 1500);
    } catch (e) {
      showToast("Refresh failed: " + e, false);
    }
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
      </div>
      <div className="page-body">
        <div className="stats-bar" style={{ marginBottom: 24 }}>
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
              <div className="stat-label">Articles Fetched</div>
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
          {topics.map((topic) => (
            <div className="topic-card" key={topic.name}>
              <div className="topic-card-header">
                <span className="topic-card-title">{topic.name}</span>
                <button
                  className="btn btn-sm"
                  onClick={() => handleRefresh(topic.name)}
                >
                  {"\u{1F504}"} Refresh
                </button>
              </div>
              <div className="topic-card-meta">
                <span>{"\u{1F4E1}"} {topic.sources.length} sources</span>
                <span>{"\u23F1"} Every {topic.interval_min} min</span>
                {topic.language && <span className="tag tag-blue">{topic.language}</span>}
              </div>
            </div>
          ))}
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
                      {new Date(post.created_at).toLocaleString()}
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
