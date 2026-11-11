import { useLayoutEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { Toaster } from 'sonner';
import { Icon } from './Icon';

export function AppToaster({ container }: { container: '.settings-dialog' | '.log-dialog' | null }) {
  const { t } = useTranslation();
  const [host] = useState(() => document.createElement('div'));

  // Native modal dialogs make the rest of the document inert. Move the same
  // portal host into the active dialog so toast state and controls stay usable.
  useLayoutEffect(() => {
    (container ? document.querySelector(container) : document.body)?.appendChild(host);
  }, [container, host]);
  useLayoutEffect(() => () => host.remove(), [host]);

  return createPortal(<Toaster
    position="bottom-right"
    theme="light"
    closeButton
    expand
    duration={5000}
    offset={24}
    containerAriaLabel={t('notifications')}
    style={{ fontFamily: 'inherit' }}
    toastOptions={{
      closeButtonAriaLabel: t('dismiss'),
      classNames: { description: 'app-toast-description' },
    }}
    icons={{
      success: <Icon name="success" size={18} />,
      info: <Icon name="info" size={18} />,
      warning: <Icon name="warning" size={18} />,
      error: <Icon name="error" size={18} />,
      loading: <span className="spinner" />,
      close: <Icon name="close" size={12} />,
    }}
  />, host);
}
