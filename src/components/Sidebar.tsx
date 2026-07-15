import { NavLink } from "react-router-dom";

const NAV_ITEMS = [
  { path: "/", icon: "\u{1F3E0}", label: "Dashboard" },
  { path: "/topics", icon: "\u{1F4E1}", label: "Topics" },
  { path: "/settings", icon: "\u2699\uFE0F", label: "Settings" },
  { path: "/logs", icon: "\u{1F4CB}", label: "Logs" },
];

export default function Sidebar() {
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
      <div className="sidebar-status">
        <span className="status-dot" />
        Running
      </div>
    </aside>
  );
}
