import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import { client } from './client';

const root = document.getElementById('root');
if (!root) throw new Error('Root element not found');

window.addEventListener('error', (event) => {
  void client.reportError('window.error', event.message || 'Uncaught window error');
});
window.addEventListener('unhandledrejection', (event) => {
  const reason = event.reason;
  const message = reason instanceof Error ? reason.message : typeof reason === 'string' ? reason : 'Unhandled promise rejection';
  void client.reportError('unhandledrejection', message);
});

createRoot(root).render(<StrictMode><App /></StrictMode>);
