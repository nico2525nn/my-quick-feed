use rusqlite::{params, Connection};
use std::path::PathBuf;
use parking_lot::Mutex;
use tracing::info;

use crate::errors::AppResult;

#[derive(Debug, Clone)]
pub struct SeenItem {
    pub id: i64,
    pub topic_id: String,
    pub source_url: String,
    pub title: Option<String>,
    pub fetched_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Post {
    pub id: i64,
    pub topic_id: String,
    pub title: String,
    pub content: String,
    pub image_url: Option<String>,
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
            CREATE TABLE IF NOT EXISTS seen_items (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                topic_id    TEXT NOT NULL,
                source_url  TEXT NOT NULL,
                title       TEXT,
                fetched_at  TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(topic_id, source_url)
            );

            CREATE TABLE IF NOT EXISTS posts (
                id                 INTEGER PRIMARY KEY AUTOINCREMENT,
                topic_id           TEXT NOT NULL,
                title              TEXT NOT NULL,
                content            TEXT NOT NULL,
                image_url          TEXT,
                discord_message_id TEXT,
                discord_thread_id  TEXT,
                created_at         TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS reactions (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                post_id     INTEGER NOT NULL REFERENCES posts(id),
                emoji       TEXT NOT NULL,
                user_id     TEXT NOT NULL,
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_seen_items_topic ON seen_items(topic_id, source_url);
            CREATE INDEX IF NOT EXISTS idx_posts_topic ON posts(topic_id);
            ",
        )?;
        info!("Database migration completed");
        Ok(())
    }

    pub fn is_seen(&self, topic_id: &str, source_url: &str) -> AppResult<bool> {
        let conn = self.conn.lock();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM seen_items WHERE topic_id = ?1 AND source_url = ?2",
            params![topic_id, source_url],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn mark_seen(
        &self,
        topic_id: &str,
        source_url: &str,
        title: Option<&str>,
    ) -> AppResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR IGNORE INTO seen_items (topic_id, source_url, title) VALUES (?1, ?2, ?3)",
            params![topic_id, source_url, title],
        )?;
        Ok(())
    }

    pub fn insert_post(
        &self,
        topic_id: &str,
        title: &str,
        content: &str,
        image_url: Option<&str>,
    ) -> AppResult<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO posts (topic_id, title, content, image_url) VALUES (?1, ?2, ?3, ?4)",
            params![topic_id, title, content, image_url],
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
        let total_articles: i64 =
            conn.query_row("SELECT COUNT(*) FROM seen_items", [], |row| row.get(0))?;
        let total_posts: i64 =
            conn.query_row("SELECT COUNT(*) FROM posts", [], |row| row.get(0))?;
        Ok(DashboardStats {
            total_topics: 0, // calculated by caller
            total_articles,
            total_posts,
        })
    }

    pub fn get_recent_seen_count(&self, topic_id: &str, since_minutes: i64) -> AppResult<i64> {
        let conn = self.conn.lock();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM seen_items WHERE topic_id = ?1 AND fetched_at > datetime('now', ?2)",
            params![topic_id, format!("-{} minutes", since_minutes)],
            |row| row.get(0),
        )?;
        Ok(count)
    }
}
