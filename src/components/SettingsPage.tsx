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
    provider: string | null;
    base_url: string | null;
    thinking_level: string | null;
  };
  topics: unknown[];
}

export default function SettingsPage() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [message, setMessage] = useState<{ type: string; text: string } | null>(null);

  useEffect(() => {
    loadConfig();
    loadAutostart();
  }, []);

  const loadConfig = async () => {
    try {
      const c = await invoke<AppConfig>("get_config");
      setConfig(c);
    } catch (e) {
      console.error("Failed to load config", e);
    }
  };

  const loadAutostart = async () => {
    try {
      const v = await invoke<boolean>("get_autostart");
      setAutostart(v);
    } catch (e) {
      console.error("get_autostart failed", e);
    }
  };

  const toggleAutostart = async () => {
    if (autostart === null) return;
    const next = !autostart;
    try {
      await invoke("set_autostart", { enabled: next });
      setAutostart(next);
      setMessage({
        type: "success",
        text: next ? "スタートアップ登録しました" : "スタートアップ登録を解除しました",
      });
    } catch (e) {
      setMessage({ type: "error", text: `スタートアップ設定失敗: ${e}` });
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
                <option value="agent">Agent Mode (OMP)</option>
                <option value="direct">Direct Mode (API)</option>
              </select>
            </div>

            {/* Model — 両モード対応 */}
            <div className="form-group">
              <label>Model</label>
              <input
                className="form-input"
                value={config.ai.model ?? ""}
                onChange={(e) =>
                  setConfig({
                    ...config,
                    ai: { ...config.ai, model: e.target.value || null },
                  })
                }
                placeholder="mimo-v2.5"
              />
            </div>
            <div className="form-row">
              <div className="form-group">
                <label>Provider</label>
                <input
                  className="form-input"
                  value={config.ai.provider ?? ""}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      ai: { ...config.ai, provider: e.target.value || null },
                    })
                  }
                  placeholder="opencode-go"
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
                  placeholder="sk-xxxxx"
                />
              </div>
            </div>

            {config.ai.mode === "direct" && (
              <div className="form-group">
                <label>Base URL (Directモード用)</label>
                <input
                  className="form-input"
                  value={config.ai.base_url ?? ""}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      ai: { ...config.ai, base_url: e.target.value || null },
                    })
                  }
                  placeholder="https://openrouter.ai/api/v1"
                />
              </div>
            )}

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
                <div className="form-group">
                  <label>Thinking Level（思考の深さ）</label>
                  <select
                    className="form-select"
                    value={config.ai.thinking_level ?? "high"}
                    onChange={(e) =>
                      setConfig({
                        ...config,
                        ai: { ...config.ai, thinking_level: e.target.value || null },
                      })
                    }
                  >
                    <option value="high">high（深く考える・推奨）</option>
                    <option value="medium">medium</option>
                    <option value="low">low（浅く速く）</option>
                    <option value="auto">auto（モデル任せ）</option>
                  </select>
                  <div style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 4 }}>
                    mimo-v2.5 は low / medium / high のみ対応。high は生成が遅くなる代わりに
                    記事の正確性が上がります。
                  </div>
                </div>
              </>
            )}
          </div>
        </div>

        {/* Startup Settings */}
        <div className="settings-section">
          <h2>Startup</h2>
          <div className="settings-card">
            <label className="form-checkbox">
              <input
                type="checkbox"
                checked={autostart === true}
                disabled={autostart === null}
                onChange={toggleAutostart}
                title={autostart === null ? "Loading..." : undefined}
              />
              Windows起動時に自動起動する
            </label>
          </div>
        </div>
      </div>
    </div>
  );
}
