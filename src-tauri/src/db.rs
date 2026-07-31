use rusqlite::{params, Connection};
use std::path::PathBuf;
use parking_lot::Mutex;
use tracing::info;

use crate::errors::AppResult;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Post {
    pub id: i64,
    pub topic_id: String,
    pub title: String,
    pub content: String,
    pub image_url: Option<String>,
    pub tags: Option<String>,
    pub discord_message_id: Option<String>,
    pub discord_thread_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PostSummary {
    pub id: i64,
    pub topic_id: String,
    pub title: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DashboardStats {
    pub total_topics: usize,
    pub total_articles: i64,
    pub total_posts: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub topic: String,
    pub message: String,
}

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn new(db_path: PathBuf) -> AppResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(&db_path)?;
        let db = Self { conn: Mutex::new(conn) };
        db.migrate()?;
        info!("Database initialized at {:?}", db_path);
        Ok(db)
    }

    fn migrate(&self) -> AppResult<()> {
        let conn = self.conn.lock();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS posts (
                id                 INTEGER PRIMARY KEY AUTOINCREMENT,
                topic_id           TEXT NOT NULL,
                title              TEXT NOT NULL,
                content            TEXT NOT NULL,
                image_url          TEXT,
                tags               TEXT,
                discord_message_id TEXT,
                discord_thread_id  TEXT,
                created_at         TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS topic_threads (
                topic_id    TEXT PRIMARY KEY,
                thread_id   TEXT NOT NULL,
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS reactions (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                post_id     INTEGER NOT NULL REFERENCES posts(id),
                emoji       TEXT NOT NULL,
                user_id     TEXT NOT NULL,
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_posts_topic ON posts(topic_id);
            ",
        )?;
        // 既存DB（tags列なし）へのマイグレーション
        let has_tags: bool = conn
            .prepare("PRAGMA table_info(posts)")
            .and_then(|mut stmt| {
                stmt.query_map([], |row| row.get::<_, String>(1))
                    .map(|rows| rows.filter_map(|r| r.ok()).any(|c| c == "tags"))
            })?;
        if !has_tags {
            conn.execute_batch("ALTER TABLE posts ADD COLUMN tags TEXT;")?;
            info!("Migrated: added posts.tags column");
        }
        info!("Database migration completed");
        Ok(())
    }

    pub fn get_topic_thread(&self, topic_id: &str) -> AppResult<Option<String>> {
        let conn = self.conn.lock();
        let result = conn.query_row(
            "SELECT thread_id FROM topic_threads WHERE topic_id = ?1",
            params![topic_id],
            |row| row.get(0),
        );
        match result {
            Ok(thread_id) => Ok(Some(thread_id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set_topic_thread(&self, topic_id: &str, thread_id: &str) -> AppResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO topic_threads (topic_id, thread_id) VALUES (?1, ?2)",
            params![topic_id, thread_id],
        )?;
        Ok(())
    }

    /// 直近N日分の投稿タイトルを取得（重複防止用にプロンプトへ埋め込む）
    pub fn get_recent_titles(&self, topic_id: &str, days: i64) -> AppResult<Vec<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT title FROM posts
             WHERE topic_id = ?1 AND created_at >= datetime('now', ?2)
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![topic_id, format!("-{} days", days)], |row| {
            row.get::<_, String>(0)
        })?;
        let mut titles = Vec::new();
        for row in rows {
            titles.push(row?);
        }
        Ok(titles)
    }

    pub fn insert_post(
        &self,
        topic_id: &str,
        title: &str,
        content: &str,
        image_url: Option<&str>,
        tags: Option<&str>,
    ) -> AppResult<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO posts (topic_id, title, content, image_url, tags) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![topic_id, title, content, image_url, tags],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_post_discord(
        &self,
        post_id: i64,
        message_id: &str,
        thread_id: Option<&str>,
    ) -> AppResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE posts SET discord_message_id = ?1, discord_thread_id = ?2 WHERE id = ?3",
            params![message_id, thread_id, post_id],
        )?;
        Ok(())
    }

    pub fn get_posts(&self, topic_id: &str, limit: i64) -> AppResult<Vec<PostSummary>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, topic_id, title, created_at FROM posts WHERE topic_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![topic_id, limit], |row| {
            Ok(PostSummary {
                id: row.get(0)?,
                topic_id: row.get(1)?,
                title: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        let mut posts = Vec::new();
        for row in rows {
            posts.push(row?);
        }
        Ok(posts)
    }

    pub fn get_all_posts(&self, limit: i64) -> AppResult<Vec<PostSummary>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, topic_id, title, created_at FROM posts ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(PostSummary {
                id: row.get(0)?,
                topic_id: row.get(1)?,
                title: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        let mut posts = Vec::new();
        for row in rows {
            posts.push(row?);
        }
        Ok(posts)
    }

    pub fn get_stats(&self) -> AppResult<DashboardStats> {
        let conn = self.conn.lock();
        let total_posts: i64 =
            conn.query_row("SELECT COUNT(*) FROM posts", [], |row| row.get(0))?;
        Ok(DashboardStats {
            total_topics: 0, // calculated by caller
            total_articles: total_posts,
            total_posts,
        })
    }

    /// トピックごとの記事数と最終投稿時刻（Dashboard タイル用）
    pub fn get_topic_stats(&self) -> AppResult<Vec<TopicStat>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT topic_id, COUNT(*) as cnt, MAX(created_at) as last
             FROM posts GROUP BY topic_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(TopicStat {
                topic_id: row.get(0)?,
                post_count: row.get(1)?,
                last_post_at: row.get(2)?,
            })
        })?;
        let mut stats = Vec::new();
        for row in rows {
            stats.push(row?);
        }
        Ok(stats)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TopicStat {
    pub topic_id: String,
    pub post_count: i64,
    pub last_post_at: Option<String>,
}
