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
  const containerRef = useRef<HTMLDivElement>(null);

  const loadLogs = async () => {
    try {
      const data = await invoke<LogEntry[]>("get_logs");
      setLogs(data);
    } catch (e) {
      console.error("Failed to load logs", e);
    }
  };

  useEffect(() => {
    loadLogs();
    const interval = setInterval(loadLogs, 5000);
    return () => clearInterval(interval);
  }, []);

  // Auto-scroll to bottom on new logs
  useEffect(() => {
    if (containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [logs]);

  const filteredLogs =
    filter === "ALL" ? logs : logs.filter((l) => l.level === filter);

  const levelClass = (level: string) => {
    switch (level) {
      case "INFO":
        return "INFO";
      case "WARN":
        return "WARN";
      case "ERROR":
        return "ERROR";
      default:
        return "INFO";
    }
  };

  return (
    <div className="fade-in">
      <div className="page-header">
        <h1>Logs</h1>
        <div style={{ display: "flex", gap: 8 }}>
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
