import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { AiSession, AiSessionMessage } from '../types';

interface Props {
  open: boolean;
  onClose: () => void;
  session: AiSession | null;
  projectPath: string;
}

function formatTime(iso: string): string {
  if (!iso) return '';
  const d = new Date(iso);
  if (isNaN(d.getTime())) return '';
  return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`;
}

export function SessionViewerModal({ open, onClose, session, projectPath }: Props) {
  const [messages, setMessages] = useState<AiSessionMessage[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!open || !session) return;
    setLoading(true);
    setError('');
    setMessages([]);

    invoke<AiSessionMessage[]>('get_ai_session_content', {
      sessionType: session.sessionType,
      sessionId: session.id,
      projectPath,
    })
      .then(setMessages)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [open, session, projectPath]);

  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', h);
    return () => window.removeEventListener('keydown', h);
  }, [open, onClose]);

  if (!open || !session) return null;

  const typeName = session.sessionType === 'claude' ? 'Claude' : 'Codex';
  const typeColor = session.sessionType === 'claude' ? 'var(--color-ai)' : 'var(--color-success)';

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center select-text" onClick={onClose}>
      <div className="absolute inset-0 bg-black/50 backdrop-blur-sm" />
      <div
        className="relative flex flex-col overflow-hidden bg-[var(--bg-surface)] border border-[var(--border-strong)] rounded-[var(--radius-md)] shadow-[var(--shadow-overlay)] animate-slide-in"
        style={{ width: '90vw', height: '80vh' }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* 工具栏 */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)] flex-shrink-0">
          <div className="flex items-center gap-2 min-w-0">
            <span
              className="flex-shrink-0 text-xs font-bold px-1.5 py-0.5 rounded"
              style={{ backgroundColor: typeColor + '22', color: typeColor }}
            >
              {typeName}
            </span>
            <span className="text-base font-medium text-[var(--text-primary)] truncate">
              {session.title}
            </span>
            <span className="text-xs text-[var(--text-muted)] flex-shrink-0">
              {messages.length > 0 && `${messages.length} 条消息`}
            </span>
          </div>
          <button
            className="text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors text-lg leading-none flex-shrink-0 ml-2"
            onClick={onClose}
          >
            ✕
          </button>
        </div>

        {/* 内容区 */}
        <div className="flex-1 overflow-auto bg-[var(--bg-base)] p-4 space-y-4">
          {loading && (
            <div className="flex items-center justify-center h-full text-[var(--text-muted)]">加载中...</div>
          )}
          {error && (
            <div className="flex items-center justify-center h-full text-[var(--color-error)]">{error}</div>
          )}
          {!loading && !error && messages.length === 0 && (
            <div className="flex items-center justify-center h-full text-[var(--text-muted)]">无消息内容</div>
          )}

          {messages.map((msg, i) => (
            <div key={i}>
              {/* 角色标签 + 时间 */}
              <div className="flex items-center gap-2 mb-1">
                <span
                  className="text-xs font-semibold"
                  style={{ color: msg.role === 'user' ? 'var(--text-secondary)' : typeColor }}
                >
                  {msg.role === 'user' ? 'User' : 'Assistant'}
                </span>
                {msg.timestamp && (
                  <span className="text-[10px] text-[var(--text-muted)]">{formatTime(msg.timestamp)}</span>
                )}
              </div>

              {/* 消息内容 */}
              <div
                className={`rounded-[var(--radius-sm)] px-3 py-2 text-sm ${
                  msg.role === 'user'
                    ? 'bg-[var(--border-subtle)] text-[var(--text-primary)]'
                    : 'bg-[var(--bg-surface)] text-[var(--text-primary)] border border-[var(--border-default)]'
                }`}
              >
                {msg.role === 'assistant' ? (
                  <div className="md-preview">
                    <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.content}</ReactMarkdown>
                  </div>
                ) : (
                  <div style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>{msg.content}</div>
                )}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
