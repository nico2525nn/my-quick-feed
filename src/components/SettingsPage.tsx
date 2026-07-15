import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface AppConfig {
  discord: {
    token: string;
    forum_channel_id: string;
  };
  ai: {
    mode: string;
    agent_command: string | null;
    agent_timeout_sec: number | null;
    api_key: string | null;
    model: string | null;
    base_url: string | null;
  };
  topics: unknown[];
}

export default function SettingsPage() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{ type: string; text: string } | null>(null);

  useEffect(() => {
    loadConfig();
  }, []);

  const loadConfig = async () => {
    try {
      const c = await invoke<AppConfig>("get_config");
      setConfig(c);
    } catch (e) {
      console.error("Failed to load config", e);
    }
  };

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    setMessage(null);
    try {
      await invoke("update_config", { config });
      setMessage({ type: "success", text: "Settings saved successfully. Scheduler restarted." });
    } catch (e) {
      setMessage({ type: "error", text: `Failed to save: ${e}` });
    } finally {
      setSaving(false);
    }
  };

  if (!config) return <div className="page-body">Loading...</div>;

  return (
    <div className="fade-in">
      <div className="page-header">
        <h1>Settings</h1>
        <button className="btn btn-primary" onClick={handleSave} disabled={saving}>
          {saving ? "Saving..." : "Save Settings"}
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
              border: `1px solid ${
                message.type === "success"
                  ? "var(--accent-green)"
                  : "var(--accent-red)"
              }`,
            }}
          >
            {message.text}
          </div>
        )}

        {/* Discord Settings */}
        <div className="settings-section">
          <h2>Discord Integration</h2>
          <div className="settings-card">
            <div className="form-group">
              <label>Bot Token</label>
              <input
                className="form-input"
                type="password"
                value={config.discord.token}
                onChange={(e) =>
                  setConfig({
                    ...config,
                    discord: { ...config.discord, token: e.target.value },
                  })
                }
                placeholder="DISCORD_BOT_TOKEN"
              />
            </div>
            <div className="form-group">
              <label>Forum Channel ID</label>
              <input
                className="form-input"
                value={config.discord.forum_channel_id}
                onChange={(e) =>
                  setConfig({
                    ...config,
                    discord: { ...config.discord, forum_channel_id: e.target.value },
                  })
                }
                placeholder="1234567890123456789"
              />
            </div>
          </div>
        </div>

        {/* AI Settings */}
        <div className="settings-section">
          <h2>AI Configuration</h2>
          <div className="settings-card">
            <div className="form-group">
              <label>Mode</label>
              <select
                className="form-select"
                value={config.ai.mode}
                onChange={(e) =>
                  setConfig({
                    ...config,
                    ai: { ...config.ai, mode: e.target.value },
                  })
                }
              >
                <option value="agent">Agent Mode (OMP/OpenCode)</option>
                <option value="direct">Direct Mode (API)</option>
              </select>
            </div>

            {/* Model — 両モード対応 */}
            <div className="form-group">
              <label>Model</label>
              <input
                className="form-input"
                value={config.ai.model ?? "gpt-4o-mini"}
                onChange={(e) =>
                  setConfig({
                    ...config,
                    ai: { ...config.ai, model: e.target.value },
                  })
                }
                placeholder="gpt-4o-mini"
              />
            </div>
            <div className="form-row">
              <div className="form-group">
                <label>Provider</label>
                <input
                  className="form-input"
                  value={config.ai.base_url ?? "https://openrouter.ai/api/v1"}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      ai: { ...config.ai, base_url: e.target.value || null },
                    })
                  }
                  placeholder="https://openrouter.ai/api/v1"
                />
              </div>
              <div className="form-group">
                <label>API Key</label>
                <input
                  className="form-input"
                  type="password"
                  value={config.ai.api_key ?? ""}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      ai: { ...config.ai, api_key: e.target.value || null },
                    })
                  }
                  placeholder="sk-or-xxxxx"
                />
              </div>
            </div>

            {config.ai.mode === "agent" && (
              <>
                <div className="form-group">
                  <label>Agent Command</label>
                  <select
                    className="form-select"
                    value={config.ai.agent_command ?? "omp"}
                    onChange={(e) =>
                      setConfig({
                        ...config,
                        ai: { ...config.ai, agent_command: e.target.value },
                      })
                    }
                  >
                    <option value="omp">OMP (Oh My Pi)</option>
                    <option value="opencode">OpenCode</option>
                  </select>
                </div>
                <div className="form-group">
                  <label>Agent Timeout (seconds)</label>
                  <input
                    className="form-input"
                    type="number"
                    min={10}
                    value={config.ai.agent_timeout_sec ?? 120}
                    onChange={(e) =>
                      setConfig({
                        ...config,
                        ai: {
                          ...config.ai,
                          agent_timeout_sec: parseInt(e.target.value) || 120,
                        },
                      })
                    }
                  />
                </div>
              </>
            )}

          </div>
        </div>
      </div>
    </div>
  );
}
