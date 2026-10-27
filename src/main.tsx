import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { domAnimation, LazyMotion, MotionConfig } from 'motion/react';
import App from './App';
import TrayPanel from './panels/TrayPanel';
import { AppStateProvider } from './lib/store';
import { RelayProvider } from './lib/relay';
import { loadTheme, saveTheme } from './lib/prefs';
import { applyTheme } from './lib/theme';
import { IS_TRAY_VIEW } from './lib/view';
import './styles.css';

// 首帧前落定外观，避免深色模式下先闪一帧浅色。
// 旧版本的草稿键只在首次启动时读取，主题同时迁移到独立键。
const initialTheme = loadTheme();
saveTheme(initialTheme);
applyTheme(initialTheme);

// 菜单栏面板使用透明窗口 + CSS 圆角；主窗口保持默认画布背景。
if (IS_TRAY_VIEW) {
  document.documentElement.dataset.view = 'tray';
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <LazyMotion features={domAnimation} strict>
      <MotionConfig
        reducedMotion="user"
        transition={{ type: 'spring', stiffness: 380, damping: 30 }}
      >
        <AppStateProvider>
          <RelayProvider>{IS_TRAY_VIEW ? <TrayPanel /> : <App />}</RelayProvider>
        </AppStateProvider>
      </MotionConfig>
    </LazyMotion>
  </StrictMode>,
);
