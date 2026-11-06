import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ArchiveContents } from './contracts';
import { formatBytes } from './formatBytes';

/** A bounded, searchable directory listing; no member is extracted to display it. */
export function ArchiveContentsList({ entries }: { entries: ArchiveContents['entries'] }) {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  const [limit, setLimit] = useState(100);
  const matching = entries.filter((entry) => entry.relativePath.toLowerCase().includes(query.toLowerCase()));
  return <section className="archive-contents" aria-label={t('archiveContents')}>
    <div className="selection-heading"><h3>{t('archiveContents')}</h3><span>{t('archiveEntryCount', { count: entries.length })}</span></div>
    <label className="archive-search"><span>{t('searchArchiveContents')}</span><input type="search" value={query} onChange={(event) => { setQuery(event.target.value); setLimit(100); }} /></label>
    {matching.length ? <ul>{matching.slice(0, limit).map((entry) => <li key={entry.id}>
      <span className="mono">{entry.relativePath}{entry.kind === 'directory' ? '/' : ''}</span>
      <small>{entry.encrypted ? t('archiveEncrypted') : entry.kind === 'directory' ? t('archiveDirectory') : formatBytes(entry.sizeBytes)}</small>
    </li>)}</ul> : <p>{t(entries.length ? 'noArchiveContentsMatch' : 'archiveEmpty')}</p>}
    {matching.length > limit && <button className="text-button" onClick={() => setLimit((value) => value + 100)}>{t('showMoreArchiveContents', { count: matching.length - limit })}</button>}
  </section>;
}
