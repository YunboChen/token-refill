import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { domAnimation, LazyMotion, MotionConfig } from 'motion/react';
import App from './App';
import { AppStateProvider } from './lib/store';
import { loadTheme, saveTheme } from './lib/prefs';
import { applyTheme } from './lib/theme';
import './styles.css';

// 首帧前落定外观，避免深色模式下先闪一帧浅色。
// 旧版本的草稿键只在首次启动时读取，主题同时迁移到独立键。
const initialTheme = loadTheme();
saveTheme(initialTheme);
applyTheme(initialTheme);

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <LazyMotion features={domAnimation} strict>
      <MotionConfig
        reducedMotion="user"
        transition={{ type: 'spring', stiffness: 380, damping: 30 }}
      >
        <AppStateProvider>
          <App />
        </AppStateProvider>
      </MotionConfig>
    </LazyMotion>
  </StrictMode>,
);
