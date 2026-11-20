import { lazy, StrictMode, Suspense } from 'react';
import { createRoot } from 'react-dom/client';
import './theme.css';

// Two pages, chosen by path: / (live) and /rules; each loads only its own code.
const LivePage = lazy(() => import('./live/LivePage').then((m) => ({ default: m.LivePage })));
const RulesPage = lazy(() => import('./rules/RulesPage').then((m) => ({ default: m.RulesPage })));
const Page = location.pathname.startsWith('/rules') ? RulesPage : LivePage;

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Suspense fallback={<div className="page muted">loading…</div>}>
      <Page />
    </Suspense>
  </StrictMode>,
);
