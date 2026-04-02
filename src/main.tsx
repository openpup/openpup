import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import './styles.css';

window.addEventListener('error', (event) => {
  console.error('openpup frontend error:', event.error ?? event.message);
});

window.addEventListener('unhandledrejection', (event) => {
  console.error('openpup frontend unhandled rejection:', event.reason);
});

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
