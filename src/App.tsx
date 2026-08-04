import { HashRouter, Routes, Route } from "react-router-dom";
import Sidebar from "./components/Sidebar";
import Dashboard from "./components/Dashboard";
import TopicsPage from "./components/TopicsPage";
import SettingsPage from "./components/SettingsPage";
import LogsPage from "./components/LogsPage";

export default function App() {
  return (
    // Tauri では非ルートパスのリロードで 404 になるため HashRouter を使用
    <HashRouter>
      <div className="app-layout">
        <Sidebar />
        <main className="main-content">
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/topics" element={<TopicsPage />} />
            <Route path="/settings" element={<SettingsPage />} />
            <Route path="/logs" element={<LogsPage />} />
          </Routes>
        </main>
      </div>
    </HashRouter>
  );
}
