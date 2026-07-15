import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface LogEntry {
  timestamp: string;
  level: string;
  topic: string;
  message: string;
}

export default function LogsPage() {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [filter, setFilter] = useState<string>("ALL");
  const [exportMsg, setExportMsg] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const loadLogs = async () => {
    try {
      const data = await invoke<LogEntry[]>("get_logs");
      setLogs(data);
    } catch (e) {
      console.error("Failed to load logs", e);
    }
  };

  const handleExport = async () => {
    try {
      const path = await invoke<string>("export_logs");
      setExportMsg("Saved: " + path);
      setTimeout(() => setExportMsg(null), 5000);
    } catch (e) {
      setExportMsg("Export failed: " + e);
      setTimeout(() => setExportMsg(null), 5000);
    }
  };

  useEffect(() => {
    loadLogs();
    const interval = setInterval(loadLogs, 5000);
    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    if (containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [logs]);

  const filteredLogs =
    filter === "ALL" ? logs : logs.filter((l) => l.level === filter);

  const levelClass = (level: string) => {
    switch (level) {
      case "INFO": return "INFO";
      case "WARN": return "WARN";
      case "ERROR": return "ERROR";
      default: return "INFO";
    }
  };

  return (
    <div className="fade-in">
      <div className="page-header">
        <h1>Logs</h1>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <button className="btn btn-sm" onClick={handleExport}>
            Export
          </button>
          <div style={{ width: 1, height: 20, background: "var(--border-color)" }} />
          {["ALL", "INFO", "WARN", "ERROR"].map((l) => (
            <button
              key={l}
              className={`btn btn-sm ${filter === l ? "btn-primary" : ""}`}
              onClick={() => setFilter(l)}
            >
              {l}
            </button>
          ))}
        </div>
      </div>
      <div className="page-body">
        {exportMsg && (
          <div
            style={{
              padding: "8px 14px",
              marginBottom: 12,
              borderRadius: "var(--radius-sm)",
              fontSize: 12,
              background: exportMsg.startsWith("Saved")
                ? "rgba(63,185,80,0.15)"
                : "rgba(248,81,73,0.15)",
              color: exportMsg.startsWith("Saved")
                ? "var(--accent-green)"
                : "var(--accent-red)",
              wordBreak: "break-all",
            }}
          >
            {exportMsg}
          </div>
        )}
        <div className="log-container" ref={containerRef}>
          {filteredLogs.length === 0 && (
            <div style={{ color: "var(--text-muted)", textAlign: "center", paddingTop: 40 }}>
              No logs yet. Logs will appear here when the pipeline runs.
            </div>
          )}
          {filteredLogs.map((entry, i) => (
            <div className="log-entry" key={i}>
              <span className="log-time">{entry.timestamp}</span>
              <span className={`log-level ${levelClass(entry.level)}`}>
                {entry.level}
              </span>
              <span className="log-topic" style={{ color: "var(--accent-blue)", minWidth: 100 }}>
                [{entry.topic}]
              </span>
              <span className="log-message">{entry.message}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
