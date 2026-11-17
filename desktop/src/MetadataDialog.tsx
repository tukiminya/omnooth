import { useEffect, useRef, useState } from 'react';
import type { LibraryItem } from './contracts';

type Props = { item: LibraryItem; busy: boolean; onClose: () => void; onSave: (title: string, details: NonNullable<LibraryItem['details']>) => void; t: (key: string) => string };
export function MetadataDialog({ item, busy, onClose, onSave, t }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [title, setTitle] = useState(item.title);
  const [details, setDetails] = useState<NonNullable<LibraryItem['details']>>(item.details ?? { description: '', shop: '', category: '', tags: [], ageRating: 'unknown', isStarred: false, releaseNotes: null });
  const [tags, setTags] = useState(details.tags.join(', '));
  useEffect(() => { dialog.current?.showModal(); }, []);
  return <dialog ref={dialog} className="metadata-dialog" onCancel={event => { event.preventDefault(); onClose(); }} aria-label={t('editMetadata')}>
    <form onSubmit={event => { event.preventDefault(); onSave(title, { ...details, tags: tags.split(',').map(tag => tag.trim()).filter(Boolean) }); }}>
      <h2>{t('editMetadata')}</h2>
      <label>{t('itemTitle')}<input required maxLength={512} value={title} onChange={event => setTitle(event.target.value)} /></label>
      <label>{t('shop')}<input maxLength={512} value={details.shop} onChange={event => setDetails({ ...details, shop: event.target.value })} /></label>
      <label>{t('category')}<input maxLength={512} value={details.category} onChange={event => setDetails({ ...details, category: event.target.value })} /></label>
      <label>{t('tags')}<input value={tags} onChange={event => setTags(event.target.value)} /></label>
      <label>{t('ageRating')}<select value={details.ageRating || 'unknown'} onChange={event => setDetails({ ...details, ageRating: event.target.value })}><option value="unknown">{t('unknownAge')}</option><option value="all">{t('allAges')}</option><option value="r18">R18</option></select></label>
      <label><input type="checkbox" checked={details.isStarred} onChange={event => setDetails({ ...details, isStarred: event.target.checked })} />{t('starred')}</label>
      <label>{t('description')}<textarea maxLength={131072} rows={5} value={details.description} onChange={event => setDetails({ ...details, description: event.target.value })} /></label>
      <footer><button type="button" className="button secondary" disabled={busy} onClick={onClose}>{t('cancel')}</button><button className="button primary" disabled={busy || !title.trim()}>{t('save')}</button></footer>
    </form>
  </dialog>;
}
