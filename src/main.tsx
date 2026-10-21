import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { domAnimation, LazyMotion, MotionConfig } from 'motion/react';
import App from './App';
import { loadPrefs } from './lib/prefs';
import { applyTheme } from './lib/theme';
import './styles.css';

// 首帧前落定外观，避免深色模式下先闪一帧浅色
applyTheme(loadPrefs().theme);

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <LazyMotion features={domAnimation} strict>
      <MotionConfig
        reducedMotion="user"
        transition={{ type: 'spring', stiffness: 380, damping: 30 }}
      >
        <App />
      </MotionConfig>
    </LazyMotion>
  </StrictMode>,
);
