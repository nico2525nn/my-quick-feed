import { NavLink, useNavigate } from "react-router-dom";

const NAV_ITEMS = [
  { path: "/", icon: "\u{1F3E0}", label: "Dashboard" },
  { path: "/topics", icon: "\u{1F4E1}", label: "Topics" },
  { path: "/logs", icon: "\u{1F4CB}", label: "Logs" },
];

export default function Sidebar() {
  const navigate = useNavigate();

  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <span className="logo">{"\u{1F4EA}"}</span>
        My Quick Feed
      </div>
      <nav className="sidebar-nav">
        {NAV_ITEMS.map((item) => (
          <NavLink
            key={item.path}
            to={item.path}
            end={item.path === "/"}
            className={({ isActive }) => (isActive ? "active" : "")}
          >
            <span className="nav-icon">{item.icon}</span>
            {item.label}
          </NavLink>
        ))}
      </nav>

      {/* Settings: サイドバー下部の独立ボタン */}
      <button className="sidebar-settings" onClick={() => navigate("/settings")}>
        <span className="nav-icon">{"\u2699\uFE0F"}</span>
        Settings
      </button>

      <div className="sidebar-status">
        <span className="status-dot" />
        Running
      </div>
    </aside>
  );
}
