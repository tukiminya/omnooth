import { useEffect, useRef, useState } from 'react';
import { Icon } from './Icon';
import type { LogSnapshot } from './contracts';

type LogLevelFilter = 'all' | LogSnapshot['entries'][number]['level'];

interface LogDialogProps {
  getLogs: () => Promise<LogSnapshot>;
  onClose: () => void;
  t: (key: string, options?: Record<string, unknown>) => string;
}

export function LogDialog({ getLogs, onClose, t }: LogDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const refreshRef = useRef<() => void>(() => {});
  const [snapshot, setSnapshot] = useState<LogSnapshot | null>(null);
  const [level, setLevel] = useState<LogLevelFilter>('all');
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [loadError, setLoadError] = useState('');

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog && !dialog.open) dialog.showModal();
    return () => { if (dialog?.open) dialog.close(); };
  }, []);

  useEffect(() => {
    let disposed = false;
    let inFlight = false;
    let hasLoaded = false;
    const refresh = async () => {
      if (disposed || inFlight) return;
      inFlight = true;
      if (hasLoaded) setRefreshing(true);
      else setLoading(true);
      try {
        const next = await getLogs();
        if (!disposed) {
          setSnapshot(next);
          setLoadError('');
        }
      } catch {
        if (!disposed) setLoadError(t('logsLoadError'));
      } finally {
        inFlight = false;
        hasLoaded = true;
        if (!disposed) {
          setLoading(false);
          setRefreshing(false);
        }
      }
    };
    refreshRef.current = () => { void refresh(); };
    void refresh();
    const interval = window.setInterval(() => { void refresh(); }, 1000);
    return () => { disposed = true; window.clearInterval(interval); };
  }, [getLogs, t]);

  const entries = snapshot?.entries ?? [];
  const visibleEntries = entries.filter((entry) => level === 'all' || entry.level === level);

  return <dialog ref={dialogRef} className="log-dialog" aria-labelledby="log-dialog-title" onCancel={(event) => { event.preventDefault(); onClose(); }} onClick={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <header className="dialog-heading log-dialog-heading">
      <div><h2 id="log-dialog-title">{t('logsTitle')}</h2><p>{t('logsDescription')}</p></div>
      <button className="icon-button" type="button" onClick={onClose} aria-label={t('close')}><Icon name="close" size={20} /></button>
    </header>
    <div className="log-toolbar">
      <label className="log-filter" htmlFor="log-level-filter"><span>{t('logsFilter')}</span><select id="log-level-filter" value={level} onChange={(event) => setLevel(event.target.value as LogLevelFilter)}>
        <option value="all">{t('logsAll')}</option>
        <option value="info">{t('logsInfo')}</option>
        <option value="warn">{t('logsWarn')}</option>
        <option value="error">{t('logsError')}</option>
      </select></label>
      {snapshot?.sessionId && <span className="log-session mono" title={snapshot.sessionId}>{t('logsSession', { id: snapshot.sessionId })}</span>}
      <button className="button secondary log-refresh" type="button" onClick={() => refreshRef.current()} disabled={loading || refreshing}>
        <Icon name="refresh" size={16} />{refreshing ? t('logsRefreshing') : t('logsRefresh')}
      </button>
    </div>
    <LogEntries entries={visibleEntries} hasEntries={entries.length > 0} filter={level} loading={loading} error={loadError} t={t} />
  </dialog>;
}

function LogEntries({ entries, hasEntries, filter, loading, error, t }: { entries: LogSnapshot['entries']; hasEntries: boolean; filter: LogLevelFilter; loading: boolean; error: string; t: LogDialogProps['t'] }) {
  if (loading) return <div className="logs-state" role="status"><span className="spinner" />{t('logsLoading')}</div>;
  return <div className="logs-content">
    {error && <div className="alert error logs-error" role="alert">{error}</div>}
    {entries.length ? <ol className="log-list" aria-label={t('logsTitle')}>
      {entries.map((entry) => <li className={`log-entry level-${entry.level}`} key={entry.id}>
        <div className="log-entry-meta"><span className="log-level-tag">{levelLabel(entry.level, t)}</span><time dateTime={entry.timestamp}>{entry.timestamp}</time><span className="log-operation">{entry.operation}</span>{entry.correlationId && <span className="log-correlation mono" title={entry.correlationId}>{entry.correlationId}</span>}</div>
        <p className="log-message">{entry.message}</p>
      </li>)}
    </ol> : <div className="logs-state" role="status">{hasEntries && filter !== 'all' ? t('logsFilteredEmpty') : t('logsEmpty')}</div>}
  </div>;
}

function levelLabel(level: LogSnapshot['entries'][number]['level'], t: LogDialogProps['t']) {
  switch (level) {
    case 'info': return t('logsInfo');
    case 'warn': return t('logsWarn');
    case 'error': return t('logsError');
  }
}
