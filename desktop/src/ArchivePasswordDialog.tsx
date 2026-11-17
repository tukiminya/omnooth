import { useEffect, useRef, useState } from 'react';
export function ArchivePasswordDialog({ busy, onClose, onExtract, t }: { busy: boolean; onClose: () => void; onExtract: (password: string) => Promise<boolean>; t: (key: string) => string }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [password, setPassword] = useState('');
  useEffect(() => { dialog.current?.showModal(); }, []);
  return <dialog ref={dialog} className="metadata-dialog" aria-label={t('archivePasswordTitle')} onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}><form onSubmit={async event => { event.preventDefault(); if (await onExtract(password)) { setPassword(''); onClose(); } }}><h2>{t('archivePasswordTitle')}</h2><p>{t('archivePasswordHint')}</p><label>{t('archivePassword')}<input type="password" autoComplete="off" value={password} maxLength={1024} onChange={event => setPassword(event.target.value)} /></label><footer><button type="button" className="button secondary" disabled={busy} onClick={() => { setPassword(''); onClose(); }}>{t('cancel')}</button><button className="button primary" disabled={busy || !password}>{t('extractArchive')}</button></footer></form></dialog>;
}
