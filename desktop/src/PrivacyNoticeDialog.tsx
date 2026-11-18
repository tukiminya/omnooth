import { useEffect, useRef } from 'react';
export function PrivacyNoticeDialog({ busy, onClose, onContinue, t }: { busy: boolean; onClose: () => void; onContinue: () => void; t: (key: string) => string }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => { dialog.current?.showModal(); }, []);
  return <dialog ref={dialog} className="metadata-dialog" aria-label={t('privacyNoticeTitle')} onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}><h2>{t('privacyNoticeTitle')}</h2><p className="plain-text">{t('privacyNoticeBody')}</p><footer><button className="button secondary" disabled={busy} onClick={onClose}>{t('cancel')}</button><button className="button primary" disabled={busy} onClick={onContinue}>{t('privacyContinue')}</button></footer></dialog>;
}
