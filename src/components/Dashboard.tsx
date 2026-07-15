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

  const loadData = async () => {
    try {
      const [s, t, p] = await Promise.all([
        invoke<DashboardStats>("get_stats"),
        invoke<TopicInfo[]>("get_topics"),
        invoke<PostSummary[]>("get_posts", { topicId: "", limit: 10 }),
      ]);
      setStats(s);
      setTopics(t);
      setRecentPosts(p);
    } catch (e) {
      console.error("Failed to load dashboard data", e);
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
    } catch (e) {
      console.error("Refresh failed", e);
    }
  };

  return (
    <div className="fade-in">
      <div className="page-header">
        <h1>Dashboard</h1>
      </div>
      <div className="page-body">
        {/* Stats */}
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

        {/* Topics Overview */}
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

        {/* Recent Posts */}
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
