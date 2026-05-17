import { useAppStore } from '../store';
import { activateProjectTerminal } from '../utils/externalTerminal';

export function ToastContainer() {
  const notifications = useAppStore((s) => s.notifications);
  const dismissNotification = useAppStore((s) => s.dismissNotification);
  const setActiveProject = useAppStore((s) => s.setActiveProject);
  const config = useAppStore((s) => s.config);

  // 最多同时渲染 5 个，超出排队
  const visible = notifications.slice(0, 5);

  if (visible.length === 0) return null;

  return (
    <div className="toast-stack">
      {visible.map((n) => (
        <div
          key={n.id}
          className="toast-card"
          onClick={() => {
            setActiveProject(n.projectId);
            if (config.companionMode) {
              const project = config.projects.find((p) => p.id === n.projectId);
              if (project) {
                void activateProjectTerminal(project.id, project.path, project.name).catch(() => {});
              }
            }
            dismissNotification(n.id);
          }}
        >
          <div className="toast-icon">✓</div>
          <div className="toast-body">
            <div className="toast-name">{n.projectName}</div>
            <div className="toast-desc">AI 已完成 · 点击查看</div>
          </div>
          <div
            className="toast-close"
            onClick={(e) => {
              e.stopPropagation();
              dismissNotification(n.id);
            }}
          >×</div>
        </div>
      ))}
    </div>
  );
}
