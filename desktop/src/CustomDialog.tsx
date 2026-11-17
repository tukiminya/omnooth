import { useEffect, useRef, useState } from 'react';
import { client } from './client';
import type { LibraryItem, SaveCustomInput } from './contracts';

export function CustomDialog({ items, item, busy, onClose, onSave, t }: { items: LibraryItem[]; item: LibraryItem | null; busy: boolean; onClose: () => void; onSave: (input: SaveCustomInput) => void; t: (key: string) => string }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [input, setInput] = useState<SaveCustomInput>({ packageId: item?.kind === 'custom' ? item.id : null, name: item?.kind === 'custom' ? item.title : '', description: item?.description ?? '', displayParentId: item?.kind === 'custom' ? item.displayParentId ?? '' : item?.id ?? items[0]?.id ?? '', unitypackagePaths: [], requiredVersionIds: item?.kind === 'custom' ? item.versions[0]?.requiredVersionIds ?? [] : [] });
  const [error, setError] = useState(false);
  useEffect(() => { dialog.current?.showModal(); }, []);
  return <dialog ref={dialog} className="metadata-dialog" aria-label={t('customPackage')} onCancel={event => { event.preventDefault(); onClose(); }}>
    <form onSubmit={event => { event.preventDefault(); onSave(input); }}>
      <h2>{t('customPackage')}</h2><p>{t('customHint')}</p>
      <label>{t('itemTitle')}<input required maxLength={512} value={input.name} onChange={event => setInput({ ...input, name: event.target.value })} /></label>
      <label>{t('description')}<textarea maxLength={32768} value={input.description} onChange={event => setInput({ ...input, description: event.target.value })} /></label>
      <label>{t('displayParent')}<select value={input.displayParentId} onChange={event => setInput({ ...input, displayParentId: event.target.value })}>{items.filter(candidate => candidate.id !== input.packageId).map(candidate => <option key={candidate.id} value={candidate.id}>{candidate.title}</option>)}</select></label>
      <fieldset><legend>{t('exactDependencies')}</legend>{items.filter(candidate => candidate.id !== input.packageId).map(candidate => <label key={candidate.id}>{candidate.title}<select aria-label={`${t('dependencyVersion')}: ${candidate.title}`} value={candidate.versions.find(version => input.requiredVersionIds.includes(version.id))?.id ?? ''} onChange={event => setInput({ ...input, requiredVersionIds: [...input.requiredVersionIds.filter(id => !candidate.versions.some(version => version.id === id)), ...(event.target.value ? [event.target.value] : [])] })}><option value="">{t('noDependency')}</option>{candidate.versions.map(version => <option value={version.id} key={version.id}>{version.label}</option>)}</select></label>)}</fieldset>
      <button className="button secondary" type="button" disabled={busy} onClick={async () => { try { const paths = await client.chooseCustomFiles(); if (paths.length) setInput({ ...input, unitypackagePaths: paths }); } catch { setError(true); } }}>{t('chooseCustomFiles')}</button>
      <ul>{input.unitypackagePaths.map(path => <li key={path} className="mono">{path.split(/[\\/]/).at(-1)}</li>)}</ul>{input.packageId && !input.unitypackagePaths.length && <p>{t('keepCustomFiles')}</p>}{error && <p role="alert">{t('operationError')}</p>}
      <footer><button className="button secondary" type="button" disabled={busy} onClick={onClose}>{t('cancel')}</button><button className="button primary" disabled={busy || !input.displayParentId || !input.requiredVersionIds.length || (!input.packageId && !input.unitypackagePaths.length)}>{t('save')}</button></footer>
    </form>
  </dialog>;
}
